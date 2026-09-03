//! ウィンドウを開く経路だけを持つ。引数の振り分け・自己デタッチ・WebView の配線・
//! イベントループ・ファイル監視・右クリックメニューの IPC。
//!
//! ウィンドウを開かない処理（`--help` / `md theme` / `--html` ダンプ）は
//! [`md_preview::cli`]、起動設定の組み立ては [`md_preview::app_config`] にある。
//! どちらも GUI に依存しないのでライブラリ側に置いてテストできるようにしてある。

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, notify::RecursiveMode, DebouncedEventKind};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::{RequestAsyncResponder, WebViewBuilder};

mod platform;

use md_preview::app_config::{self, AppConfig};
use md_preview::cli;
use md_preview::html::json_string;
use md_preview::request::{self, handle_request, percent_decode};
use md_preview::theme;

enum AppEvent {
    Close,
    Ready,
    /// 変更されたファイルの識別子（root 相対パス、または root の外なら絶対パス）。
    /// ページ側は「いま開いているファイルか」を照合して再読込するかを決める。
    Reload(String),
}

/// 自己デタッチ後の子プロセスに「お前が本体だ」と伝える目印。
/// これが無いと、子がまた孫を起動して止まらない。
const DETACHED_ENV: &str = "MD_DETACHED";

/// 自分自身を別のプロセスグループで起動し直す。子の起動に成功したら true を返し、
/// 親はそのまま終了する。自分の実行ファイルが辿れないなど切り離せない事情がある
/// ときは false を返し、前景での表示に落とす（何も出ないより開いた方がよい）。
fn detach_self(stdin_mode: bool, targets: &[String], current_dir: &Option<PathBuf>) -> bool {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            eprintln!("md: 自分の実行ファイルが辿れないため前景で開きます: {}", e);
            return false;
        }
    };

    // 引数のエラーは、標準エラー出力を持っている親のうちに出しておく。子は stderr を
    // 持たないので、ここを素通りさせると「窓も出ずエラーも出ず終了コード 0」になる。
    // 本体の from_paths と同じ関門（開けないパス・フォルダ混在・root の広がり）を通す。
    if !targets.is_empty() {
        let _ = app_config::plan_paths(targets, current_dir);
    }

    let mut cmd = Command::new(exe);
    cmd.args(std::env::args().skip(1))
        .env(DETACHED_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // パイプで渡された markdown は子の標準入力には届かないので、親が読んで
    // 一時ファイルへ置き、その場所を渡す。後片付けは子（＝本体）が行う。
    if stdin_mode {
        cmd.env(app_config::STDIN_FILE_ENV, app_config::spool_stdin());
    }

    // 端末のプロセスグループから外す。呼び出し元がグループごと畳んでも巻き込まれない。
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    match cmd.spawn() {
        Ok(_) => true,
        Err(e) => {
            eprintln!("md: バックグラウンドで起動できませんでした: {}", e);
            std::process::exit(1);
        }
    }
}

/// ウィンドウを開かずに済むサブコマンドを処理する。処理したら true。
fn run_terminal_command(args: &[String]) -> bool {
    if args.len() == 2 && (args[1] == "--help" || args[1] == "-h") {
        println!("{}", cli::USAGE);
        return true;
    }
    if args.len() == 2 && (args[1] == "--version" || args[1] == "-V") {
        println!("md {}", env!("CARGO_PKG_VERSION"));
        return true;
    }
    if args.len() == 2 && args[1] == "--sample" {
        print!("{}", cli::SAMPLE_MD);
        return true;
    }

    // `md theme [<name>]` — テーマの一覧表示 / 切り替え。`theme` という名前の
    // ファイルに邪魔されないよう、パス解決より前に処理する（そういうファイルを
    // 開きたいときは `md ./theme` を使う）。
    if args.len() >= 2 && args[1] == "theme" {
        cli::run_theme_command(&args[2..]);
        return true;
    }

    // `md uninstall` — md が置いた設定・データを片付ける。`theme` と同じく、
    // `uninstall` という名前のファイルに邪魔されないようパス解決より前に処理する
    // （そういうファイルを開きたいときは `md ./uninstall`）。
    // ここで処理を終えるので、この後ろのバンドルへの乗り換えも通らない。
    if args.len() >= 2 && args[1] == "uninstall" {
        md_preview::uninstall::run(&args[2..]);
        return true;
    }

    // `md --html <file> [theme]` — ウィンドウを開かず、描画したページを stdout へ。
    // 省略可能な theme 引数は、ユーザーが保存した使用中テーマに触れずに描画対象の
    // テーマだけを上書きするので、スクリーンショットツールが設定を乱さずライト/
    // ダークを撮り分けられる。
    //
    // 開発/テスト専用: `debug_assertions` でゲートしており、リリースビルドからは
    // 完全にコンパイル除外され、ユーザー向けコマンドとしては現れない。
    #[cfg(debug_assertions)]
    if (args.len() == 3 || args.len() == 4) && args[1] == "--html" {
        cli::run_html_dump(&args[2], args.get(3).map(String::as_str));
        return true;
    }

    false
}

