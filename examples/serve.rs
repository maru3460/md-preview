//! 開発用の最小 HTTP サーバ。`handle_request` をそのまま HTTP で公開し、
//! ブラウザ（Playwright）から実物のページを叩けるようにする。
//!
//! 製品バイナリには含まれない（`cargo install` は `[[bin]] md` だけを入れる）。
//! これは「UI の回帰テストのための足場」であって、ユーザー向けの機能ではない。
//!
//! ```sh
//! cargo run --example serve -- [--port <port>] <dir|file>...   # 既定ポート 7878
//! ```
//!
//! ファイルを 2 つ以上渡すと製品の `md a.md b.md` と同じ複数タブ起動になる
//! （ポートを位置引数から `--port` に追い出したのはこのため）。
//!
//! ウィンドウ表示との違い:
//!   - 起動スクリプト（`MD_*` グローバル＋ folder.js）は WKUserScript が無いので、
//!     配信する index HTML の `<head>` 直後へ差し込む。CSP を緩めないよう、
//!     ページが持っている nonce を読み出して使い回す。
//!   - IPC（`window.ipc`）が無いので、`ready` / `close` / メニュー操作を捨てる
//!     スタブを置く。これが無いと初回描画の最後で例外になり、以降が動かない。

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use md_preview::app_config::AppConfig;
use md_preview::request::{handle_request, percent_decode, RequestContext};
use md_preview::theme;

fn main() {
    let (port, paths) = parse_args(std::env::args().skip(1).collect());
    if paths.is_empty() {
        eprintln!("使い方: cargo run --example serve -- [--port <port>] <dir|file>...");
        std::process::exit(1);
    }

    let custom_css = md_preview::user_style_css();
    let (paint, appearance) = theme::resolve(&theme::read_active_name());
    let theme_css = theme::style_layer(appearance, &paint);
    let current_dir = std::env::current_dir().ok().and_then(|d| d.canonicalize().ok());

    // 1 つなら from_path、2 つ以上なら全部をタブに乗せる。製品の main.rs と同じ入口。
    let config = AppConfig::from_paths(&paths, &theme_css, &custom_css, &current_dir);

    let boot = format!(
        "{}\n{}\n{}",
        IPC_STUB,
        config.page_globals(appearance),
        md_preview::html::FOLDER_JS
    );
    let index = inject_boot_script(&config.html_bytes, &boot);

    let ctx = Arc::new(RequestContext {
        root_dir: config.root_dir.clone(),
        index_html: index,
        theme_css,
        custom_css,
    });

    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|e| {
        eprintln!("ポート {} を開けませんでした: {}", port, e);
        std::process::exit(1);
    });
    // Playwright 側がこの 1 行を待って接続する。
    println!("listening on http://127.0.0.1:{}", port);
    let _ = std::io::stdout().flush();

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let ctx = ctx.clone();
        std::thread::spawn(move || serve_one(stream, &ctx));
    }
}

/// `--port <port>` を取り出し、残りをパスとして返す。ポートを位置引数のままに
/// すると複数ファイル指定と区別が付かないので、フラグに切ってある。
fn parse_args(args: Vec<String>) -> (u16, Vec<String>) {
    let mut port = 7878;
    let mut paths = Vec::new();
    let mut rest = args.into_iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--port" => {
                port = rest.next().and_then(|p| p.parse().ok()).unwrap_or_else(|| {
                    eprintln!("--port には数字を渡してください");
                    std::process::exit(1);
                });
            }
            _ => paths.push(arg),
        }
    }
    (port, paths)
}

/// `window.ipc` のスタブ。ウィンドウ側では Rust が受けるものを、ここでは記録だけする
/// （テストから `window.__mdIpc` を見れば ready / close の発火を確認できる）。
const IPC_STUB: &str = "window.__mdIpc = []; \
window.ipc = { postMessage: function(m) { window.__mdIpc.push(m); } };";

/// 起動スクリプトを `<head>` 直後へ差し込む。CSP を維持するため、ページが持っている
/// nonce を読み出して同じものを付ける。
fn inject_boot_script(page: &[u8], boot: &str) -> Vec<u8> {
    let html = String::from_utf8_lossy(page).into_owned();
    let nonce = html
        .split_once("nonce=\"")
        .and_then(|(_, rest)| rest.split_once('"').map(|(n, _)| n.to_string()))
        .unwrap_or_default();
    let tag = format!("\n<script nonce=\"{}\">{}</script>", nonce, boot);
    // 最初の `<head>` が本物（生成ページは `<html>\n<head>` で始まる）。
    html.replacen("<head>", &format!("<head>{}", tag), 1).into_bytes()
}

fn serve_one(mut stream: TcpStream, ctx: &RequestContext) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    // ヘッダは読み捨てる（GET だけを相手にする）。
    loop {
        let mut h = String::new();
        match reader.read_line(&mut h) {
            Ok(0) => break,
            Ok(_) if h.trim().is_empty() => break,
            Ok(_) => {}
            Err(_) => return,
        }
    }

    // "GET /path?query HTTP/1.1"
    let target = line.split_whitespace().nth(1).unwrap_or("/");
    let (raw_path, query) = match target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (target, ""),
    };
    // ウィンドウ側（main.rs）と同じく、パスだけデコードしてから渡す。
    let url_path = percent_decode(raw_path);

    let resp = handle_request(ctx, &url_path, query);
    let status = resp.status().as_u16();
    let ctype = resp
        .headers()
        .get("Content-Type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let body = resp.body().to_vec();

    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        status,
        if status == 200 { "OK" } else { "Not Found" },
        ctype,
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}
