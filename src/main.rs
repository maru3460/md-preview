//! ウィンドウを開く経路だけを持つ。引数の振り分け・既存インスタンスへの転送と
//! 受け側の座取り（#31）・自己デタッチ・WebView の配線・イベントループ・
//! ファイル監視・右クリックメニューの IPC。
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
use tao::window::{Theme as OsTheme, WindowBuilder};
use wry::{RequestAsyncResponder, WebViewBuilder};

mod platform;

use md_preview::app_config::{self, AppConfig};
use md_preview::cli;
use md_preview::html::json_string;
use md_preview::request::{self, handle_request, percent_decode};
use md_preview::theme;

enum AppEvent {
    Close,
    /// 変更されたファイルの識別子（絶対パス）。
    /// ページ側は「いま開いているファイルか」を照合して再読込するかを決める。
    Reload(String),
    /// 別プロセスの md から転送されてきた「これをタブで開け」（#31）。
    Open(md_preview::instance::Message),
    /// ページが `MdOpenFiles` を受けられる状態になった合図。
    /// 窓は中身を待たずに出るので、これより前の `Open` は溜めておく。
    Ready,
    /// 窓が全画面から抜け終わった（#59）。AppKit の通知を
    /// `platform::watch_exit_fullscreen` で受けて、ここへ流し直している。
    ExitedFullscreen,
    /// 全画面から抜けるのを待つ期限が来た（#59）。
    CloseDeadline,
}

/// 自己デタッチ後の子プロセスに「お前が本体だ」と伝える目印。
/// これが無いと、子がまた孫を起動して止まらない。
const DETACHED_ENV: &str = "MD_DETACHED";

/// デタッチをやめて前景で開く逃げ道。窓を持つプロセスの panic や出力は
/// デタッチすると /dev/null へ行くので、それを読みたい開発時のためにある。
/// フラグではなく環境変数なのは、人が日常で使うものではないから（`MD_NO_BUNDLE`
/// と同じ扱い）。
const NO_DETACH_ENV: &str = "MD_NO_DETACH";

/// 単一インスタンス化そのものを切る逃げ道。
///
/// Why not `MD_NO_DETACH` に含める: 目的が違う。`MD_NO_DETACH` は「窓を持つプロセスの
/// 出力を読みたい」で、こちらは「既存の窓へ送らず自分で開きたい」。兼務させると、
/// 転送を切らずにログだけ読みたいときに逃げ場が無くなる。
const NO_IPC_ENV: &str = "MD_NO_IPC";

/// 層2 で受け側の座を取れなかったとき、勝った方が bind するのを待つ上限。
/// 所有者が居ることは flock で分かっているので、まだ bind していないだけなら待つ
/// 価値がある（層1 と違って「誰も居ない」可能性は無い）。
const SEAT_WAIT: Duration = Duration::from_secs(2);
const SEAT_POLL: Duration = Duration::from_millis(20);

