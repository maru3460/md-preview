//! 日本語入力（変換候補ウィンドウ）の切り分け用の最小ウィンドウ。
//!
//! md のページ（CSS / JS / CSP / カスタムプロトコル / 起動スクリプト）を一切通さず、
//! tao のウィンドウ + wry の WKWebView + 素の HTML だけを出す。症状が md のコード側か、
//! それより下（wry / tao / macOS）かを分けるための足場。
//!
//! 製品バイナリには含まれない（`cargo install` は `[[bin]] md` だけを入れる）。
//!
//! ```sh
//! cargo run --example ime            # ウィンドウを出す。入力欄で「さかな」+ space×2
//! IME_PROBE=1 cargo run --example ime  # ウィンドウを開かず CFBundle の状態だけ出す
//! IME_FIX=bundleid cargo run --example ime  # 実行時に identifier を注入して試す（効かない）
//! ```
//!
//! この足場で分かったこと（詳細は GitHub Issue #6）:
//!
//! 変換候補ウィンドウが出る条件は「**exec に使われたパスの隣に、ディスク上の実ファイルと
//! して `Info.plist` があること**」だけ。macOS の `current_exe()` / `_NSGetExecutablePath`
//! は symlink も `..` も解決しないので、見られるのは実体の隣ではなく起動に使ったパスの隣。
//!
//! - `.app` 拡張子も `Contents/MacOS/` 構造も要らない（ただのディレクトリに実行ファイルと
//!   `Info.plist` を並べるだけで出る）
//! - 署名も `CFBundleIdentifier` も `open` 経由も要らない
//! - バンドル内の実行ファイルが symlink なのは可。バンドル外の symlink から中の実バイナリを
//!   起動するのは不可
//! - プロセス内で身元を足しても届かない（実行時の infoDictionary 注入も、Mach-O の
//!   `__TEXT,__info_plist` への焼き込みも効かない）
//!
//! OS 更新で挙動が変わったときは、この足場で同じ表を取り直す。

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

// 素の HTML。JS 無し、CSS は position:fixed の検証に必要な分だけ。
const HTML: &str = r#"<!doctype html>
<html><head><meta charset="utf-8">
<style>
  body { font: 16px -apple-system, sans-serif; padding: 24px; margin: 0; }
  h3 { margin: 28px 0 6px; font-size: 14px; }
  textarea, input { font: inherit; padding: 6px; width: 380px; }
  .fixed { position: fixed; right: 24px; top: 24px;
           padding: 10px; background: #eee; border-radius: 8px; }
  .spacer { height: 1200px; }
</style></head>
<body>
<p>「さかな」と打って space を 2 回。変換候補のリストが出るか見る。</p>

<h3>1) 素の textarea（static）</h3>
<textarea rows="3"></textarea>

<h3>2) 素の input（static）</h3>
<input>

<div class="fixed">
  <div>3) position:fixed の中</div>
  <textarea rows="2"></textarea>
</div>

<div class="spacer"></div>

<h3>4) スクロールした先の textarea</h3>
<textarea rows="3"></textarea>
<div class="spacer"></div>
</body></html>"#;

/// `NSBundle.mainBundle` の infoDictionary へ CFBundleIdentifier を差し込む。
/// 素のバイナリの mainBundle は実行ファイルのあるディレクトリを指す“バンドルもどき”で、
/// bundleIdentifier は nil になる。これを埋めれば Text Input Services がアプリを
/// 識別できるようになるか、を確かめるための実験。
#[cfg(target_os = "macos")]
fn inject_bundle_identifier(ident: &str) -> String {
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::{msg_send, sel};
    use objc2_foundation::NSString;

    unsafe {
        let Some(cls) = AnyClass::get(c"NSBundle") else {
            return "NSBundle クラスが引けない".into();
        };
        let bundle: *mut AnyObject = msg_send![cls, mainBundle];
        if bundle.is_null() {
            return "mainBundle が nil".into();
        }
        let dict: *mut AnyObject = msg_send![bundle, infoDictionary];
        if dict.is_null() {
            return "infoDictionary が nil".into();
        }
        let responds: bool = msg_send![dict, respondsToSelector: sel!(setObject:forKey:)];
        if !responds {
            return "infoDictionary が immutable（setObject:forKey: を持たない）".into();
        }
        let key = NSString::from_str("CFBundleIdentifier");
        let val = NSString::from_str(ident);
        let _: () = msg_send![dict, setObject: &*val, forKey: &*key];

        let got: *mut AnyObject = msg_send![bundle, bundleIdentifier];
        if got.is_null() {
            "差し込んだが bundleIdentifier はまだ nil".into()
        } else {
            let s: *mut AnyObject = msg_send![got, description];
            let _ = s;
            format!("bundleIdentifier = {}", ident)
        }
    }
}

#[cfg(target_os = "macos")]
fn report_bundle_state(label: &str) {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use std::ffi::{c_char, CStr};

    unsafe fn ns_string(obj: *mut AnyObject) -> String {
        if obj.is_null() {
            return "(nil)".into();
        }
        let c: *const c_char = msg_send![obj, UTF8String];
        if c.is_null() {
            return "(nil)".into();
        }
        CStr::from_ptr(c).to_string_lossy().into_owned()
    }

    unsafe {
        let Some(cls) = AnyClass::get(c"NSBundle") else { return };
        let bundle: *mut AnyObject = msg_send![cls, mainBundle];

        let ident: *mut AnyObject = msg_send![bundle, bundleIdentifier];
        let url: *mut AnyObject = msg_send![bundle, bundleURL];
        let path: *mut AnyObject = if url.is_null() {
            std::ptr::null_mut()
        } else {
            msg_send![url, path]
        };
        let dict: *mut AnyObject = msg_send![bundle, infoDictionary];
        let count: usize = if dict.is_null() {
            0
        } else {
            msg_send![dict, count]
        };
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|e| format!("(err: {})", e));

        println!("[{}]", label);
        println!("  current_exe()      = {}", exe);
        println!("  mainBundle URL     = {}", ns_string(path));
        println!("  bundleIdentifier   = {}", ns_string(ident));
        println!("  infoDictionary     = {} 件", count);
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    {
        report_bundle_state("起動時");
        if std::env::var("IME_FIX").as_deref() == Ok("bundleid") {
            println!("[fix] {}", inject_bundle_identifier("dev.local.md-preview"));
            report_bundle_state("差し込み後");
        }
        // CFBundle の状態だけ見たいとき用。ウィンドウを開かずに終わる。
        if std::env::var_os("IME_PROBE").is_some() {
            return;
        }
    }

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("IME 切り分け（素の wry + tao）")
        .build(&event_loop)
        .expect("window");
    let _webview = WebViewBuilder::new()
        .with_html(HTML)
        .build(&window)
        .expect("webview");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}
