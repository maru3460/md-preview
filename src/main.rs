use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebouncedEventKind};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{RequestAsyncResponder, WebViewBuilder};

mod platform;

use md_preview::theme;
use md_preview::html::{build_folder_html, build_html, html_escape, json_string, render_full_document, FOLDER_JS, INIT_JS};
use md_preview::request::{handle_request, has_md_descendant, ok_response, percent_decode, render_html_iframe, safe_join, source_view_html};

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
    /// 右クリックメニューの出し分けに使う実行モード。"folder" | "single" | "stdin"。
    menu_mode: &'static str,
    /// 単一ファイルモードで、コメントの file:line に使う相対パス（cwd 外なので basename）。
    /// folder / cwd モードは JS 側が現在ファイルの相対パスを持つので None。
    file_rel: Option<String>,
}

/// stdin から読んだ markdown を単一ページとして表示する設定。監視は行わない。
fn build_stdin_config(theme_css: &str, custom_css: &str, current_dir: &Option<PathBuf>) -> AppConfig {
    let mut markdown = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut markdown) {
        eprintln!("md: 標準入力を読み込めませんでした: {}", e);
        std::process::exit(1);
    }
    let title = "stdin".to_string();
    let html = render_full_document(&markdown, &title, theme_css, custom_css);
    let root = current_dir.clone().unwrap_or_else(|| PathBuf::from("."));
    AppConfig {
        title,
        init_script: INIT_JS,
        html_bytes: html.into_bytes(),
        // 見出しナビ(TOC)を初期表示できる幅。本文720px＋右サイドバーが収まる。
        window_width: 1100.0,
        root_dir: root,
        single_file_path: None,
        watch_enabled: false,
        menu_mode: "stdin",
        file_rel: None,
    }
}

/// 引数で渡されたパスを解決し、フォルダ / cwd 内ファイル / 単一ファイルの
/// いずれかに応じた設定を組み立てる。
fn build_path_config(arg: &str, theme_css: &str, custom_css: &str, current_dir: &Option<PathBuf>) -> AppConfig {
    let path = Path::new(arg)
        .canonicalize()
        .unwrap_or_else(|e| {
            eprintln!("md: '{}' を開けませんでした: {}", arg, e);
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
            // ファイルツリー(250px)＋本文＋見出しナビ(TOC)が並んでも収まる幅。
            window_width: 1280.0,
            root_dir: path,
            single_file_path: None,
            watch_enabled: true,
            menu_mode: "folder",
            file_rel: None,
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
            // ファイルツリー(250px)＋本文＋見出しナビ(TOC)が並んでも収まる幅。
            window_width: 1280.0,
            root_dir: cwd,
            single_file_path: None,
            watch_enabled: true,
            menu_mode: "folder",
            file_rel: None,
        }
    } else {
        // cwd 外の単一ファイル: 単一ページ表示。親ディレクトリを root にして監視する。
        // read_to_string だと非UTF-8(バイナリ)で即エラー終了し、GUI 起動では stderr が
        // 見えず「無反応」になる。フォルダの ?file= 経路と揃えて、バイトで読んで
        // 非UTF-8 なら「表示できません」窓を出す（読み込み自体の失敗だけ終了扱い）。
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("md: '{}' を読み込めませんでした: {}", path.display(), e);
                std::process::exit(1);
            }
        };
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Markdown Preview")
            .to_string();
        // .md はレンダリング、.html は iframe 描画、それ以外は行番号付きのソース表示
        // にする（folder / cwd モードの ?file= 経路と挙動を揃える）。
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let is_markdown = matches!(ext.as_deref(), Some("md") | Some("markdown"));
        let is_html_file = matches!(ext.as_deref(), Some("html") | Some("htm"));
        let html = if is_html_file {
            // html は中身をソースではなく iframe で描画する。root=親ディレクトリなので
            // src はファイル名。article に html-page クラスを付けて全幅・iframe 前提にする。
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or(&title);
            build_html(&render_html_iframe(name), &title, theme_css, custom_css, "html-page")
        } else {
            match String::from_utf8(bytes) {
                Ok(text) if is_markdown => render_full_document(&text, &title, theme_css, custom_css),
                Ok(text) => {
                    build_html(&source_view_html(&path, &text), &title, theme_css, custom_css, "source-page")
                }
                // バイナリも非md扱い（source-page）にして、init.js の raw 有効判定
                // （source-page 有無で見る）で raw トグルが出ないように揃える。
                Err(_) => build_html(
                    &format!(
                        r#"<p class="binary-msg">バイナリファイルは表示できません: {}</p>"#,
                        html_escape(&title)
                    ),
                    &title,
                    theme_css,
                    custom_css,
                    "source-page",
                ),
            }
        };
        let base_dir = path.parent().unwrap_or(&path).to_path_buf();
        // cwd 外の単一ファイルなので、コメントの file:line には basename を使う。
        let file_rel = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string());
        AppConfig {
            title,
            init_script: INIT_JS,
            html_bytes: html.into_bytes(),
            // 見出しナビ(TOC)を初期表示できる幅。本文720px＋右サイドバーが収まる。
            window_width: 1100.0,
            root_dir: base_dir,
            single_file_path: Some(path),
            watch_enabled: true,
            menu_mode: "single",
            file_rel,
        }
    }
}

