use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebouncedEventKind};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{RequestAsyncResponder, WebViewBuilder};

mod html;
mod platform;
mod request;

use html::{build_folder_html, build_html, json_string, parse_frontmatter, render_body, render_frontmatter_html, FOLDER_JS, INIT_JS};
use request::{handle_request, has_md_descendant, ok_response, percent_decode, safe_join};

enum AppEvent {
    Close,
    Ready,
    Reload(Option<String>),
}

const SAMPLE_MD: &str = include_str!("sample.md");

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 2 && args[1] == "--sample" {
        print!("{}", SAMPLE_MD);
        return;
    }

    let stdin_mode = (args.len() == 1 && !std::io::stdin().is_terminal())
        || (args.len() == 2 && args[1] == "-");

    if !stdin_mode && args.len() != 2 {
        eprintln!("Usage: md <file.md|directory>");
        eprintln!("       md -                     read markdown from stdin");
        eprintln!("       cat file.md | md         read markdown from stdin (implicit)");
        eprintln!("       md --sample              print sample markdown to stdout");
        std::process::exit(1);
    }

    let custom_css = std::env::var("HOME")
        .ok()
        .and_then(|home| {
            std::fs::read_to_string(format!("{}/.config/md-preview/style.css", home)).ok()
        })
        .unwrap_or_default();

    let current_dir = std::env::current_dir().ok()
        .and_then(|d| d.canonicalize().ok());

    let (title, init_script, html_bytes, window_width, root_dir, single_file_path, watch_enabled) = if stdin_mode {
        let mut markdown = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut markdown) {
            eprintln!("Error: cannot read stdin: {}", e);
            std::process::exit(1);
        }
        let title = "stdin".to_string();
        let (fm_pairs, body) = parse_frontmatter(&markdown);
        let fm_html = render_frontmatter_html(&fm_pairs);
        let html = build_html(&format!("{}{}", fm_html, render_body(body)), &title, &custom_css);
        let root = current_dir.clone().unwrap_or_else(|| PathBuf::from("."));
        (title, INIT_JS, html.into_bytes(), 900.0_f64, root, None, false)
    } else {
        let path = Path::new(&args[1])
            .canonicalize()
            .unwrap_or_else(|e| {
                eprintln!("Error: cannot resolve '{}': {}", args[1], e);
                std::process::exit(1);
            });

        let is_folder = path.is_dir();
        let file_in_cwd = !is_folder && current_dir.as_ref()
            .map(|cwd| path.starts_with(cwd))
            .unwrap_or(false);

        if is_folder {
            let title = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(".")
                .to_string();
            let html = build_folder_html(&title, &custom_css, None);
            (title, FOLDER_JS, html.into_bytes(), 1200.0_f64, path.clone(), None, true)
        } else if file_in_cwd {
            let cwd = current_dir.clone().unwrap();
            let rel = path.strip_prefix(&cwd)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let dir_title = cwd
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(".")
                .to_string();
            let html = build_folder_html(&dir_title, &custom_css, Some(&rel));
            (dir_title, FOLDER_JS, html.into_bytes(), 1200.0_f64, cwd, None, true)
        } else {
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
            let (fm_pairs, body) = parse_frontmatter(&markdown);
            let fm_html = render_frontmatter_html(&fm_pairs);
            let html = build_html(&format!("{}{}", fm_html, render_body(body)), &title, &custom_css);
            let base_dir = path.parent().unwrap_or(&path).to_path_buf();
            (title, INIT_JS, html.into_bytes(), 900.0_f64, base_dir, Some(path.clone()), true)
        }
    };

    #[cfg(target_os = "macos")]
    let launcher_pid = platform::get_frontmost_pid();

    let event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let _watcher = if watch_enabled {
        spawn_watcher(root_dir.clone(), single_file_path.clone(), proxy.clone())
    } else {
        None
    };

    let window = WindowBuilder::new()
        .with_title(&title)
        .with_inner_size(LogicalSize::new(window_width, 700.0_f64))
        .with_visible(false)
        .build(&event_loop)
        .expect("Failed to create window");

    let webview = WebViewBuilder::new()
        .with_initialization_script(init_script)
        .with_navigation_handler(|url: String| {
            if url.starts_with("http://") || url.starts_with("https://") {
                std::process::Command::new("open").arg(&url).spawn().ok();
                false
            } else {
                true
            }
        })
        .with_asynchronous_custom_protocol("mdpreview".to_string(), {
            let custom_css = custom_css;
            let single_file = single_file_path.clone();
            move |_webview_id, request, responder: RequestAsyncResponder| {
                let url_path = percent_decode(request.uri().path());
                let query = request.uri().query().unwrap_or("").to_string();

                if let Some(rel_encoded) = query.strip_prefix("has_md=") {
                    let rel = percent_decode(rel_encoded);
                    let root = root_dir.clone();
                    std::thread::spawn(move || {
                        let found = safe_join(&root, &rel)
                            .map(|p| has_md_descendant(&p))
                            .unwrap_or(false);
                        let body = format!(r#"{{"has_md":{}}}"#, found).into_bytes();
                        responder.respond(ok_response("application/json; charset=utf-8", body));
                    });
                    return;
                }

                responder.respond(handle_request(&url_path, &query, &root_dir, &html_bytes, &custom_css, single_file.as_deref()));
            }
        })
        .with_ipc_handler(move |msg| {
            match msg.body().as_str() {
                "close" => { let _ = proxy.send_event(AppEvent::Close); }
                "ready" => { let _ = proxy.send_event(AppEvent::Ready); }
                _ => {}
            }
        })
        .with_url("mdpreview://localhost/")
        .build(&window)
        .expect("Failed to create WebView");

    #[cfg(target_os = "macos")]
    platform::setup_menu();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(AppEvent::Close) => {
                #[cfg(target_os = "macos")]
                if let Some(pid) = launcher_pid {
                    platform::activate_pid(pid);
                }
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(AppEvent::Ready) => {
                window.set_visible(true);
            }
            Event::UserEvent(AppEvent::Reload(rel)) => {
                let arg = match rel {
                    Some(r) => json_string(&r),
                    None => "null".to_string(),
                };
                let script = format!("window.MdReload && window.MdReload({});", arg);
                let _ = webview.evaluate_script(&script);
            }
            _ => {}
        }
    });
}