fn main() {
    // デタッチ指定はどの位置に書いてもよいので、先に抜き取っておく。
    // 以降の引数の数え方は従来どおりでよくなる。
    let mut args: Vec<String> = Vec::new();
    let mut detach_flag: Option<bool> = None;
    for (i, arg) in std::env::args().enumerate() {
        match arg.as_str() {
            "--detach" if i > 0 => detach_flag = Some(true),
            "--no-detach" if i > 0 => detach_flag = Some(false),
            _ => args.push(arg),
        }
    }

    if run_terminal_command(&args) {
        return;
    }

    let stdin_mode = args.len() == 1 && !std::io::stdin().is_terminal();

    // ファイルは何個でも受ける（2 つ以上ならタブとして並べて開く）。
    if !stdin_mode && args.len() < 2 {
        eprintln!("{}", cli::USAGE);
        std::process::exit(1);
    }

    let current_dir = std::env::current_dir().ok().and_then(|d| d.canonicalize().ok());

    // macOS で日本語入力の変換候補パネルを出すため、最小のバンドルへ乗り換える
    // （成功するとここから戻らない）。ウィンドウを開かない経路を通したくないので
    // run_terminal_command と引数チェックの後、自己デタッチの判定より前に置く。
    // ここより後ろだと、乗り換え後の current_exe() を detach_self が使えない。
    md_preview::bundle::relaunch_in_flat_bundle();

    // ここから先はウィンドウを開く経路。コマンドを終了せずに待たせると、
    // 待ち時間に上限のある呼び出し元（エージェントのコマンド実行ツールなど）が
    // 上限に達したときにプロセスグループごと畳み、ウィンドウまで消えてしまう。
    // そこで自分自身を別のプロセスグループで起動し直し、親は即座に終了する。
    // 端末から人が叩いたときは前景のままにして、Ctrl-C で閉じられる形を保つ。
    if std::env::var_os(DETACHED_ENV).is_none() {
        let detach = detach_flag.unwrap_or_else(|| !std::io::stdout().is_terminal());
        let targets: &[String] = if args.len() > 1 { &args[1..] } else { &[] };
        if detach && detach_self(stdin_mode, targets, &current_dir) {
            return;
        }
    }

    let custom_css = md_preview::user_style_css();
    let (theme_paint, appearance) = theme::resolve(&theme::read_active_name());
    let theme_css = theme::style_layer(appearance, &theme_paint);

    let config = if stdin_mode {
        AppConfig::from_stdin(&theme_css, &custom_css, &current_dir)
    } else {
        AppConfig::from_paths(&args[1..], &theme_css, &custom_css, &current_dir)
    };
    // ページへ注入する起動スクリプト。ウィンドウを作る前に組み立てる（下で config を
    // 部分ムーブするため）。
    let init_script = format!("{}\n{}", config.page_globals(appearance), md_preview::html::FOLDER_JS);
    let AppConfig {
        title,
        html_bytes,
        root_dir,
        stdin_dir,
    } = config;

    #[cfg(target_os = "macos")]
    let launcher_pid = platform::get_frontmost_pid();

    let event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let watcher = spawn_watcher(root_dir.clone(), proxy.clone());
    // root の外のファイルを開いたときに、そのファイルを監視へ足すため IPC から触る。
    // 監視は root の再帰監視だけなので、これが無いと root 外はホットリロードが効かない。
    let watcher = std::sync::Arc::new(std::sync::Mutex::new(watcher));
    let ipc_watcher = watcher.clone();

    let window = WindowBuilder::new()
        .with_title(&title)
        .with_inner_size(LogicalSize::new(app_config::WINDOW_WIDTH, app_config::WINDOW_HEIGHT))
        .with_visible(false)
        .build(&event_loop)
        .expect("Failed to create window");

    // 下のカスタムプロトコルのクロージャが `root_dir` をムーブするので、IPC
    // ハンドラが必要とするもの（copy-abs/reveal/open のパス解決用）を先に clone する。
    let ipc_root = root_dir.clone();

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
            // macOS では WKWebView がこのクロージャをメインスレッドで呼ぶ。ここで
            // 同期処理をすると、その間ウィンドウが固まる（ファイル一覧の全走査・git の
            // 子プロセス・diff の LCS が該当）。以前は has_md だけを個別にスレッドへ
            // 逃がしていたが、理由はどのリクエストにも当てはまるので一律で逃がす。
            //
            // リクエストごとにスレッドを立てる。ローカルファイルの読み出しが主で
            // 個々は短命なので、プールを挟んで重いリクエストの後ろに軽いリクエストが
            // 詰まる（画像が 1 枚ずつしか出ない等）弊害の方を避ける。
            let ctx = std::sync::Arc::new(request::RequestContext {
                root_dir: root_dir.clone(),
                index_html: html_bytes,
                theme_css,
                custom_css,
            });
            move |_webview_id, request, responder: RequestAsyncResponder| {
                let url_path = percent_decode(request.uri().path());
                let query = request.uri().query().unwrap_or("").to_string();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    responder.respond(handle_request(&ctx, &url_path, &query));
                });
            }
        })
        .with_ipc_handler(move |msg| {
            // 正規の IPC はすべて自前のスクリプト（＝トップフレーム）から来る。
            // html ファイルを描く iframe は sandbox 無しなので、その中の script も
            // top.ipc を叩けてしまう。クリップボードもパス操作も渡さない。
            if !is_top_frame(msg.uri()) {
                return;
            }
            let body = msg.body().as_str();
            match body {
                "close" => { let _ = proxy.send_event(AppEvent::Close); }
                "ready" => { let _ = proxy.send_event(AppEvent::Ready); }
                _ => {
                    if let Some(rest) = body.strip_prefix("menu:") {
                        let (verb, payload) = rest.split_once(':').unwrap_or((rest, ""));
                        handle_menu(verb, payload, &ipc_root);
                    } else if let Some(abs) = body.strip_prefix("watch:") {
                        watch_extra(&ipc_watcher, Path::new(abs));
                    } else if let Some(text) = body.strip_prefix("copy:") {
                        platform::copy_to_clipboard(text);
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
                // stdin を実体化した一時ファイルはウィンドウと寿命を揃える
                // （表示中はドキュメントそのものなので、読んだ直後には消せない）。
                // ⌘Q（AppKit の terminate）はここを通らないので取り残しうるが、
                // 置き場所が $TMPDIR なので OS の掃除に任せる。
                if let Some(dir) = &stdin_dir {
                    let _ = std::fs::remove_dir_all(dir);
                }
                #[cfg(target_os = "macos")]
                if let Some(pid) = launcher_pid {
                    platform::activate_pid(pid);
                }
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(AppEvent::Ready) => {
                window.set_visible(true);
            }
            Event::UserEvent(AppEvent::Reload(id)) => {
                let script = format!("window.MdReload && window.MdReload({});", json_string(&id));
                let _ = webview.evaluate_script(&script);
            }
            _ => {}
        }
    });
}

/// 右クリックメニュー由来の IPC（`menu:<verb>:<payload>`）を処理する。
/// ここに来るのは絶対パスコピー / Finder表示 / 既定アプリで開く の 3 つ——
/// いずれも payload が「パス」なので、`resolve_target` で解決してから触る。
/// 任意テキストのクリップボード書き込みはパス解決を通さない別の口（`copy:`）。
fn handle_menu(verb: &str, payload: &str, root: &Path) {
    let Some(path) = resolve_target(payload, root) else { return };
    match verb {
        "abs" => platform::copy_to_clipboard(&path.to_string_lossy()),
        "reveal" => platform::reveal_in_finder(&path),
        "open" => {
            if !is_blocked_ext(&path) {
                platform::open_default(&path);
            }
        }
        _ => {}
    }
}

/// IPC の送信元がトップフレーム（自前のスクリプトが動く文書）か。
///
/// wry は `WKScriptMessage.frameInfo` の URL を `Request` の URI に載せてくる。
/// トップは `with_url` で読んだ `mdpreview://localhost/` のままなので、そこに
/// 完全一致するかで見る。html を描く iframe の src は必ずファイルのパス
/// （`/rel` か `/__abs/abs`）なので一致しない。
///
/// 判定は必ず許可制で書く。「iframe だと分かる形だけ弾く」という否定形にすると、
/// `about:blank` / `about:srcdoc` の無名サブフレームを通してしまう——`http::Uri` は
/// `//` を持たない `scheme:opaque` を authority として読むので、あれらは
/// scheme=None・path="" になり、パスだけを見る判定では素通りする。
///
/// これで塞げない経路が 1 つ残る。悪意ある html が `<iframe src="/">` を作ると
/// アプリのシェルがそのフレームに読み込まれ、URL は `mdpreview://localhost/` に
/// なる。`frameInfo` は「messageHandlers を持つフレーム」に紐づくので、そこ経由で
/// 投げられた IPC はトップと見分けが付かない。URI では原理的に判定できないため、
/// ここは「事故を減らす衛生」であって信頼できない html への対策ではない。
/// 本来の防御は README のとおり「信頼できない .html を開かない」ことである。
fn is_top_frame(uri: &wry::http::Uri) -> bool {
    uri.scheme_str() == Some("mdpreview")
        && uri.authority().map(|a| a.as_str()) == Some("localhost")
        && uri.path() == "/"
}

/// メニュー操作対象の絶対パスを解決する。`id` は `?file=` と同じ識別子
/// （root 相対なら root 内に限定、絶対パスなら root の外でも可）。
/// プレビューに何も開いていないときは空で来るので、その場合は何もしない。
fn resolve_target(id: &str, root: &Path) -> Option<PathBuf> {
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    request::id_to_path(root, id)
}

/// root の外のファイルを監視対象に足す。エディタの「別ファイルを書いて rename」に
/// 耐えるよう、ファイルそのものではなく親ディレクトリを非再帰で見る。
/// 既に見ている場所を重ねて watch しても notify 側が畳むので、重複管理はしない。
fn watch_extra(
    watcher: &std::sync::Mutex<Option<notify_debouncer_mini::Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>>>,
    file: &Path,
) {
    let Some(dir) = file.parent() else { return };
    let Ok(mut guard) = watcher.lock() else { return };
    if let Some(d) = guard.as_mut() {
        let _ = d.watcher().watch(dir, RecursiveMode::NonRecursive);
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

fn spawn_watcher(
    root: PathBuf,
    proxy: tao::event_loop::EventLoopProxy<AppEvent>,
) -> Option<notify_debouncer_mini::Debouncer<notify_debouncer_mini::notify::RecommendedWatcher>> {
    let root_for_cb = root.clone();
    let mut debouncer = new_debouncer(Duration::from_millis(80), move |res: notify_debouncer_mini::DebounceEventResult| {
        let Ok(events) = res else { return };
        for ev in events {
            if !matches!(ev.kind, DebouncedEventKind::Any) { continue; }
            let path = match ev.path.canonicalize() {
                Ok(p) => p,
                Err(_) => ev.path.clone(),
            };
            // レンダリング対象（md / html）の変更だけをホットリロードに回す。
            // 判定は request::is_renderable（RENDERABLE_EXT）に委ねる。以前はここで
            // 拡張子を書き並べており、.markdown が漏れる回帰を起こしていた。
            if !request::is_renderable(&path) {
                continue;
            }
            // JS 側が持っている識別子（root 相対 or 絶対パス）と同じ形で通知する。
            // 形がズレると「開いているファイルが変わったか」の照合が外れて再読込しない。
            let id = request::file_id(&root_for_cb, &path);
            let _ = proxy.send_event(AppEvent::Reload(id));
        }
    }).ok()?;

    // root 配下は再帰で見る。root の外のファイルはページから watch: が飛んでくるので
    // watch_extra が個別に足す。
    debouncer.watcher().watch(&root, RecursiveMode::Recursive).ok()?;
    Some(debouncer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn top(s: &str) -> bool {
        is_top_frame(&s.parse::<wry::http::Uri>().unwrap())
    }

    #[test]
    fn top_frame_accepts_only_the_app_page() {
        // with_url が読む URL そのもの。query / fragment が付いても本体は同じ。
        assert!(top("mdpreview://localhost/"));
        assert!(top("mdpreview://localhost"));  // path は "/" に正規化される
        assert!(top("mdpreview://localhost/?file=a.md"));
        assert!(top("mdpreview://localhost/#sec"));
    }

    #[test]
    fn html_iframes_are_rejected() {
        // html を描く iframe。src は asset_url が組むファイルのパス。
        assert!(!top("mdpreview://localhost/docs/page.html"));
        assert!(!top("mdpreview://localhost/__abs/Users/me/page.html"));
        assert!(!top("mdpreview://localhost/a.html?x=1"));
    }

    #[test]
    fn anonymous_subframes_are_rejected() {
        // これを通すのが「パスだけ見る」判定の穴だった。`http::Uri` は `//` の無い
        // `scheme:opaque` を authority として読むので、どれも path == "" になる。
        for u in ["about:blank", "about:srcdoc", "javascript:void(0)"] {
            assert_eq!(u.parse::<wry::http::Uri>().unwrap().path(), "", "{u}");
            assert!(!top(u), "{u}");
        }
        // パスだけの相対 URI も、トップとは名乗れない。
        assert!(!top("/"));
        assert!(!is_top_frame(&wry::http::Uri::default()));
    }

    #[test]
    fn other_origins_are_rejected() {
        assert!(!top("mdpreview://evil/"));
        assert!(!top("https://localhost/"));
        // file:/// は http::Uri がパースできない（authority が空）。wry がここへ
        // 届ける前に捨てるので、到達しないことだけ記録しておく。
        assert!("file:///".parse::<wry::http::Uri>().is_err());
    }
}