const SAMPLE_MD: &str = include_str!("sample.md");

/// `--help` とエラー時のどちらでも使い回す使い方テキスト。
const USAGE: &str = "\
md - 高速Markdownプレビュー

使い方:
  md <file.md|dir>    ファイルかディレクトリをプレビュー表示します
  cat file.md | md    標準入力（パイプ）からMarkdownを読みます
  md theme [<name>]   テーマ一覧を表示、または <name> に切り替えます
  md --sample         サンプルのMarkdownを標準出力に出します
  md --help, -h       このヘルプを表示します
  md --version, -V    バージョンを表示します";

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() == 2 && (args[1] == "--help" || args[1] == "-h") {
        println!("{}", USAGE);
        return;
    }
    if args.len() == 2 && (args[1] == "--version" || args[1] == "-V") {
        println!("md {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    if args.len() == 2 && args[1] == "--sample" {
        print!("{}", SAMPLE_MD);
        return;
    }

    // `md theme [<name>]` — テーマの一覧表示 / 切り替え。`theme` という名前の
    // ファイルに邪魔されないよう、パス解決より前に処理する（そういうファイルを
    // 開きたいときは `md ./theme` を使う）。
    if args.len() >= 2 && args[1] == "theme" {
        run_theme_command(&args[2..]);
        return;
    }

    // `md --html <file.md> [theme]` — ウィンドウを開かず、完全に描画したページを
    // stdout へ出力する。ライブプレビューと同じ `build_html` を通るので、出力は
    // WebView の表示に忠実。ヘッドレス描画（スクリーンショット/表示確認）や
    // スナップショットテストで使う。省略可能な theme 引数は、ユーザーが保存した
    // 使用中テーマに触れずに描画対象のテーマだけを上書きするので、スクリーン
    // ショットツールが設定を乱さずライト/ダークを撮り分けられる。
    //
    // 開発/テスト専用: `debug_assertions` でゲートしており、リリースビルドからは
    // 完全にコンパイル除外され、ユーザー向けコマンドとしては現れない。
    #[cfg(debug_assertions)]
    if (args.len() == 3 || args.len() == 4) && args[1] == "--html" {
        run_html_dump(&args[2], args.get(3).map(String::as_str));
        return;
    }

    let stdin_mode = args.len() == 1 && !std::io::stdin().is_terminal();

    if !stdin_mode && args.len() != 2 {
        eprintln!("{}", USAGE);
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
        menu_mode,
        file_rel,
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

    // 解決済みテーマの appearance をページへ公開し、JS で描画する図（mermaid）が
    // OS のダークモード設定ではなくテーマに追従するようにする。MD_MENU_MODE は
    // 右クリックメニューのどの項目を有効化/表示するかを制御する。どちらも初期化
    // スクリプト（WKUserScript）経由で注入する。これはページのスクリプトより先に
    // 走り、ページの CSP の対象外なので、本文の inline script が禁止されていても
    // コンテキストメニューはこれらを読める。
    // コメントの file:line に使う単一ファイルの相対パス。folder / stdin は None なので
    // JS 側は空（folder は currentFilePath を使う / stdin はファイル無し）。
    let file_rel_js = match &file_rel {
        Some(s) => json_string(s),
        None => "''".to_string(),
    };
    let init_script = format!(
        "window.MD_APPEARANCE = '{}'; window.MD_MENU_MODE = '{}'; window.MD_FILE_REL = {};\n{}",
        appearance.as_str(),
        menu_mode,
        file_rel_js,
        init_script
    );

    // 下のカスタムプロトコルのクロージャが `root_dir` をムーブするので、IPC
    // ハンドラが必要とするもの（copy-abs/reveal/open のパス解決用）を先に clone する。
    let ipc_root = root_dir.clone();
    let ipc_single = single_file_path.clone();

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
            let body = msg.body().as_str();
            match body {
                "close" => { let _ = proxy.send_event(AppEvent::Close); }
                "ready" => { let _ = proxy.send_event(AppEvent::Ready); }
                _ => {
                    if let Some(rest) = body.strip_prefix("menu:") {
                        let (verb, payload) = rest.split_once(':').unwrap_or((rest, ""));
                        handle_menu(verb, payload, &ipc_root, ipc_single.as_deref());
                    }
                }
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

/// 右クリックメニュー由来の IPC（`menu:<verb>:<payload>`）を処理する。
/// 相対パス・選択テキストのコピーは JS 側で完結するので、ここに来るのは
/// 絶対パスコピー / Finder表示 / 既定アプリで開く の 3 つだけ。
fn handle_menu(verb: &str, payload: &str, root: &Path, single: Option<&Path>) {
    match verb {
        "abs" => {
            if let Some(p) = resolve_target(payload, root, single) {
                platform::copy_to_clipboard(&p.to_string_lossy());
            }
        }
        "reveal" => {
            if let Some(p) = resolve_target(payload, root, single) {
                platform::reveal_in_finder(&p);
            }
        }
        "open" => {
            if let Some(p) = resolve_target(payload, root, single) {
                if !is_blocked_ext(&p) {
                    platform::open_default(&p);
                }
            }
        }
        _ => {}
    }
}

/// メニュー操作対象の絶対パスを解決する。`rel` が空なら単一ファイルモードの
/// single_file_path を、非空なら `safe_join` で root 内に限定して解決する。
/// root 外（traversal・絶対パス・root 外へ向かう symlink）は None。
fn resolve_target(rel: &str, root: &Path, single: Option<&Path>) -> Option<PathBuf> {
    let rel = rel.trim();
    if rel.is_empty() {
        single.map(|p| p.to_path_buf())
    } else {
        safe_join(root, rel)
    }
}

/// `open` で起動すると任意コード実行・任意URL遷移になりうる拡張子を弾く。
/// CSP で本文の script は無効化済みだが、多層防御として実行系の起動を拒否する。
/// （`open` はテキスト系拡張子を実行せず既定エディタで開くだけなので、ここでは
/// 実際に「起動/遷移」しうる型だけを対象にする。）
fn is_blocked_ext(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some(
            "app" | "command" | "terminal" | "workflow" | "scpt" | "scptd" | "applescript"
                | "osascript" | "action" | "webloc" | "url" | "fileloc" | "shortcut"
                | "appex" | "xpc" | "prefpane" | "qlgenerator" | "vbs"
                // Java 実行系。html を iframe 描画するようになり、悪意ある html が
                // top.ipc 経由で menu:open を叩ける経路が増えたため実行系を塞いでおく。
                | "jar" | "jnlp"
        )
    )
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

/// テーマのパレットを見せる、truecolor ブロックを隙間なく並べた帯。
fn swatch_strip(hexes: &[&str]) -> String {
    let mut s = String::new();
    for hex in hexes {
        if let Some((r, g, b)) = hex_rgb(hex) {
            s.push_str(&format!("\x1b[48;2;{};{};{}m  \x1b[0m", r, g, b));
        }
    }
    s
}

/// テーマをグループ分けして一覧表示する。TTY ではテーマごとの色見本と、使用中の
/// ものにアクセント色のドットを付ける。パイプ時は grep しやすいよう素の名前だけ。
fn theme_list_text(active: &str, rich: bool) -> String {
    use theme::Appearance::{Auto, Dark, Light};
    let user = theme::user_theme_names();
    let mut s = String::new();

    if rich {
        s.push_str(&format!("\n  \x1b[1mテーマ\x1b[0m  \x1b[2m· 使用中: {}\x1b[0m\n", active));
    } else {
        s.push_str(&format!("テーマ（使用中: {}）\n", active));
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
                let over = if overridden { "  \x1b[2m（ユーザー定義で上書き）\x1b[0m" } else { "" };
                s.push_str(&format!("  {} {}{}  {}{}\n", marker, name, pad, swatch_strip(&t.swatch), over));
            } else {
                let marker = if is_active { "*" } else { " " };
                let over = if overridden { "  （ユーザー定義で上書き）" } else { "" };
                s.push_str(&format!("  {} {}{}\n", marker, t.name, over));
            }
        }
    };

    group(&mut s, "ライト", theme::BUILTIN.iter().filter(|t| t.appearance == Light).collect());
    group(&mut s, "ダーク", theme::BUILTIN.iter().filter(|t| t.appearance == Dark).collect());
    group(&mut s, "auto · OS設定に追従", theme::BUILTIN.iter().filter(|t| t.appearance == Auto).collect());

    let user_only: Vec<&String> = user
        .iter()
        .filter(|u| !theme::BUILTIN.iter().any(|t| t.name == u.as_str()))
        .collect();
    if !user_only.is_empty() {
        let header = if rich { "\n  \x1b[1;2mユーザー\x1b[0m\n" } else { "\nユーザー\n" };
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
                eprintln!("md: '{}' というテーマはありません", name);
                eprint!("{}", theme_list_text(&theme::read_active_name(), std::io::stderr().is_terminal()));
                std::process::exit(2);
            }
            if let Err(e) = theme::write_active_name(name) {
                eprintln!("md: テーマを保存できませんでした: {}", e);
                std::process::exit(1);
            }
            println!("テーマを '{}' に切り替えました", name);
        }
        _ => {
            eprintln!("使い方: md theme [<name>]");
            std::process::exit(1);
        }
    }
}

