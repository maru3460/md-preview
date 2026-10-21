//! 開発用の最小 HTTP サーバ。`handle_request` をそのまま HTTP で公開し、
//! ブラウザ（Playwright）から実物のページを叩けるようにする。
//!
//! 製品バイナリには含まれない（`cargo install` は `[[bin]] md` だけを入れる）。
//! これは「UI の回帰テストのための足場」であって、ユーザー向けの機能ではない。
//!
//! ```sh
//! cargo run --example serve -- [--port <port>] [--doc-scripts] [--external] <dir|file>...   # 既定ポート 7878
//! ```
//!
//! ファイルを 2 つ以上渡すと製品の `md a.md b.md` と同じ複数タブ起動になる
//! （ポートを位置引数から `--port` に追い出したのはこのため）。
//!
//! CSP の 2 つのスイッチ（`csp::Policy`）は `--doc-scripts` と `--external` で決める。
//! 利用者の実際の設定は読まない（UI テストが開発機の設定に左右されないように）。
//! `/__lib/` を許す住所は、本番の `mdpreview://localhost` ではなく自分の origin
//! （`http://127.0.0.1:<port>`）にする。
//!
//! ウィンドウ表示との違い:
//!   - 起動スクリプト（`MD_*` グローバル＋ folder.js）は WKUserScript が無いので、
//!     配信する index HTML の `<head>` 直後へ差し込む。CSP の meta より前に置くので、
//!     meta はこの script を縛らない（meta はそれより後ろにしか効かない）。ハッシュも要らない。
//!   - IPC（`window.ipc`）が無いので、`ready` / `close` / メニュー操作を捨てる
//!     スタブを置く。これが無いと初回描画の最後で例外になり、以降が動かない。

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use md_preview::app_config::{AppConfig, PageInputs};
use md_preview::csp::Policy;
use md_preview::request::{handle_request, percent_decode, HeaderMap, RequestContext};
use md_preview::theme;

fn main() {
    let Args { port, paths, policy } = parse_args(std::env::args().skip(1).collect());
    if paths.is_empty() {
        eprintln!("使い方: cargo run --example serve -- [--port <port>] [--doc-scripts] [--external] <dir|file>...");
        std::process::exit(1);
    }

    let custom_css = md_preview::user_style_css();
    let (paint, appearance, _) = theme::resolve(&theme::read_active_name());
    let theme_css = theme::style_layer(appearance, &paint);
    let current_dir = std::env::current_dir().ok().and_then(|d| d.canonicalize().ok());
    let origin = format!("http://127.0.0.1:{}", port);

    // 1 つなら from_path、2 つ以上なら全部をタブに乗せる。製品の main.rs と同じ入口。
    let page = PageInputs { theme_css: &theme_css, custom_css: &custom_css, policy, lib_origin: &origin };
    let config = AppConfig::from_paths(&paths, &page, &current_dir);

    let boot = format!(
        "{}\n{}\n{}",
        IPC_STUB,
        // Quick Access は空で始める。IPC が無いので登録も保存も効かないうえ、
        // ここで `~/.config` を読むと UI テストが手元の登録内容に左右される。
        config.page_globals(appearance, &[]),
        md_preview::html::FOLDER_JS
    );
    let index = inject_boot_script(&config.html_bytes, &boot);

    let ctx = Arc::new(RequestContext::new(config.root_dir.clone(), index, &page));

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

struct Args {
    port: u16,
    paths: Vec<String>,
    policy: Policy,
}

/// `--port <port>` と CSP のスイッチを取り出し、残りをパスとして返す。ポートを位置引数の
/// ままにすると複数ファイル指定と区別が付かないので、フラグに切ってある。
fn parse_args(args: Vec<String>) -> Args {
    let mut port = 7878;
    let mut paths = Vec::new();
    let mut policy = Policy::default();
    let mut rest = args.into_iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--port" => {
                port = rest.next().and_then(|p| p.parse().ok()).unwrap_or_else(|| {
                    eprintln!("--port には数字を渡してください");
                    std::process::exit(1);
                });
            }
            "--doc-scripts" => policy.doc_scripts = true,
            "--external" => policy.external = true,
            _ => paths.push(arg),
        }
    }
    Args { port, paths, policy }
}