fn spawn_watcher(
    root: PathBuf,
    single_file: Option<PathBuf>,
    proxy: tao::event_loop::EventLoopProxy<AppEvent>,
) -> Option<notify_debouncer_mini::Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>> {
    let root_for_cb = root.clone();
    let single_for_cb = single_file.clone();
    let mut debouncer = new_debouncer(Duration::from_millis(80), move |res: notify_debouncer_mini::DebounceEventResult| {
        let Ok(events) = res else { return };
        for ev in events {
            if !matches!(ev.kind, DebouncedEventKind::Any) { continue; }
            let path = match ev.path.canonicalize() {
                Ok(p) => p,
                Err(_) => ev.path.clone(),
            };
            if let Some(ref sf) = single_for_cb {
                if path == *sf {
                    let _ = proxy.send_event(AppEvent::Reload(None));
                }
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("md") { continue; }
            let rel = path.strip_prefix(&root_for_cb)
                .ok()
                .map(|p| p.to_string_lossy().into_owned());
            let _ = proxy.send_event(AppEvent::Reload(rel));
        }
    }).ok()?;

    let (watch_path, mode): (PathBuf, RecursiveMode) = match single_file.as_deref() {
        Some(f) => (f.parent().unwrap_or(Path::new(".")).to_path_buf(), RecursiveMode::NonRecursive),
        None => (root.clone(), RecursiveMode::Recursive),
    };
    debouncer.watcher().watch(&watch_path, mode).ok()?;
    Some(debouncer)
}
