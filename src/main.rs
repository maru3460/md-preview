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
mod theme;

use html::{build_folder_html, json_string, render_full_document, FOLDER_JS, INIT_JS};
use request::{handle_request, has_md_descendant, ok_response, percent_decode, safe_join};

enum AppEvent {
    Close,
    Ready,
    Reload(Option<String>),
}

/// ウィンドウ起動に必要な、入力モード（stdin / フォルダ / cwd内ファイル / 単一ファイル）
/// ごとに決まる設定一式。各 `build_*_config` がこれを組み立てて返す。
struct AppConfig {
    title: String,
    init_script: &'static str,
    html_bytes: Vec<u8>,
    window_width: f64,
    root_dir: PathBuf,
    single_file_path: Option<PathBuf>,
    watch_enabled: bool,
}

/// stdin から読んだ markdown を単一ページとして表示する設定。監視は行わない。
fn build_stdin_config(theme_css: &str, custom_css: &str, current_dir: &Option<PathBuf>) -> AppConfig {
    let mut markdown = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut markdown) {
        eprintln!("Error: cannot read stdin: {}", e);
        std::process::exit(1);
    }
    let title = "stdin".to_string();
    let html = render_full_document(&markdown, &title, theme_css, custom_css);
    let root = current_dir.clone().unwrap_or_else(|| PathBuf::from("."));
    AppConfig {
        title,
        init_script: INIT_JS,
        html_bytes: html.into_bytes(),
        window_width: 900.0,
        root_dir: root,
        single_file_path: None,
        watch_enabled: false,
    }
}

/// 引数で渡されたパスを解決し、フォルダ / cwd 内ファイル / 単一ファイルの
/// いずれかに応じた設定を組み立てる。
fn build_path_config(arg: &str, theme_css: &str, custom_css: &str, current_dir: &Option<PathBuf>) -> AppConfig {
    let path = Path::new(arg)
        .canonicalize()
        .unwrap_or_else(|e| {
            eprintln!("Error: cannot resolve '{}': {}", arg, e);
            std::process::exit(1);
        });

    let is_folder = path.is_dir();
    let file_in_cwd = !is_folder && current_dir.as_ref()
        .map(|cwd| path.starts_with(cwd))
        .unwrap_or(false);

    if is_folder {
        // フォルダ指定: ツリー付きの folder モード。root はそのフォルダ。
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(".")
            .to_string();
        let html = build_folder_html(&title, theme_css, custom_css, None);
        AppConfig {
            title,
            init_script: FOLDER_JS,
            html_bytes: html.into_bytes(),
            window_width: 1200.0,
            root_dir: path,
            single_file_path: None,
            watch_enabled: true,
        }
    } else if file_in_cwd {
        // cwd 配下のファイル: cwd を root にした folder モードで開き、その1枚を初期表示。
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
        let html = build_folder_html(&dir_title, theme_css, custom_css, Some(&rel));
        AppConfig {
            title: dir_title,
            init_script: FOLDER_JS,
            html_bytes: html.into_bytes(),
            window_width: 1200.0,
            root_dir: cwd,
            single_file_path: None,
            watch_enabled: true,
        }
    } else {
        // cwd 外の単一ファイル: 単一ページ表示。親ディレクトリを root にして監視する。
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
        let html = render_full_document(&markdown, &title, theme_css, custom_css);
        let base_dir = path.parent().unwrap_or(&path).to_path_buf();
        AppConfig {
            title,
            init_script: INIT_JS,
            html_bytes: html.into_bytes(),
            window_width: 900.0,
            root_dir: base_dir,
            single_file_path: Some(path),
            watch_enabled: true,
        }
    }
}