#[cfg(debug_assertions)]
fn run_html_dump(arg: &str, theme_override: Option<&str>) {
    // ライブプレビューと揃えて、非UTF-8(バイナリ)はエラー終了ではなく
    // 「表示できません」メッセージにする（読み込み自体の失敗だけ終了扱い）。
    let bytes = match std::fs::read(arg) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("md: '{}' を読み込めませんでした: {}", arg, e);
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

    // ライブプレビューと挙動を揃える。.md 以外は本文としてではなくソース表示にする。
    let path = Path::new(arg);
    let is_markdown = matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("md") | Some("markdown")
    );
    let html = match String::from_utf8(bytes) {
        Ok(text) if is_markdown => render_full_document(&text, &title, &theme_css, &custom_css),
        Ok(text) => {
            build_html(&source_view_html(path, &text), &title, &theme_css, &custom_css, "source-page")
        }
        // 単一ファイル経路と揃えてバイナリも source-page 扱いにする。
        Err(_) => build_html(
            &format!(
                r#"<p class="binary-msg">バイナリファイルは表示できません: {}</p>"#,
                html_escape(&title)
            ),
            &title,
            &theme_css,
            &custom_css,
            "source-page",
        ),
    };
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
            // レンダリング対象（md / html）の変更だけをホットリロードに回す。
            // 以前は "md" だけ見ており .markdown が漏れていた（＋html 未対応だった）。
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase());
            if !matches!(ext.as_deref(), Some("md") | Some("markdown") | Some("html") | Some("htm")) {
                continue;
            }
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