/// 自分自身を別のプロセスグループで起動し直す。子の起動に成功したら true を返し、
/// 親はそのまま終了する。自分の実行ファイルが辿れないなど切り離せない事情がある
/// ときは false を返し、前景での表示に落とす（何も出ないより開いた方がよい）。
///
/// `argv` は実行ファイル名を除いた引数で、フラグを含んだまま子へ渡す（子も
/// `split_open_flags` を通るので、`-n` は子まで届く必要がある）。`targets` はそこから
/// フラグを剥がしたパスで、検証に使う。
fn detach_self(
    stdin_mode: bool,
    argv: &[String],
    current_dir: &Option<PathBuf>,
    targets: &[String],
) -> bool {
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

    // 子の口は 3 つとも /dev/null にする。端末へ繋ぐと、窓を持つプロセスが吐く
    // AppKit / 入力メソッドのログ（IMKCFRunLoopWakeUpReliable など）が md の名前で
    // 混ざる。パイプへ繋ぐと、握ったままウィンドウが生き続けて呼び出し元が EOF 待ちで
    // 戻らなくなる。人に見せる価値があるのは「窓が出ない」エラーだけで、それは上の
    // plan_paths と、main の頭の引数チェック・stdin の実体化で親が出し切っている。
    //
    // 子へ渡すのは env::args() の取り直しではなく、親が受け取った argv そのもの。
    // 取り直すと「検証したもの」と「渡すもの」が別々に育って食い違える。
    // フラグを剥がさないのは、子も `split_open_flags` を通るから（`-n` が子まで
    // 届かないと、座を取り損ねた 2 枚目が転送に回ってしまう）。
    let mut cmd = Command::new(exe);
    cmd.args(argv)
        .env(DETACHED_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // パイプで渡された markdown は子の標準入力には届かない。実体化は main の頭で
    // 済ませて環境変数に置いてあるので、ここは（継承で足りるが）明示的に渡すだけ。
    // 後片付けは子（＝本体）が行う。
    if stdin_mode {
        if let Some(spooled) = std::env::var_os(app_config::STDIN_FILE_ENV) {
            cmd.env(app_config::STDIN_FILE_ENV, spooled);
        }
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

/// 既存の窓へ送ってよい場面か。`--new-window` と `MD_NO_IPC` で切れる。
///
/// 「送るか」と「受けるか」は別の判断で、`-n` が切るのは送る側だけである
/// （[`take_the_seat`] を参照）。
fn may_forward(flags: &cli::OpenFlags) -> bool {
    !flags.new_window && std::env::var_os(NO_IPC_ENV).is_none()
}

/// 転送する内容を組み立てる。転送に向かない引数なら `None`（従来どおり窓を開く）。
///
/// パスの検証（開けないパス・フォルダ混在・root の広がり）は `plan_paths` に任せる。
/// ここは stderr を持っている経路なので、落ちるなら人に見える形で落ちてよい。
fn message_to_forward(
    stdin_mode: bool,
    targets: &[String],
    current_dir: &Option<PathBuf>,
) -> Option<md_preview::instance::Message> {
    use md_preview::instance::Message;

    let ids = if stdin_mode {
        // パイプ入力も `md file.md` と同じ経路に乗せる。実体化は main の頭で済んで
        // いるので、ここはそのパスを識別子にするだけ。
        let doc = PathBuf::from(std::env::var_os(app_config::STDIN_FILE_ENV)?)
            .canonicalize()
            .ok()?;
        vec![request::file_id(&doc)]
    } else {
        // ディレクトリ引数は転送しない。タブに乗らないし、既存の窓の root を
        // 差し替える仕組みもまだ無い（#34）。ここを転送に回すと、#34 が入るまで
        // 「別のフォルダを開く」手段が完全に消える。**#34 が入ったら外す暫定。**
        if let [only] = targets {
            if Path::new(only).is_dir() {
                return None;
            }
        }
        app_config::plan_paths(targets, current_dir).1
    };
    if ids.is_empty() {
        return None;
    }

    let mut msg = Message::new(ids);
    msg.cwd = current_dir.as_ref().map(|d| d.to_string_lossy().into_owned());
    msg.sender_pid = Some(std::process::id() as i32);
    // stdin の一時ディレクトリは受け側が引き取る。**送り側は消さない**——消すと
    // 受け側が死んだパスを開くことになる。門を通らないものは載せない（＝誰も
    // 消さない。消し損ねる方が誤削除より安い）。
    //
    // ワイヤに載せるのは**実体化したファイルのパス**で、ディレクトリではない。
    // 門（`owned_stdin_dir`）は「ファイルを受け取って、消してよい親を返す」形なので、
    // ディレクトリを載せると受け側が同じ門を通したときに $TMPDIR の親を見ることに
    // なり、必ず弾かれる。送り側と受け側が**同じ値を同じ門に通す**のが要件。
    if stdin_mode {
        if let Some(doc) = std::env::var_os(app_config::STDIN_FILE_ENV) {
            let doc = PathBuf::from(doc);
            if app_config::owned_stdin_dir(&doc).is_some() {
                msg.own = vec![doc.to_string_lossy().into_owned()];
            }
        }
    }
    Some(msg)
}

/// 既に動いている md へ渡せたら true（呼び出し側はそのまま終了する）。
fn forward_to_running_instance(
    flags: &cli::OpenFlags,
    stdin_mode: bool,
    targets: &[String],
    current_dir: &Option<PathBuf>,
) -> bool {
    use md_preview::instance::Endpoint;

    if !may_forward(flags) {
        return false;
    }
    let Some(ep) = Endpoint::user_default() else { return false };
    let Some(msg) = message_to_forward(stdin_mode, targets, current_dir) else { return false };
    matches!(deliver(&ep, &msg), Delivery::Done)
}

/// [`deliver`] の結果。**`Retry` と `GiveUp` を潰してはいけない。** 潰すと層2 の
/// リトライが「話が通じないと分かっている相手」を 2 秒ぶん叩き続ける。
enum Delivery {
    /// 渡せた。呼び出し側はそのまま終了してよい。
    Done,
    /// まだ届かない。層2 なら待つ価値がある（所有者は居ると分かっているので）。
    Retry,
    /// 待っても無駄。自分で窓を開く。
    GiveUp,
}

/// 1 通送って、通ったら受け側を前面化する。
fn deliver(ep: &md_preview::instance::Endpoint, msg: &md_preview::instance::Message) -> Delivery {
    use md_preview::instance::{try_send, Sent};

    match try_send(ep, msg) {
        Sent::Delivered { receiver_pid } => {
            // ack は待たない。`spike/activation` の実測では、送り側が activate を
            // 撃ってから即死しても 41ms 後に着弾した。
            if msg.activate {
                if let Some(pid) = receiver_pid {
                    platform::activate_other(pid);
                }
            }
            Delivery::Done
        }
        Sent::Incompatible { version } => {
            // 古い窓が生きたまま `cargo install` で入れ替えるのは日常なので、
            // 黙って諦めずに理由を出す。待っても新しくならないのでリトライには回さない。
            //
            // 端末に出るのは層1（親）から呼ばれたときだけ。層2 はデタッチ済みの子なので
            // /dev/null へ行くが、同じ状況なら層1 で既に出ているので取りこぼさない。
            eprintln!("md: 動いている md（プロトコル {}）の方が新しいので、別の窓で開きます", version);
            Delivery::GiveUp
        }
        // 同じパスに別のプログラムが居る。待っても md にはならない。
        Sent::Stranger => Delivery::GiveUp,
        // 1 通に収まらない。受け側は捨てるので、自分で開く。
        Sent::TooLarge => Delivery::GiveUp,
        // まだ bind していないだけかもしれない。層2 はここを待つ。
        Sent::NoReceiver => Delivery::Retry,
    }
}

/// 受け側の座を取る。取れたらロックを握った [`Owner`] を返す（**bind はまだ**。
/// accept の直前まで遅らせる理由は呼び出し側にある）。負けたら**窓を作らずに**
/// 勝った方へ渡して終了する（層2）。座を取れない・取らない場合は `None` で、
/// そのまま従来どおり窓を開く。
///
/// [`Owner`]: md_preview::instance::Owner
///
/// **`-n` でも座は取りに行く。** `-n` が言っているのは「既存の窓へ送るな」であって
/// 「受けるな」ではない。ここで降りると、その日の 1 枚目が `md -n` だったときに座が
/// 空のまま残り、以降の `md` が全部新しい窓になる。
fn take_the_seat(
    flags: &cli::OpenFlags,
    stdin_mode: bool,
    targets: &[String],
    current_dir: &Option<PathBuf>,
) -> Option<md_preview::instance::Owner> {
    use md_preview::instance::{claim, Claim, Endpoint};

    if std::env::var_os(NO_IPC_ENV).is_some() {
        return None;
    }
    let ep = Endpoint::user_default()?;
    // `-n` は 2 枚目として開くのが目的なので、埋まっていたら黙って窓を作る。
    let forwarding = may_forward(flags);
    let deadline = std::time::Instant::now() + SEAT_WAIT;
    let mut msg = None;
    loop {
        match claim(&ep) {
            Claim::Owner(owner) => return Some(owner),
            Claim::Taken if !forwarding => return None,
            Claim::Taken => {}
            Claim::Failed(_) => return None,
        }
        // 所有者は確実に居る（flock を握っている）。まだ bind していないだけ
        // かもしれないので、層1 と違ってここは待つ。
        let msg = match &msg {
            Some(m) => m,
            None => msg.insert(message_to_forward(stdin_mode, targets, current_dir)?),
        };
        match deliver(&ep, msg) {
            Delivery::Done => std::process::exit(0),
            Delivery::GiveUp => return None,
            Delivery::Retry => {}
        }
        if std::time::Instant::now() >= deadline {
            // 所有者が起動に失敗したらしい。2 枚目として開く（何も出ないよりまし）。
            return None;
        }
        std::thread::sleep(SEAT_POLL);
        // ループの頭で claim をやり直す。所有者が bind の前に落ちると flock は
        // 空くので、ここで取り直さないと「座は空いているのに誰も座らない」まま
        // 全員が 2 枚目を開く状態が、窓を全部閉じるまで続く。
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
    let args: Vec<String> = std::env::args().collect();

    if run_terminal_command(&args) {
        return;
    }

    // 窓を開く経路だけフラグを剥がす。`theme` / `uninstall` / `--html` は上で処理
    // 済みなので、ここに現れるのはフラグとパスだけ。
    let (open_flags, targets) = match cli::split_open_flags(&args[1..]) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{}", msg);
            eprintln!("{}", cli::USAGE);
            std::process::exit(2);
        }
    };

    let stdin_mode = targets.is_empty() && !std::io::stdin().is_terminal();

    // ファイルは何個でも受ける（2 つ以上ならタブとして並べて開く）。
    if !stdin_mode && targets.is_empty() {
        eprintln!("{}", cli::USAGE);
        std::process::exit(1);
    }

    let current_dir = std::env::current_dir().ok().and_then(|d| d.canonicalize().ok());

    // 標準入力は一度しか読めない。転送に回すのか、子へ渡すのか、前景で開くのかを
    // 決める前に実体化して、環境変数で全経路に持ち回る（env なら exec も spawn も
    // そのまま越える）。ここで読まずに各経路が読むと、転送が空振りしたときの
    // 2 回目が空のファイルになる。
    //
    // まだスレッドを 1 つも立てていないので set_var は安全。
    if stdin_mode && std::env::var_os(app_config::STDIN_FILE_ENV).is_none() {
        std::env::set_var(app_config::STDIN_FILE_ENV, app_config::spool_stdin());
    }

    // ── 転送（#31 の層1）───────────────────────────────────────
    // バンドルへの乗り換え（exec）と自己デタッチ（spawn）より前に置く。転送で済む
    // ときはプロセスを増やさずに数ミリ秒で返せるし、まだ stderr を持っているので
    // エラーが人に見える。
    //
    // **ここは最適化で、正しさを担うのは下の claim の方。** 冷スタートが 2 本同時だと
    // ここは両方とも「誰も居ない」と読む。
    if forward_to_running_instance(&open_flags, stdin_mode, &targets, &current_dir) {
        return;
    }

    // macOS で日本語入力の変換候補パネルを出すため、最小のバンドルへ乗り換える
    // （成功するとここから戻らない）。ウィンドウを開かない経路を通したくないので
    // run_terminal_command と引数チェックの後、自己デタッチの判定より前に置く。
    // ここより後ろだと、乗り換え後の current_exe() を detach_self が使えない。
    md_preview::bundle::relaunch_in_flat_bundle();

    // ここから先はウィンドウを開く経路。前景で待たせると、待ち時間に上限のある
    // 呼び出し元（エージェントのコマンド実行ツールなど）が上限に達したときに
    // プロセスグループごと畳み、ウィンドウまで消えてしまう。人が叩いたときも「開いて
    // 終わり」でプロンプトが返る方が自然なので、stdout が端末かどうかで挙動を分けない。
    if std::env::var_os(DETACHED_ENV).is_none()
        && std::env::var_os(NO_DETACH_ENV).is_none()
        && detach_self(stdin_mode, &args[1..], &current_dir, &targets)
    {
        return;
    }

    // ── 受け側の座を取る（#31 の層2）────────────────────────────
    // ここで負けたら、窓を作らずに勝った方へ渡して終わる。層1 と違ってこちらは
    // 「所有者が確実に居る」状態なので、まだ bind していないだけなら待つ。
    let seat = take_the_seat(&open_flags, stdin_mode, &targets, &current_dir);

    let custom_css = md_preview::user_style_css();
    let (theme_paint, appearance, active_theme) = theme::resolve(&theme::read_active_name());
    let theme_css = theme::style_layer(appearance, &theme_paint);

    let config = if stdin_mode {
        AppConfig::from_stdin(&theme_css, &custom_css, &current_dir)
    } else {
        AppConfig::from_paths(&targets, &theme_css, &custom_css, &current_dir)
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
    // 戻し先を持たない OS でも同じ形で持ち回れるようにする。閉じる処理を
    // `finish_and_exit` に切ったので、ここが cfg で消えると呼び出し側まで cfg が要る。
    #[cfg(not(target_os = "macos"))]
    let launcher_pid: Option<i32> = None;

    let event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    // 全画面まわりの通知と、閉じる待ちの期限を自分へ戻すぶん。`proxy` は下で
    // ipc_handler へムーブされるので、控えをここで取っておく。
    let fullscreen_proxy = event_loop.create_proxy();

    // 転送の受け口を開ける。accept ループは別スレッドで、届いたものは
    // EventLoopProxy 経由でメインスレッドへ渡す（ファイル監視と同じ形）。
    //
    // bind は accept の直前でやる。座取り（flock）と同時に bind してしまうと、
    // そこからここまでの間（テーマ解決・ツリー走査・窓と webview の作成）に来た
    // 接続がバックログに溜まったまま挨拶を返せない。送り側はそれを「生きているが
    // 詰まっている」と読んで前面化を諦めるし、その間に所有者が落ちると
    // （`plan_paths` の exit(1) や窓作成の失敗）**転送が黙って消える**。
    let seat = seat.and_then(|owner| match owner.listen() {
        Ok(listening) => {
            let proxy = proxy.clone();
            Some(listening.serve(move |msg| {
                let _ = proxy.send_event(AppEvent::Open(msg));
            }))
        }
        // bind できないなら単一インスタンス化を諦めるだけ。窓は普通に開く。
        Err(e) => {
            eprintln!("md: 受け口を開けませんでした（単一インスタンス化なしで続けます）: {}", e);
            None
        }
    });

    let watcher = spawn_watcher(root_dir.clone(), proxy.clone());
    // root の外のファイルを開いたときに、そのファイルを監視へ足すため IPC から触る。
    // 監視は root の再帰監視だけなので、これが無いと root 外はホットリロードが効かない。
    let watcher = std::sync::Arc::new(std::sync::Mutex::new(watcher));
    let ipc_watcher = watcher.clone();

    // 窓は中身を待たずに出す。webview が最初のフレームを描くまで実測で 160ms 前後
    // かかるので、そこまで隠すと「打ってから窓が出るまで」がそのぶん丸ごと伸びる。
    // 代わりに下地をテーマの背景色で塗り、白い板が一瞬見えるのを防ぐ。
    //
    // None は「OS の外観が読めなかった」。塗らずに既定の背景色へ任せる方が、
    // 当てずっぽうで塗って外すより見え方が悪くない（theme::window_bg を参照）。
    //
    // Why not: None のときは下の透過も効かない。色を渡さないと wry の is_some() が
    // 偽になり drawsBackground が既定の true のまま残るので、この経路だけは
    // 「OS 既定の窓色 → 白 → ページ」の二段のちらつきが残る。塗る色が無い以上
    // 透かしても白の代わりが無いので、直しようが無い方を選んでいる。
    let bg = window_bg_rgba(active_theme, platform::os_is_dark());

    let mut window_builder = WindowBuilder::new()
        .with_title(&title)
        .with_inner_size(LogicalSize::new(app_config::WINDOW_WIDTH, app_config::WINDOW_HEIGHT));
    if let Some(color) = bg {
        window_builder = window_builder.with_background_color(color);
    }
    let window = window_builder.build(&event_loop).expect("Failed to create window");
    apply_window_appearance(&window, appearance);

    // 窓と同じ色の二重指定に見えるが、引き金になっているのは色の中身ではなく
    // 「色を渡したこと」の方である。wry の transparent feature は is_some() だけを
    // 見て drawsBackground=false を立てる。これを消すと WKWebView が既定どおり
    // 白を敷き、webview が乗ってから最初のフレームが届くまで（打鍵から数えて
    // 149ms → 259ms の 110ms）が白に戻る。上の 160ms は窓が出た 95ms から数えた
    // 長さなので、こちらより一回り長い。
    //
    // 色の中身が使われるのは underPageBackgroundColor（行き過ぎスクロールの
    // 跳ね返り、macOS 12 以降）だけ。透けた先に見えるのは窓側に塗った色なので、
    // ここを別の色にしても隙間の見え方は変わらない。
    let mut webview_builder = WebViewBuilder::new();
    if let Some(color) = bg {
        webview_builder = webview_builder.with_background_color(color);
    }
    let webview = webview_builder
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
            // Why not: 悪意ある html への対策にはならない（同一オリジンなのでシェルを
            // 読み込んだフレームを自前で作ればトップと区別が付かない）。事故を減らす衛生。
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
                        handle_menu(verb, payload);
                    } else if let Some(id) = body.strip_prefix("watch:") {
                        // 識別子は `id_to_path` を通す。ここだけ素通しにすると
                        // 「識別子とファイルの唯一の関門」が嘘になり、次に触る者が
                        // その嘘を根拠に検証を省く。
                        if let Some(path) = request::id_to_path(id) {
                            watch_extra(&ipc_watcher, &path);
                        }
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
    {
        platform::setup_menu();
        platform::set_dock_icon();
    }

    // 転送（#31）はページの準備を待たない。ソケットは窓より先に受けられるし、窓が
    // 出てからページが MdOpenFiles を定義するまでにも間がある。その間の
    // evaluate_script は黙って落ちるので、`Ready` が来るまで溜めておく。
    let mut page_ready = false;
    let mut pending_opens: Vec<String> = Vec::new();

    // 掃除を約束した一時ディレクトリ。自分の stdin と、転送で所有権を引き取った
    // ぶんが混ざって溜まる。**プロセスが終わるときに消すもの**で、タブを閉じても
    // 消さない（1 回あたり数 KB で、置き場所は $TMPDIR）。
    let mut owned_dirs: Vec<PathBuf> = stdin_dir.into_iter().collect();

    // 閉じる処理の進み具合（#59）。全画面のときだけ「抜け終わるのを待つ」状態を挟む。
    let mut closing = Closing::No;

    // 全画面から抜け終わった合図の購読。**プロセスと寿命を揃える**（`EventLoop::run` は
    // 戻らないので、ここに置いたまま最後まで生きる）。
    let _fullscreen_watch = platform::watch_exit_fullscreen(&window, {
        let proxy = fullscreen_proxy.clone();
        move || {
            let _ = proxy.send_event(AppEvent::ExitedFullscreen);
        }
    });

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            }
            | Event::UserEvent(AppEvent::Close) => {
                // Why not 「抜けている最中」も待つ: styleMask は抜け始めで false に
                // 落ちるので、⌃⌘F で抜けるアニメーション中（約 1 秒）に閉じると、ここは
                // 「全画面ではない」と読んで即終了する＝ #59 の症状がそのまま出る。
                // 塞ぐには `NSWindowWillExitFullScreenNotification` をもう 1 本購読して
                // 「遷移中」を自前で持つことになるが、**わざわざその 1 秒に ⌘W を押した
                // 場合だけ**で、しかも直す前と同じ着地にしかならない。割に合わないと見た。
                match close_step(platform::is_window_fullscreen(&window), &closing) {
                    CloseStep::ExitFullscreen => {
                        // 座はここで手放す。閉じると決めた窓が受け口を持ったままだと、
                        // 抜けるのを待っている 1 秒ほどの間に届いた転送が、タブを足した
                        // 直後にプロセスごと消える（＝叩いたのに何も出ない）。先に
                        // ソケットを消せば、後から来た md は繋がらず自分で窓を開く。
                        // unlink は冪等なので、終了時にもう一度撃っても構わない。
                        if let Some(handle) = &seat {
                            handle.unlink();
                        }
                        // 抜けるのは tao 経由で頼む。`toggleFullScreen:` を直接叩くより
                        // 安全で、遷移の最中に呼ばれたぶんは tao が積み直してくれる。
                        window.set_fullscreen(None);
                        closing = Closing::ExitingFullscreen;
                        // 期限は別スレッドから送る。`ControlFlow::WaitUntil` は使えない
                        // ——このクロージャは 1 周回に何度も呼ばれ、頭の `Wait` が
                        // 周回の最後に必ず上書きするので、タイマーが張られない（実測）。
                        // 監視と IPC が使っている「スレッド → proxy」に揃える。
                        let proxy = fullscreen_proxy.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(FULLSCREEN_EXIT_WAIT);
                            let _ = proxy.send_event(AppEvent::CloseDeadline);
                        });
                    }
                    CloseStep::Finish => {
                        finish_and_exit(
                            &mut closing,
                            &mut owned_dirs,
                            &seat,
                            launcher_pid,
                            control_flow,
                        );
                    }
                    // 待っている最中に来た要求（⌘W 連打、待ち中の赤ボタンや ⌘Q）と、
                    // 終了処理の後に届いたぶんは捨てる。
                    CloseStep::KeepWaiting | CloseStep::Ignore => {}
                }
            }
            // 全画面から抜け終わった。
            Event::UserEvent(AppEvent::ExitedFullscreen) => {
                if closing == Closing::ExitingFullscreen {
                    finish_and_exit(&mut closing, &mut owned_dirs, &seat, launcher_pid, control_flow);
                }
            }
            // 抜け終わりの通知が来ないまま期限が過ぎた（#59）。閉じられない窓を残すより
            // 諦めて閉じる。着地は #59 を直す前と同じ（デスクトップ Space）で、悪化はしない。
            Event::UserEvent(AppEvent::CloseDeadline) => {
                if closing == Closing::ExitingFullscreen {
                    finish_and_exit(&mut closing, &mut owned_dirs, &seat, launcher_pid, control_flow);
                }
            }
            Event::WindowEvent {
                event: WindowEvent::ThemeChanged(os_theme),
                ..
            } => {
                // 下地は窓を作るときに一度塗るだけなので、OS の外観が変わると
                // 取り残される。描き終わったページの背景が覆っている間は見えないが、
                // リサイズで新しく広がった領域と、行き過ぎスクロールの跳ね返りに
                // 古い色が出る。
                // 外観を固定したテーマなら同じ色が返るので実質なにも起きない。
                if let Some(color) = window_bg_rgba(active_theme, Some(os_theme == OsTheme::Dark)) {
                    window.set_background_color(Some(color));
                    let _ = webview.set_background_color(color);
                }
            }
            Event::UserEvent(AppEvent::Reload(id)) => {
                let script = format!("window.MdReload && window.MdReload({});", json_string(&id));
                let _ = webview.evaluate_script(&script);
            }
            Event::UserEvent(AppEvent::Ready) => {
                page_ready = true;
                let script = md_preview::html::open_files_script(&pending_opens);
                pending_opens.clear();
                if !script.is_empty() {
                    let _ = webview.evaluate_script(&script);
                }
            }
            Event::UserEvent(AppEvent::Open(msg)) => {
                // 掃除の約束はワイヤから来た値を信用せず、送り側と同じ門に通してから
                // 引き取る（信用した時点で `own=/etc` が通る道ができる）。
                // 載っているのは実体化したファイルのパスで、門が消してよい親を返す。
                for doc in &msg.own {
                    if let Some(dir) = app_config::owned_stdin_dir(Path::new(doc)) {
                        if !owned_dirs.contains(&dir) {
                            owned_dirs.push(dir);
                        }
                    }
                }
                // アプリを前面に出すのは送り側の仕事（`platform::activate_other`）だが、
                // 最小化された窓・隠れた窓を持ち上げられるのは自分だけ。両方要る。
                window.set_minimized(false);
                window.set_visible(true);
                window.set_focus();
                // ここでは形（先頭が `/`）しか見ない。実体への解決は下流の `?file=`
                // （`request::id_to_path`）が唯一の関門で、解決できなければ 404 →
                // showLoadError が画面に理由を出す。
                //
                // Why not ここで id_to_path を通す: 落とすと「窓は前に出たのに何も
                // 起きない」になる。`watch:` が通すのは監視という副作用を伴うからで、
                // 「ページへ文字列を渡すだけ」のここに同じ門は要らない。
                let ids: Vec<String> =
                    msg.files.into_iter().filter(|id| id.starts_with('/')).collect();
                if ids.is_empty() {
                    return;
                }
                if !page_ready {
                    pending_opens.extend(ids);
                    return;
                }
                let _ = webview.evaluate_script(&md_preview::html::open_files_script(&ids));
            }
            _ => {}
        }
    });
}

