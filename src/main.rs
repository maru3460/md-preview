use std::borrow::Cow;
use std::path::Path;

use percent_encoding::percent_decode_str;
use pulldown_cmark::{html, Options, Parser};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

const CSS: &str = include_str!("style.css");
const HLJS_JS: &str = include_str!("highlight.min.js");
const HLJS_LIGHT_CSS: &str = include_str!("hljs-light.min.css");
const HLJS_DARK_CSS: &str = include_str!("hljs-dark.min.css");

const INIT_JS: &str = r#"
function addHeadingIds() {
    document.querySelectorAll('h1,h2,h3,h4,h5,h6').forEach(function(h) {
        if (!h.id) {
            h.id = h.textContent
                .toLowerCase()
                .replace(/[^\p{L}\p{N}\s-]/gu, '')
                .trim()
                .replace(/\s+/g, '-');
        }
    });
}
// DOMContentLoaded 後すぐにウィンドウを表示するとペイント前の白背景が一瞬見える。
// setTimeout で1フレーム分待ってペイント完了後に表示する。
document.addEventListener('DOMContentLoaded', function() {
    addHeadingIds();
    hljs.highlightAll();
    setTimeout(function() { window.ipc.postMessage('ready'); }, 50);
});
document.addEventListener('keydown', function(e) {
    if (e.metaKey && e.key === 'w') {
        e.preventDefault();
        window.ipc.postMessage('close');
    }
});
document.addEventListener('click', function(e) {
    var a = e.target.closest('a[href]');
    if (!a) return;
    var href = a.getAttribute('href');
    if (!href || href.charAt(0) !== '#') return;
    e.preventDefault();
    var id = decodeURIComponent(href.slice(1));
    var target = document.getElementById(id);
    if (target) target.scrollIntoView({ behavior: 'smooth' });
});
"#;

const MD_OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_FOOTNOTES);

fn render_body(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, MD_OPTIONS);
    let mut body = String::new();
    html::push_html(&mut body, parser);
    body
}

fn build_html(body: &str, title: &str, custom_css: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>{css}</style>
<style>{hljs_light}</style>
<style>@media(prefers-color-scheme:dark){{{hljs_dark}}}</style>
<style>{custom_css}</style>
<script>{hljs_js}</script>
</head>
<body>
<article class="markdown-body">
{body}
</article>
</body>
</html>"#,
        title = title,
        css = CSS,
        hljs_light = HLJS_LIGHT_CSS,
        hljs_dark = HLJS_DARK_CSS,
        custom_css = custom_css,
        hljs_js = HLJS_JS,
        body = body,
    )
}

fn guess_mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

#[cfg(target_os = "macos")]
fn get_frontmost_pid() -> Option<libc::pid_t> {
    use objc2_app_kit::NSWorkspace;
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    Some(app.processIdentifier())
}

#[cfg(target_os = "macos")]
fn activate_pid(pid: libc::pid_t) {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        #[allow(deprecated)]
        app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
    }
}

fn ok_response(content_type: &str, body: Vec<u8>) -> wry::http::Response<Cow<'static, [u8]>> {
    wry::http::Response::builder()
        .header("Content-Type", content_type)
        .body(Cow::Owned(body))
        .unwrap()
}

fn not_found_response() -> wry::http::Response<Cow<'static, [u8]>> {
    wry::http::Response::builder()
        .status(404)
        .body(Cow::Borrowed(b"Not Found" as &[u8]))
        .unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: md <file.md>");
        std::process::exit(1);
    }

    let path = std::path::Path::new(&args[1])
        .canonicalize()
        .unwrap_or_else(|e| {
            eprintln!("Error: cannot resolve '{}': {}", args[1], e);
            std::process::exit(1);
        });

    let base_dir = path.parent().unwrap().to_path_buf();

    let markdown = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("Error: cannot read '{}': {}", path.display(), e);
            std::process::exit(1);
        }
    };

    let title = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Markdown Preview")
        .to_string();

    let custom_css = std::env::var("HOME")
        .ok()
        .and_then(|home| {
            std::fs::read_to_string(format!("{}/.config/md-preview/style.css", home)).ok()
        })
        .unwrap_or_default();

    let html = build_html(&render_body(&markdown), &title, &custom_css);

    #[cfg(target_os = "macos")]
    let launcher_pid = get_frontmost_pid();

    let event_loop = EventLoopBuilder::<&'static str>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title(&title)
        .with_inner_size(LogicalSize::new(900.0_f64, 700.0_f64))
        .with_visible(false)
        .build(&event_loop)
        .expect("Failed to create window");

    let webview = WebViewBuilder::new()
        .with_initialization_script(INIT_JS)
        .with_navigation_handler(|url: String| {
            if url.starts_with("http://") || url.starts_with("https://") {
                std::process::Command::new("open").arg(&url).spawn().ok();
                false
            } else {
                true
            }
        })
        .with_custom_protocol("mdpreview".to_string(), {
            let html_bytes = html.into_bytes();
            let custom_css = custom_css;
            move |_webview_id, request| {
                let url_path = percent_decode_str(request.uri().path())
                    .decode_utf8_lossy();

                if url_path == "/" {
                    ok_response("text/html; charset=utf-8", html_bytes.clone())
                } else {
                    let relative = url_path.strip_prefix('/').unwrap_or(&url_path);
                    let file_path = base_dir.join(&*relative);
                    if file_path.extension().and_then(|e| e.to_str()) == Some("md") {
                        match std::fs::read_to_string(&file_path) {
                            Ok(content) => {
                                let title = file_path
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or("Markdown Preview");
                                let rendered =
                                    build_html(&render_body(&content), title, &custom_css);
                                ok_response("text/html; charset=utf-8", rendered.into_bytes())
                            }
                            Err(_) => not_found_response(),
                        }
                    } else {
                        match std::fs::read(&file_path) {
                            Ok(bytes) => ok_response(guess_mime(&file_path), bytes),
                            Err(_) => not_found_response(),
                        }
                    }
                }
            }
        })
        .with_ipc_handler(move |msg| {
            match msg.body().as_str() {
                "close" => { let _ = proxy.send_event("close"); }
                "ready" => { let _ = proxy.send_event("ready"); }
                _ => {}
            }
        })
        .with_url("mdpreview://localhost/")
        .build(&window)
        .expect("Failed to create WebView");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        let _ = &webview;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent("close") => {
                #[cfg(target_os = "macos")]
                if let Some(pid) = launcher_pid {
                    activate_pid(pid);
                }
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent("ready") => {
                window.set_visible(true);
            }
            _ => {}
        }
    });
}