/// `window.ipc` のスタブ。ウィンドウ側では Rust が受けるものを、ここでは記録だけする
/// （テストから `window.__mdIpc` を見れば close などの発火を確認できる。初期描画の
/// 完了は IPC ではなく `document.documentElement.dataset.mdReady` に出る）。
///
/// `root:` だけは記録では足りない。root が動くと**サーバの応答が変わる**（`?dir=` も
/// `?files=` も new root から答える）ので、ページ側だけ切り替えても嘘のツリーになる。
/// `/__setroot` へ回してから `MdSetRoot` を呼ぶ——ウィンドウ側で `main.rs` が
/// 「root を差し替えてから evaluate_script」とやっているのと同じ順序。
/// 解決できなかったときに `MdRootFailed` を呼ぶのも窓と同じ（#35。消えたフォルダを
/// 押したときに知らせる経路で、これが無いと UI テストから触れない）。
const IPC_STUB: &str = "window.__mdIpc = []; \
window.ipc = { postMessage: function(m) { \
  window.__mdIpc.push(m); \
  if (m.indexOf('root:') !== 0) return; \
  fetch('/__setroot?p=' + encodeURIComponent(m.slice(5))).then(function(r) { \
    return r.ok ? r.text() : null; \
  }).then(function(resolved) { \
    if (resolved && window.MdSetRoot) window.MdSetRoot(resolved); \
    else if (!resolved && window.MdRootFailed) window.MdRootFailed(m.slice(5)); \
  }); \
} };";

/// 起動スクリプトを `<head>` 直後へ差し込む。
///
/// 置き場所は CSP の meta より前でなければならない。meta はそれより後ろの script だけを
/// 縛るので、ここに置けばハッシュに載っていなくても動く（後ろに置くと止まる）。
/// 本番の初期化スクリプト（WKUserScript）が CSP の外で走るのと同じ立場にするため。
/// 位置はテスト（`boot_script_lands_before_the_csp_meta`）で押さえる。
fn inject_boot_script(page: &[u8], boot: &str) -> Vec<u8> {
    let html = String::from_utf8_lossy(page).into_owned();
    let tag = format!("\n<script>{}</script>", boot);
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
    // ヘッダは `handle_request` へ渡す（動画の Range を見る）。GET だけを相手にするので
    // 本文は読まない。
    let mut req_headers = HeaderMap::new();
    loop {
        let mut h = String::new();
        match reader.read_line(&mut h) {
            Ok(0) => break,
            Ok(_) if h.trim().is_empty() => break,
            Ok(_) => {
                if let Some((name, value)) = h.split_once(':') {
                    if let (Ok(n), Ok(v)) = (
                        md_preview::request::HeaderName::from_bytes(name.trim().as_bytes()),
                        md_preview::request::HeaderValue::from_str(value.trim()),
                    ) {
                        req_headers.append(n, v);
                    }
                }
            }
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

    // 開発用の口（#34）。製品の `handle_request` には root を動かす経路が無い
    // （持っているのは `main.rs` のイベントループ）ので、ここがその代役になる。
    // 判定はウィンドウ側と揃える——canonicalize して、ディレクトリでなければ断る。
    if url_path == "/__setroot" {
        let arg = query.strip_prefix("p=").map(percent_decode).unwrap_or_default();
        let resolved = std::path::PathBuf::from(arg).canonicalize().ok().filter(|p| p.is_dir());
        // **正規化した値を返す。** ウィンドウ側は `change_root` が canonicalize した
        // パスを `set_root_script` に載せるので、ここで生のままページへ返すと
        // `MD_ROOT_DIR` の形がテストと実機で食い違う（$TMPDIR は
        // `/var/folders/…` → `/private/var/…` のリンク）。
        let body = match &resolved {
            Some(p) => {
                ctx.set_root(p.clone());
                p.to_string_lossy().into_owned()
            }
            None => String::new(),
        };
        let head = format!(
            "HTTP/1.1 {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            if resolved.is_some() { "200 OK" } else { "404 Not Found" },
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body.as_bytes());
        let _ = stream.flush();
        return;
    }

    let resp = handle_request(ctx, &url_path, query, &req_headers);
    let status = resp.status();
    let body = resp.body().to_vec();

    // 応答のヘッダは全部そのまま通す（CSP・nosniff・Content-Range など、ウィンドウの側で
    // WebKit が受け取るものと同じものをブラウザにも見せる）。長さと接続はここで決める。
    let mut head = format!(
        "HTTP/1.1 {} {}\r\n",
        status.as_u16(),
        status.canonical_reason().unwrap_or("")
    );
    for (name, value) in resp.headers() {
        if name == "content-length" || name == "connection" {
            continue;
        }
        head.push_str(&format!("{}: {}\r\n", name, value.to_str().unwrap_or("")));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 起動用の script は CSP の meta より前に入る。後ろに入るとハッシュに無いので止まり、
    /// Playwright のページが初期化されない。
    #[test]
    fn boot_script_lands_before_the_csp_meta() {
        let page = md_preview::html::build_folder_html("t", "", "", &Policy::default(), "http://127.0.0.1:1");
        let out = String::from_utf8(inject_boot_script(page.as_bytes(), "BOOT")).unwrap();
        let boot = out.find("<script>BOOT</script>").expect("差し込まれていない");
        let meta = out.find("Content-Security-Policy").unwrap();
        assert!(boot < meta, "起動用の script が meta より後ろにある");
    }
}