const SAMPLE_MD: &str = include_str!("sample.md");

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 2 && args[1] == "--sample" {
        print!("{}", SAMPLE_MD);
        return;
    }

    // `md theme [<name>]` — list or set the active theme. Handled before any
    // path resolution so a file literally named `theme` doesn't shadow it
    // (use `md ./theme` to open such a file).
    if args.len() >= 2 && args[1] == "theme" {
        run_theme_command(&args[2..]);
        return;
    }

    // `md --html <file.md> [theme]` — print the fully rendered page to stdout
    // instead of opening a window. Same `build_html` path as the live preview,
    // so the output is faithful to what the WebView shows. Used for headless
    // rendering (screenshot/visual checks) and snapshot tests. The optional
    // theme argument overrides which theme is rendered without touching the
    // user's saved active theme, so a screenshot tool can shoot light/dark
    // variants without disturbing their config.
    //
    // Dev/test-only: gated on `debug_assertions` so it is compiled out of
    // release builds entirely and never appears as a user-facing command.
    #[cfg(debug_assertions)]
    if (args.len() == 3 || args.len() == 4) && args[1] == "--html" {
        run_html_dump(&args[2], args.get(3).map(String::as_str));
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

    let (theme_paint, appearance) = theme::resolve(&theme::read_active_name());
    let theme_css = theme::style_layer(appearance, &theme_paint);

    let current_dir = std::env::current_dir().ok()
        .and_then(|d| d.canonicalize().ok());

    let AppConfig {
        title,
        init_script,
        html_bytes,
        window_width,
        root_dir,
        single_file_path,
        watch_enabled,
    } = if stdin_mode {
        build_stdin_config(&theme_css, &custom_css, &current_dir)
    } else {
        build_path_config(&args[1], &theme_css, &custom_css, &current_dir)
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

    // Expose the resolved theme's appearance to the page so JS-rendered
    // diagrams (mermaid) follow the theme instead of the OS dark-mode setting.
    let init_script = format!("window.MD_APPEARANCE = '{}';\n{}", appearance.as_str(), init_script);

    let webview = WebViewBuilder::new()
        .with_initialization_script(&init_script)
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
            let theme_css = theme_css;
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

                responder.respond(handle_request(&url_path, &query, &root_dir, &html_bytes, &theme_css, &custom_css, single_file.as_deref()));
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

fn hex_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some((r, g, b))
}

/// A seamless strip of truecolor blocks previewing a theme's palette.
fn swatch_strip(hexes: &[&str]) -> String {
    let mut s = String::new();
    for hex in hexes {
        if let Some((r, g, b)) = hex_rgb(hex) {
            s.push_str(&format!("\x1b[48;2;{};{};{}m  \x1b[0m", r, g, b));
        }
    }
    s
}

/// Grouped theme listing. On a TTY: color swatches per theme + the active one
/// marked with an accent dot. Piped: plain names so it stays greppable.
fn theme_list_text(active: &str, rich: bool) -> String {
    use theme::Appearance::{Auto, Dark, Light};
    let user = theme::user_theme_names();
    let mut s = String::new();

    if rich {
        s.push_str(&format!("\n  \x1b[1mthemes\x1b[0m  \x1b[2m· active: {}\x1b[0m\n", active));
    } else {
        s.push_str(&format!("themes (active: {})\n", active));
    }

    let group = |s: &mut String, label: &str, names: Vec<&theme::Theme>| {
        if names.is_empty() {
            return;
        }
        if rich {
            s.push_str(&format!("\n  \x1b[1;2m{}\x1b[0m\n", label));
        } else {
            s.push_str(&format!("\n{}\n", label));
        }
        for t in names {
            let is_active = t.name == active;
            let overridden = user.iter().any(|u| u == t.name);
            if rich {
                let marker = if is_active {
                    let (r, g, b) = hex_rgb(t.swatch[2]).unwrap_or((255, 255, 255));
                    format!("\x1b[38;2;{};{};{}m●\x1b[0m", r, g, b)
                } else {
                    " ".to_string()
                };
                let pad = " ".repeat(16usize.saturating_sub(t.name.chars().count()));
                let name = if is_active { format!("\x1b[1m{}\x1b[0m", t.name) } else { t.name.to_string() };
                let over = if overridden { "  \x1b[2m(overridden by user)\x1b[0m" } else { "" };
                s.push_str(&format!("  {} {}{}  {}{}\n", marker, name, pad, swatch_strip(&t.swatch), over));
            } else {
                let marker = if is_active { "*" } else { " " };
                let over = if overridden { "  (overridden by user)" } else { "" };
                s.push_str(&format!("  {} {}{}\n", marker, t.name, over));
            }
        }
    };

    group(&mut s, "light", theme::BUILTIN.iter().filter(|t| t.appearance == Light).collect());
    group(&mut s, "dark", theme::BUILTIN.iter().filter(|t| t.appearance == Dark).collect());
    group(&mut s, "auto · follows OS", theme::BUILTIN.iter().filter(|t| t.appearance == Auto).collect());

    let user_only: Vec<&String> = user
        .iter()
        .filter(|u| !theme::BUILTIN.iter().any(|t| t.name == u.as_str()))
        .collect();
    if !user_only.is_empty() {
        let header = if rich { "\n  \x1b[1;2muser\x1b[0m\n" } else { "\nuser\n" };
        s.push_str(header);
        for name in user_only {
            let marker = if rich {
                if name.as_str() == active { "\x1b[1m●\x1b[0m" } else { " " }
            } else if name.as_str() == active {
                "*"
            } else {
                " "
            };
            s.push_str(&format!("  {} {}\n", marker, name));
        }
    }
    s
}

fn run_theme_command(rest: &[String]) {
    match rest {
        [] => {
            let rich = std::io::stdout().is_terminal();
            print!("{}", theme_list_text(&theme::read_active_name(), rich));
        }
        [name] => {
            if !theme::theme_exists(name) {
                eprintln!("md: unknown theme '{}'", name);
                eprint!("{}", theme_list_text(&theme::read_active_name(), std::io::stderr().is_terminal()));
                std::process::exit(2);
            }
            if let Err(e) = theme::write_active_name(name) {
                eprintln!("md: cannot save theme: {}", e);
                std::process::exit(1);
            }
            println!("theme set to '{}'", name);
        }
        _ => {
            eprintln!("Usage: md theme [<name>]");
            std::process::exit(1);
        }
    }
}

#[cfg(debug_assertions)]
fn run_html_dump(arg: &str, theme_override: Option<&str>) {
    let markdown = match std::fs::read_to_string(arg) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("md: cannot read '{}': {}", arg, e);
            std::process::exit(1);
        }
    };
    let title = Path::new(arg)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Markdown Preview")
        .to_string();

    let custom_css = std::env::var("HOME")
        .ok()
        .and_then(|home| std::fs::read_to_string(format!("{}/.config/md-preview/style.css", home)).ok())
        .unwrap_or_default();
    let theme_name = theme_override
        .map(String::from)
        .unwrap_or_else(theme::read_active_name);
    let (theme_paint, appearance) = theme::resolve(&theme_name);
    let theme_css = theme::style_layer(appearance, &theme_paint);

    let html = render_full_document(&markdown, &title, &theme_css, &custom_css);
    print!("{}", html);
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