/// 掃除を約束した一時ディレクトリを消す。**プロセスが終わるときに呼ぶもの**で、
/// 呼ぶのは [`finish_and_exit`] だけ。
fn drop_owned(dirs: &mut Vec<PathBuf>) {
    for dir in dirs.drain(..) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// 後始末をして終了する。**プロセスが終わる唯一の経路。**
///
/// 4 つの手順をここ 1 箇所に集める。閉じる要求は #59 で「全画面なら先に抜ける」という
/// 待ちを挟むようになり、終了に至る入口が 2 つ（要求を受けた所と、抜け終わった所）に
/// 増えたため。#49（窓を閉じてもプロセスを生かす）が入ったら、隠す経路はこれを呼ばず、
/// 本当の終了だけがここへ来る。
fn finish_and_exit(
    closing: &mut Closing,
    owned_dirs: &mut Vec<PathBuf>,
    seat: &Option<md_preview::instance::Handle>,
    launcher_pid: Option<i32>,
    control_flow: &mut ControlFlow,
) {
    // 「終わった」を立てるのはここ 1 箇所。入口が 3 つ（要求・通知・期限）あるので、
    // 呼び出し側の約束にすると 1 つ忘れただけで後始末が二度走る。
    *closing = Closing::Done;
    // stdin を実体化した一時ファイルはプロセスと寿命を揃える（表示中はドキュメント
    // そのものなので、読んだ直後には消せない）。メニューの ⌘Q は `performClose:` なので
    // ここを通るが、Dock からの Quit（terminate）とクラッシュは通らない。置き場所が
    // $TMPDIR なので取り残しは OS の掃除に任せる。
    drop_owned(owned_dirs);
    // ソケットファイルの後始末は衛生であって、正しさの要件ではない。次の起動が
    // listen() で無条件に unlink → bind し直すので、terminate やクラッシュで
    // 取り残しても壊れない。
    if let Some(handle) = seat {
        handle.unlink();
    }
    #[cfg(target_os = "macos")]
    if let Some(pid) = launcher_pid {
        platform::activate_pid(pid);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = launcher_pid;
    *control_flow = ControlFlow::Exit;
}

/// 全画面から抜け終わるのを待つ上限。超えたら諦めて閉じる。
///
/// 諦めた先は #59 を直す前と同じ着地（デスクトップ Space へ落ちる）で、悪化はしない。
/// 上限を置くのは、抜けられない状況——AppKit が `windowDidFailToEnterFullScreen:` の
/// 側へ倒れた、遷移が終わらない——で閉じられない窓を作らないため。
///
/// 片道のアニメーションは実測 0.6〜1 秒だが、**入っている途中に閉じると往復ぶん要る**。
/// tao は遷移中の `set_fullscreen` を積んでおいて入り終わってから流すので
/// （0.35 `macos/window.rs` の `target_fullscreen`）、「入る → 抜ける」の 2 回が直列になる。
/// そこで閉じられないと #59 が直っていないのと同じなので、往復に足りる幅を取る。
const FULLSCREEN_EXIT_WAIT: Duration = Duration::from_millis(2500);


/// 閉じる処理がどこまで進んでいるか。
///
/// 全画面の窓をそのまま終了させると、macOS からは強制終了と同じ形に見えて、閉じた後に
/// 隣のデスクトップ Space が出てくる（#59）。先に全画面から抜けて元の Space へ戻してから
/// 閉じると、出てくるのは起動元の端末が居る Space になる。
/// 閉じる処理がどこまで進んでいるか。
///
/// 全画面の窓をそのまま終了させると、macOS からは強制終了と同じ形に見えて、閉じた後に
/// 隣のデスクトップ Space が出てくる（#59）。先に全画面から抜けて元の Space へ戻してから
/// 閉じると、出てくるのは起動元の端末が居る Space になる。
#[derive(Debug, PartialEq, Eq)]
enum Closing {
    No,
    /// 全画面から抜けるよう頼んで、抜け終わるのを待っている。
    ExitingFullscreen,
    /// 終了処理は済んだ。`ControlFlow::Exit` を立てた後も、そのイテレーションぶんの
    /// イベントは届き続けるので、二度と後始末を走らせないための状態。
    Done,
}

/// 閉じる要求を受けたとき、いま何をすべきか。
#[derive(Debug, PartialEq, Eq)]
enum CloseStep {
    /// 全画面から抜けるよう頼んで、待ちに入る。
    ExitFullscreen,
    /// 待っている最中の要求。捨てる。
    KeepWaiting,
    /// 後始末をして終了する。
    Finish,
    /// もう終わっている。何もしない。
    Ignore,
}

/// [`CloseStep`] を決める。窓もイベントループも要らないので、ここだけテストできる。
///
/// 待ちを終わらせるのは抜け終わりの通知（[`AppEvent::ExitedFullscreen`]）か期限
/// （[`AppEvent::CloseDeadline`]）で、どちらもイベントとして届く。だからこの関数は
/// 時刻を持たない。
fn close_step(fullscreen: bool, closing: &Closing) -> CloseStep {
    match closing {
        Closing::Done => CloseStep::Ignore,
        Closing::ExitingFullscreen => CloseStep::KeepWaiting,
        Closing::No if fullscreen => CloseStep::ExitFullscreen,
        Closing::No => CloseStep::Finish,
    }
}

#[cfg(test)]
mod close_tests {
    use super::*;

    #[test]
    fn a_plain_window_closes_immediately() {
        assert_eq!(close_step(false, &Closing::No), CloseStep::Finish);
    }

    #[test]
    fn a_fullscreen_window_exits_fullscreen_first() {
        assert_eq!(close_step(true, &Closing::No), CloseStep::ExitFullscreen);
    }

    #[test]
    fn close_requests_during_the_wait_are_ignored() {
        // 待っている間は全画面かどうかを見ない。styleMask は抜け始めで false へ落ちるので、
        // 見てしまうと待ちが即終わる。
        assert_eq!(close_step(true, &Closing::ExitingFullscreen), CloseStep::KeepWaiting);
        assert_eq!(close_step(false, &Closing::ExitingFullscreen), CloseStep::KeepWaiting);
    }

    #[test]
    fn a_finished_close_ignores_later_requests() {
        assert_eq!(close_step(true, &Closing::Done), CloseStep::Ignore);
        assert_eq!(close_step(false, &Closing::Done), CloseStep::Ignore);
    }
}

/// 窓の外観（タイトルバーと信号ボタン）をテーマに合わせる。
///
/// 外観を固定したテーマだけ指定する。OS 追従のテーマに指定してしまうと、OS の設定を
/// 変えても窓だけ古い外観に取り残される。
/// OS の外観が変わっても呼び直さなくてよい。指定しなかった窓（OS 追従テーマ）は
/// macOS が勝手に追随し、指定した窓（固定テーマ）は追随しないのが正しい姿だから。
fn apply_window_appearance(window: &tao::window::Window, appearance: theme::Appearance) {
    match appearance {
        theme::Appearance::Dark => platform::set_window_appearance(window, true),
        theme::Appearance::Light => platform::set_window_appearance(window, false),
        theme::Appearance::Auto => {}
    }
}

/// 窓と webview に渡す下地色。色を決められないときは None（塗らない）。
fn window_bg_rgba(
    active_theme: Option<&theme::Theme>,
    os_dark: Option<bool>,
) -> Option<(u8, u8, u8, u8)> {
    theme::window_bg(active_theme, os_dark).map(|[r, g, b]| (r, g, b, 255))
}

/// 右クリックメニュー由来の IPC（`menu:<verb>:<payload>`）を処理する。
/// ここに来るのは絶対パスコピー / Finder表示 / 既定アプリで開く の 3 つ——
/// いずれも payload が「パス」なので、`resolve_target` で解決してから触る。
/// 任意テキストのクリップボード書き込みはパス解決を通さない別の口（`copy:`）。
fn handle_menu(verb: &str, payload: &str) {
    let Some(path) = resolve_target(payload) else { return };
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

/// メニュー操作対象の絶対パスを解決する。`id` は `?file=` と同じ識別子（絶対パス）。
/// プレビューに何も開いていないときは空で来るので、その場合は何もしない。
fn resolve_target(id: &str) -> Option<PathBuf> {
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    request::id_to_path(id)
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
/// ページに CSP は無く、本文の script もこの IPC を叩けるので、ここが実行系に対する
/// 唯一のガードになる。
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
            // JS 側が持っている識別子（絶対パス）と同じ形で通知する。形がズレると
            // 「開いているファイルが変わったか」の照合が外れて再読込しない。
            let id = request::file_id(&path);
            let _ = proxy.send_event(AppEvent::Reload(id));
        }
    }).ok()?;

    // root 配下は再帰で見る。root の外のファイルはページから watch: が飛んでくるので
    // watch_extra が個別に足す。
    //
    // root を `/` にできない理由の 1 つがここ。再帰監視はボリューム全体の FSEvents を
    // 受けることになり、走査に予算を付けても減らせない（`app_config::files_root` の門）。
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
