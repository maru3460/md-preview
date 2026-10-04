//! 起動時の入力（stdin / フォルダ / ファイル）から、ウィンドウを開くのに必要な
//! 設定一式を組み立てる。
//!
//! 入り口はどれもフォルダモード 1 本に落ちる。stdin だけは実体のファイルが無いので、
//! 一時ファイルへ書き出してから「root の外にあるファイル」として開く。
//! ファイルをどう描画するかは [`crate::request::render_file`] に委ねる
//! （配信経路と描画を必ず一致させるため）。

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::html::{build_folder_html, json_string};
use crate::request::{self, file_id};

/// 標準入力から読んだ内容を、自己デタッチした子へ渡すための一時ファイルのパス。
/// 子は標準入力を持たないので、親が読んでファイル経由で渡す。
pub const STDIN_FILE_ENV: &str = "MD_STDIN_FILE";

/// stdin を実体化する一時ディレクトリの名前の頭。後片付けしてよい場所かの判定にも使う。
const STDIN_DIR_PREFIX: &str = "md-stdin-";

/// ウィンドウの幅。ファイルツリー(250px) ＋ 本文 ＋ 見出しナビ(TOC) が収まる。
pub const WINDOW_WIDTH: f64 = 1280.0;
/// ウィンドウの高さ。
pub const WINDOW_HEIGHT: f64 = 700.0;

/// 初期サイズの下限。ツリーとタブバーが潰れずに出る大きさ。
const MIN_WINDOW_WIDTH: u32 = 480;
const MIN_WINDOW_HEIGHT: u32 = 320;

/// これを超える値は書き損じとして読まない（`14400x900` のような桁の打ち間違い）。
/// 画面より大きいだけの値は、窓を作るときに画面へ収める（`main.rs`）。
const MAX_WINDOW_SIDE: u32 = 16384;

/// `settings` の `window-size=`（#42）の値を幅と高さ（論理ポイント）へ。
///
/// 書いていない・読めないなら既定の [`WINDOW_WIDTH`] × [`WINDOW_HEIGHT`]。
/// 小さすぎる値は下限まで持ち上げる。
///
/// Why not 小さすぎる値も書き損じとして既定へ戻す: 手で `400x300` と書いた人は
/// 「小さめにしたい」という意図を持っている。下限で受ける方がその意図に近い。
pub fn window_size(value: Option<&str>) -> (f64, f64) {
    match value.and_then(parse_window_size) {
        Some((w, h)) => {
            let (w, h) = raise_to_min(w, h);
            (w as f64, h as f64)
        }
        None => (WINDOW_WIDTH, WINDOW_HEIGHT),
    }
}

/// 下限まで持ち上げる。読むとき（[`window_size`]）と、設定タブから保存するときの
/// 両方が通る——保存する側が通さないと、欄には `100` と出ているのに次の窓は 480 で
/// 開く、という食い違いになる。
pub fn raise_to_min(w: u32, h: u32) -> (u32, u32) {
    (w.max(MIN_WINDOW_WIDTH), h.max(MIN_WINDOW_HEIGHT))
}

/// `1440x900` → `(1440, 900)`。区切りは `x` だけを受ける（`×` は手で打ちにくい）。
pub fn parse_window_size(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.trim().split_once('x')?;
    let w: u32 = w.trim().parse().ok()?;
    let h: u32 = h.trim().parse().ok()?;
    if w == 0 || h == 0 || w > MAX_WINDOW_SIDE || h > MAX_WINDOW_SIDE {
        return None;
    }
    Some((w, h))
}

/// 保存する綴り。[`parse_window_size`] と対。
pub fn format_window_size(w: u32, h: u32) -> String {
    format!("{w}x{h}")
}

/// ウィンドウ起動に必要な、入力から決まる設定一式。
pub struct AppConfig {
    pub title: String,
    pub html_bytes: Vec<u8>,
    pub root_dir: PathBuf,
    /// stdin を実体化した一時ディレクトリ。**プロセスが終わるときに消す**
    /// （表示中はドキュメントそのものなので、読んだ直後には消せない）。
    /// stdin 以外では None。
    ///
    /// これは「自分が掘ったぶん」だけ。転送（#31）で他のプロセスから所有権を
    /// 引き取ったぶんと合流して、`main.rs` の `owned_dirs` がまとめて面倒を見る。
    pub stdin_dir: Option<PathBuf>,
}

impl AppConfig {
    /// ページの JS から読む起動時グローバル。ウィンドウ表示では初期化スクリプト
    /// （WKUserScript）として注入される。これはページのスクリプトより先に走るので、
    /// 各モジュールは自分の初期化時点でこれらを読める。
    ///
    /// - `MD_APPEARANCE`     解決済みテーマの外観。JS で描く図（mermaid）を OS 設定では
    ///                       なくテーマに追従させる。
    /// - `MD_RENDERABLE_EXT` レンダリング対象の拡張子。定義元は `request::RENDERABLE_EXT`。
    /// - `MD_ROOT_DIR`       配信ルートの絶対パス。識別子（絶対パス）から画面に出す
    ///                       名前を作るのと、本文の URL（root 相対）を識別子へ戻すのに
    ///                       要る。`MdCommon.idToDisplay` / `urlToId` の基準。
    /// - `MD_STDIN_PREFIX`   パイプ入力を実体化する一時ディレクトリの名前の頭。
    ///                       定義元はこのモジュールの `STDIN_DIR_PREFIX`。タブが「同名なら親の名前を
    ///                       添える」規則を、パイプの置き場所には当てないために要る。
    /// - `MD_QUICK_ACCESS`   Quick Access の並び（#35）。`[{path, dir}]` で、並びが表示順。
    ///
    /// `quick` を引数で受けるのは、これが AppConfig（起動の入力）ではなく
    /// **ユーザーの持ち物**だから。`~/.config` をここで読むと、`examples/serve.rs` で
    /// 立てる UI テストが手元の登録内容に左右される。
    pub fn page_globals(
        &self,
        appearance: crate::theme::Appearance,
        quick: &[crate::quick_access::Entry],
    ) -> String {
        let renderable = request::RENDERABLE_EXT
            .iter()
            .map(|e| json_string(e))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "window.MD_APPEARANCE = {}; window.MD_RENDERABLE_EXT = [{}]; window.MD_ROOT_DIR = {}; window.MD_STDIN_PREFIX = {}; window.MD_QUICK_ACCESS = [{}];",
            json_string(appearance.as_str()),
            renderable,
            json_string(&self.root_dir.to_string_lossy()),
            json_string(STDIN_DIR_PREFIX),
            quick_json(quick),
        )
    }

    /// ツリー付きのページ。`initial_files` は起動時にタブとして開く識別子
    /// （絶対パス。先頭が最初に表示される）。
    fn folder(root: PathBuf, theme_css: &str, custom_css: &str, initial_files: &[String]) -> Self {
        let title = dir_name(&root);
        let html = build_folder_html(&title, theme_css, custom_css, initial_files);
        AppConfig {
            title,
            html_bytes: html.into_bytes(),
            root_dir: root,
            stdin_dir: None,
        }
    }

    /// パイプで渡された markdown を一時ファイルへ書き出し、root（＝作業ディレクトリ）の
    /// 外にあるファイルとして開く。ツリーには出ないがタブには出る。
    pub fn from_stdin(theme_css: &str, custom_css: &str, current_dir: &Option<PathBuf>) -> Self {
        Self::stdin_doc(materialize_stdin(), theme_css, custom_css, current_dir)
    }

    /// 実体化した stdin のファイルから設定を組み立てる。root は作業ディレクトリなので、
    /// ツリーとファイル検索は「いまいる場所」を見せられる（一時ファイルの置き場所を
    /// 見せても意味が無い）。stdin のファイルはその root の外なので、識別子は絶対パス。
    fn stdin_doc(
        doc: PathBuf,
        theme_css: &str,
        custom_css: &str,
        current_dir: &Option<PathBuf>,
    ) -> Self {
        let root = stdin_root(&doc, current_dir);
        let id = file_id(&doc);
        let mut config = Self::folder(root, theme_css, custom_css, &[id]);
        config.stdin_dir = owned_stdin_dir(&doc);
        config
    }

    /// `--notify` を受け取る md が居なかったときの窓（#36）。渡されたファイルは
    /// タブで開いた状態で始まり、**通知には積まない**（もう見えているので）。
    ///
    /// root は [`files_root`] を通さず**常に作業ディレクトリ**にする。別のツリーの
    /// ファイルを 2 つ渡されると共通の親が `/` まで広がり、ボリューム全体の再帰監視と
    /// ⌘P の予算切れを踏むため。エージェントが並べたパスは cwd の外に出やすい。
    ///
    /// ⚠️ issue #36 はこれを「共通の親が `/` まで広がると**起動を拒否される**から」と
    /// 書いているが、その門はもう無い（[`files_root`] の Why not）。理由は入れ替わって
    /// いて、規則だけが残っている。
    pub fn from_notify(
        args: &[String],
        theme_css: &str,
        custom_css: &str,
        current_dir: &Option<PathBuf>,
    ) -> Self {
        let ids = notify_ids(args);
        Self::folder(notify_root(&ids, current_dir), theme_css, custom_css, &ids)
    }

    /// 引数で渡されたパス（1 つ以上）から設定を組み立てる。
    pub fn from_paths(
        args: &[String],
        theme_css: &str,
        custom_css: &str,
        current_dir: &Option<PathBuf>,
    ) -> Self {
        let (root, ids) = plan_paths(args, current_dir);
        Self::folder(root, theme_css, custom_css, &ids)
    }
}

/// `MD_QUICK_ACCESS` に載せる形。**並びが表示順**なので、受け取った順のまま出す。
/// `dir` を持たせるのは、消えたパスでも行を描けるようにするため（#35）。
fn quick_json(quick: &[crate::quick_access::Entry]) -> String {
    quick
        .iter()
        .map(|e| format!("{{\"path\":{},\"dir\":{}}}", json_string(&e.path), e.is_dir))
        .collect::<Vec<_>>()
        .join(",")
}

/// ウィンドウのタイトルに使うディレクトリ名。
///
/// `/` には `file_name` が無い。そこで `.` に落とすと、root を `/` まで上げた窓の
/// タイトルが「.」になってどこを見ているか分からなくなるので、パスそのものを名前にする。
pub fn dir_name(p: &Path) -> String {
    p.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| p.to_string_lossy().into_owned())
}

/// 引数のパス群から「root」と「起動時にタブとして開く識別子」を決める。
///
/// **デタッチする親と本体の両方から呼ぶこと。** 子プロセスは標準エラー出力を
/// 持たないので、ここで落ちる条件を親で通しておかないと「ウィンドウも出ず、
/// エラーも出ず、終了コード 0」という無反応になる。
pub fn plan_paths(args: &[String], current_dir: &Option<PathBuf>) -> (PathBuf, Vec<String>) {
    // フォルダ指定だけは root がそのまま決まる（初期表示するファイルは無い）。
    if let [only] = args {
        let path = resolve_arg_path(only);
        if path.is_dir() {
            return (path, Vec::new());
        }
    }

    let paths = resolve_file_args(args, "複数指定できるのはファイルだけです");
    let root = files_root(&paths, current_dir);
    let ids = paths.iter().map(|p| file_id(p)).collect();
    (root, ids)
}

/// `--notify` が積むファイルたちの識別子（#36）。
///
/// [`plan_paths`] に任せられないのは、あちらがフォルダ単発を「ツリーの頂点」として
/// 受けるため（`md <dir>` はそれで正しい）。**通知に積む対象はファイルだけ**なので、
/// 数に関係なくフォルダを弾く。開けないパスは `resolve_arg_path` がそこで終わらせる。
///
/// **stderr を持っている経路から呼ぶこと。** デタッチした子から呼ぶと、フォルダを
/// 渡されたときに「窓も出ずエラーも出ず終了コード 1」になる。
pub fn notify_ids(args: &[String]) -> Vec<String> {
    resolve_file_args(args, "--notify に積めるのはファイルだけです")
        .iter()
        .map(|p| file_id(p))
        .collect()
}

/// ファイル指定の引数を絶対パスへ解決する。重複は 1 つにまとめる。
///
/// `dir_refusal` を呼び出し側から受けるのは、フォルダを弾く理由が経路ごとに違うため
/// （複数指定は「2 つのツリーを同時に出せない」、`--notify` は「積む対象ではない」）。
fn resolve_file_args(args: &[String], dir_refusal: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for arg in args {
        let path = resolve_arg_path(arg);
        // フォルダは root を決める側なので、ファイルと混ぜられない
        // （2 つのツリーを同時に出す作りになっていない）。
        if path.is_dir() {
            eprintln!("md: {}（'{}' はフォルダ）", dir_refusal, arg);
            std::process::exit(1);
        }
        // タブの識別子はパスなので、同じファイルを 2 回渡されても 1 枚にまとめる。
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// `--notify` が冷スタートしたときの root（#36）。**常に作業ディレクトリ。**
///
/// cwd が取れないとき（消えている等）だけ、先頭のファイルの親へ逃がす。`.` へ
/// 落とさないのは、root がそのまま `?dir=` の識別子になるため——絶対パスでなければ
/// `request::id_to_path` が弾き、ツリーも初期タブも出ない真っ白になる
/// （[`stdin_root`] と同じ事情）。
fn notify_root(ids: &[String], current_dir: &Option<PathBuf>) -> PathBuf {
    current_dir
        .clone()
        .or_else(|| Path::new(ids.first()?).parent().map(Path::to_path_buf))
        // 絶対パスでない答えは捨てる。`notify_ids` が canonicalize を通すので
        // いまここへ相対パスは来ないが、`"a.md"` の親は**空文字**であって `None`
        // ではないので、来た日に「root が空の窓」が黙って開く。
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// タブに乗せるファイルたちを収める root を決める。
///
/// 全部が cwd 配下ならこれまでどおり cwd を root にし、そうでなければ指定された
/// ファイルたちの共通の親まで広げる（1 つだけなら、そのファイルの親ディレクトリ）。
fn files_root(paths: &[PathBuf], current_dir: &Option<PathBuf>) -> PathBuf {
    // Why not: root が `/` まで広がっても止めない。ここには「`/` は重いから開かせない」
    // という門があったが、**門として成立していなかった。** `plan_paths` がフォルダ単発を
    // 手前で返すので `md /` には一度も効かず、効いていたのはファイル指定の 2 経路だけ
    // だった——共通の親が `/` に広がる `md ~/a.md /tmp/b.md` と、cwd がそのまま
    // root になる `cd / && md a.md`。「フォルダなら通るのにファイルなら止まる」という
    // 一貫しない門だったので外した。
    //
    // 門が挙げていた重さ（`/` の再帰監視・⌘P の予算切れ）は**消えていない。**
    // 再帰監視は `main.rs` の `spawn_watcher` がいまも張るし、⌘P の
    // `FILE_LIST_MAX`（`request.rs`）もそのまま在る。`h` で天井を越えられるように
    // なったぶん、到達しやすくもなっている。それでも外したのは、門が守れていた範囲が
    // 上の 2 経路しか無く、`md /` を塞げていない以上「重さへの対策」として
    // 機能していなかったため（判断はユーザー）。
    current_dir
        .clone()
        .filter(|cwd| paths.iter().all(|p| p.starts_with(cwd)))
        .or_else(|| common_ancestor(paths))
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// 渡されたファイルを全部含む、いちばん深いディレクトリ。
/// 共通の祖先を持たない（別ボリュームなど）なら None。
fn common_ancestor(paths: &[PathBuf]) -> Option<PathBuf> {
    let mut acc = paths.first()?.parent()?.to_path_buf();
    for p in paths.iter().skip(1) {
        let parent = p.parent()?;
        // acc を親方向へ削っていき、この 1 つも収まる深さまで戻す。
        while !parent.starts_with(&acc) {
            if !acc.pop() {
                return None;
            }
        }
    }
    Some(acc)
}

/// 引数のパスを絶対パスへ解決する。開けないパスはここで終わる。
fn resolve_arg_path(arg: &str) -> PathBuf {
    Path::new(arg).canonicalize().unwrap_or_else(|e| {
        eprintln!("md: '{}' を開けませんでした: {}", arg, e);
        std::process::exit(1);
    })
}

/// stdin を開くときの root。作業ディレクトリをそのまま使う。
///
/// 作業ディレクトリが取れないとき（cwd が消えている等）に `.` へ落とさないのは、
/// root がそのまま `?dir=` の識別子になるため。識別子は絶対パスでなければ
/// `request::id_to_path` が弾き、ツリーも初期タブも出ない真っ白になる。
///
/// Why not: cwd が `/` のときに一時ファイルの場所へ逃がす枝があったが、外した。
/// `files_root` の門と同じ理由で書かれていたので、門と一緒に理由が消えている。
fn stdin_root(doc: &Path, current_dir: &Option<PathBuf>) -> PathBuf {
    current_dir
        .clone()
        .unwrap_or_else(|| doc.parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir))
}

/// プロセスが終わるときに消してよい一時ディレクトリ。
///
/// `STDIN_FILE_ENV` は環境変数なので、外から任意の場所を指せる。`doc` の親を
/// 無条件に消すと `MD_STDIN_FILE=/etc/hosts` で `/etc` が飛ぶので、
/// 自分が掘る形（`$TMPDIR/md-stdin-<pid>/`）に一致するものだけを対象にする。
///
/// 転送（#31）で受け側が所有権を引き取るときも**この同じ門を通す**。ワイヤから来た
/// 値を信用して消すと、同じ穴が env から socket へ移るだけになる。
/// 送り側も、門を通らないものは `own=` に載せない（消し損ねる方が誤削除より安い）。
pub fn owned_stdin_dir(doc: &Path) -> Option<PathBuf> {
    let dir = doc.parent()?;
    if !dir.file_name()?.to_str()?.starts_with(STDIN_DIR_PREFIX) {
        return None;
    }
    if dir.parent()? != canonical(std::env::temp_dir()) {
        return None;
    }
    Some(dir.to_path_buf())
}

/// stdin の markdown を実体のファイルにする。
///
/// 実体化は `main` の頭で 1 回だけ行い、`STDIN_FILE_ENV` に置いて全経路で持ち回る
/// （転送・exec・spawn・前景）。なので**ここへ来るときは環境変数が必ず立っている**。
/// `None` の枝は、この関数をライブラリとして単体で呼ぶ経路のための受け皿である。
fn materialize_stdin() -> PathBuf {
    match std::env::var_os(STDIN_FILE_ENV) {
        Some(p) => canonical(PathBuf::from(p)),
        None => spool_stdin(),
    }
}

/// 標準入力を読み、一時ファイルへ書き出してそのパスを返す。
/// 自己デタッチする親も、子へ渡す内容をここに置く。
pub fn spool_stdin() -> PathBuf {
    let mut markdown = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut markdown) {
        eprintln!("md: 標準入力を読み込めませんでした: {}", e);
        std::process::exit(1);
    }
    write_spool(&markdown)
}

/// 一時ファイルへ書き出す。プロセス専用のディレクトリを掘るのは、このファイルを
/// 「root の外のファイル」として監視へ足すとき、監視対象が $TMPDIR 全体ではなく
/// この 1 ファイルだけで済むため（後片付けの範囲も同じ理由で絞れる）。
fn write_spool(markdown: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("{}{}", STDIN_DIR_PREFIX, std::process::id()));
    let path = dir.join("stdin.md");
    if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, markdown)) {
        eprintln!("md: 標準入力を一時ファイルへ書き出せませんでした: {}", e);
        std::process::exit(1);
    }
    canonical(path)
}

/// 識別子と実パスを突き合わせられるよう正規化する。macOS の $TMPDIR は
/// `/var` → `/private/var` のシンボリックリンク越しに来るので、揃えないと
/// JS が持つ識別子と監視側が組む識別子がズレる。
fn canonical(p: PathBuf) -> PathBuf {
    p.canonicalize().unwrap_or(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_window_size_is_read_back_as_logical_points() {
        assert_eq!(window_size(Some("1440x900")), (1440.0, 900.0));
        assert_eq!(parse_window_size(&format_window_size(1512, 945)), Some((1512, 945)));
    }

    #[test]
    fn a_missing_or_unreadable_window_size_falls_back_to_the_default() {
        let default = (WINDOW_WIDTH, WINDOW_HEIGHT);
        assert_eq!(window_size(None), default);
        // 「既定に戻す」は空の値を書く。
        assert_eq!(window_size(Some("")), default);
        assert_eq!(window_size(Some("1440")), default);
        assert_eq!(window_size(Some("1440×900")), default);
        assert_eq!(window_size(Some("0x900")), default);
        assert_eq!(window_size(Some("-1x900")), default);
        // 桁の打ち間違いは画面へ収めるより、書き損じとして読まない方が害が小さい。
        assert_eq!(window_size(Some("144000x900")), default);
    }

    #[test]
    fn a_too_small_window_size_is_raised_to_the_minimum() {
        assert_eq!(
            window_size(Some("100x100")),
            (MIN_WINDOW_WIDTH as f64, MIN_WINDOW_HEIGHT as f64)
        );
    }

    /// 設定タブから保存するときも下限を通す。通さないと、欄には 100 と出ているのに
    /// 次の窓は 480 で開く。
    #[test]
    fn saving_raises_a_too_small_size_to_the_minimum_as_reading_does() {
        assert_eq!(raise_to_min(100, 1000), (MIN_WINDOW_WIDTH, 1000));
        let (w, h) = raise_to_min(100, 100);
        assert_eq!(window_size(Some(&format_window_size(w, h))), (w as f64, h as f64));
    }

    #[test]
    fn spaces_around_the_numbers_are_tolerated() {
        assert_eq!(window_size(Some(" 1440 x 900 ")), (1440.0, 900.0));
    }

    fn paths(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn common_ancestor_is_the_deepest_shared_dir() {
        // 複数ファイル指定の root を決める要。ここが浅すぎるとツリーが巨大になり、
        // 深すぎると root 外のファイルが開けなくなる。
        assert_eq!(
            common_ancestor(&paths(&["/a/b/x.md", "/a/b/y.md"])),
            Some(PathBuf::from("/a/b"))
        );
        assert_eq!(
            common_ancestor(&paths(&["/a/b/x.md", "/a/c/d/y.md"])),
            Some(PathBuf::from("/a"))
        );
        // 片方がもう片方の祖先にあるときは、浅い方まで戻る。
        assert_eq!(
            common_ancestor(&paths(&["/a/b/c/x.md", "/a/y.md"])),
            Some(PathBuf::from("/a"))
        );
        // 共通が root しか無いなら root。
        assert_eq!(
            common_ancestor(&paths(&["/a/x.md", "/b/y.md"])),
            Some(PathBuf::from("/"))
        );
        // 1 つだけならその親ディレクトリ（cwd の外のファイルを開くときの root）。
        assert_eq!(
            common_ancestor(&paths(&["/a/b/x.md"])),
            Some(PathBuf::from("/a/b"))
        );
    }

    #[test]
    fn common_ancestor_of_nothing_is_none() {
        assert_eq!(common_ancestor(&[]), None);
    }

    #[test]
    fn files_root_prefers_cwd_and_falls_back_to_the_shared_parent() {
        let cwd = Some(PathBuf::from("/work"));
        // cwd 配下に収まるなら cwd が root（ツリーに作業ディレクトリを出す）。
        assert_eq!(files_root(&paths(&["/work/docs/a.md"]), &cwd), PathBuf::from("/work"));
        // cwd の外なら、そのファイルの親ディレクトリまで root を寄せる。
        assert_eq!(files_root(&paths(&["/other/a.md"]), &cwd), PathBuf::from("/other"));
        // 片方でも外に出ていたら共通の親へ。
        assert_eq!(
            files_root(&paths(&["/work/docs/a.md", "/work/lib/b.md"]), &None),
            PathBuf::from("/work")
        );
        // 別のボリューム同士など、共通の親が `/` まで広がっても開ける。
        assert_eq!(
            files_root(&paths(&["/work/a.md", "/tmp/b.md"]), &None),
            PathBuf::from("/")
        );
    }

    #[test]
    fn notify_root_is_the_working_dir_even_when_the_files_are_elsewhere() {
        let cwd = Some(PathBuf::from("/work"));
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // cwd 配下でも外でも cwd。`files_root` と違って共通の親へ広げない——
        // 別のツリーの 2 本を渡されたときに `/` を丸ごと監視することになる。
        assert_eq!(notify_root(&ids(&["/work/a.md"]), &cwd), PathBuf::from("/work"));
        assert_eq!(notify_root(&ids(&["/other/a.md", "/tmp/b.md"]), &cwd), PathBuf::from("/work"));
        // cwd が取れないときだけ、先頭のファイルの親へ逃がす。
        assert_eq!(notify_root(&ids(&["/other/a.md"]), &None), PathBuf::from("/other"));
    }

    #[test]
    fn notify_root_is_always_absolute() {
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // root はそのまま `?dir=` の識別子になる。`.` や空を返すと `id_to_path` が
        // 弾いて真っ白な窓になるので、逃げ道の先まで絶対パスであること。
        for (given, cwd) in [
            (ids(&["/a/b.md"]), Some(PathBuf::from("/work"))),
            (ids(&["/a/b.md"]), None),
            (ids(&["relative.md"]), None),
            (Vec::new(), None),
        ] {
            assert!(notify_root(&given, &cwd).is_absolute(), "{given:?} / {cwd:?}");
        }
    }

    /// `from_notify` が `notify_root` を通っていること。`files_root` に差し替えても
    /// 単体では動いてしまうので、**この関数の存在理由そのもの**をここで押さえる。
    #[test]
    fn a_notification_cold_start_roots_at_the_working_dir_not_the_shared_parent() {
        let dir = std::env::temp_dir().join("md-notify-coldstart");
        let _ = std::fs::remove_dir_all(&dir);
        let (a, b) = (dir.join("one"), dir.join("two"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("x.md"), "# x").unwrap();
        std::fs::write(b.join("y.md"), "# y").unwrap();

        // cwd の外にある、共通の親が cwd ではない 2 本。`files_root` ならその共通の親
        // （…/md-notify-coldstart）になるところを、cwd のままにする。
        let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
        let args = vec![
            a.join("x.md").to_string_lossy().into_owned(),
            b.join("y.md").to_string_lossy().into_owned(),
        ];
        let config = AppConfig::from_notify(&args, "", "", &Some(cwd.clone()));
        assert_eq!(config.root_dir, cwd);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `spool_stdin` が作るのと同じ形の（存在しない）パス。
    fn spooled(name: &str) -> PathBuf {
        canonical(std::env::temp_dir()).join(format!("{}{}", STDIN_DIR_PREFIX, name)).join("stdin.md")
    }

    #[test]
    fn stdin_opens_the_spooled_file_as_an_out_of_root_tab() {
        // stdin は root（cwd）の外の一時ファイルとして開く。識別子が絶対パスのまま
        // 乗ることと、片付け先のディレクトリを覚えていることを押さえる。
        let doc = spooled("test");
        let cwd = Some(PathBuf::from("/work"));
        let config = AppConfig::stdin_doc(doc.clone(), "/* theme */", "/* custom */", &cwd);

        assert_eq!(config.root_dir, PathBuf::from("/work"));
        assert_eq!(config.title, "work", "タイトルは root（cwd）の名前");
        assert_eq!(config.stdin_dir, doc.parent().map(|p| p.to_path_buf()));
        let html = String::from_utf8(config.html_bytes).unwrap();
        assert!(
            html.contains(&format!(r#"var INITIAL_FILES = ["{}"];"#, doc.to_string_lossy())),
            "stdin のファイルが絶対パスの識別子でタブに乗っていない: {html}"
        );
    }

    #[test]
    fn the_dir_we_actually_write_to_is_recognized_as_ours() {
        // 上の 2 つは組み立てたパスで判定を見ている。実際に書き出した場所がその形と
        // 一致しているか（$TMPDIR の正規化のズレで片付けが黙って効かなくならないか）を
        // 往復で確かめる。
        let doc = write_spool("# x\n");
        assert!(doc.is_file(), "書き出せていない: {}", doc.display());
        let dir = owned_stdin_dir(&doc);
        assert_eq!(dir.as_deref(), doc.parent(), "自分が掘った場所を片付け対象にできていない");
        let _ = std::fs::remove_dir_all(dir.unwrap());
    }

    #[test]
    fn stdin_root_is_the_working_directory_and_falls_back_to_the_spool() {
        let doc = spooled("slash");
        // cwd はそのまま root。`/` も例外ではない。
        assert_eq!(stdin_root(&doc, &Some(PathBuf::from("/work"))), PathBuf::from("/work"));
        assert_eq!(stdin_root(&doc, &Some(PathBuf::from("/"))), PathBuf::from("/"));
        // cwd が取れないときだけ、一時ファイルの置き場所へ逃がす（相対パスにしない）。
        assert_eq!(stdin_root(&doc, &None), doc.parent().unwrap());
    }

    #[test]
    fn only_our_own_spool_dir_is_ever_deleted() {
        // MD_STDIN_FILE は環境変数なので外から任意の場所を指せる。自分が掘る形
        // （$TMPDIR/md-stdin-*/）以外を片付け対象にすると、その親ごと消してしまう。
        assert!(owned_stdin_dir(&spooled("42")).is_some());
        // $TMPDIR 直下のファイル → $TMPDIR そのものを消してはいけない。
        assert_eq!(owned_stdin_dir(&canonical(std::env::temp_dir()).join("stdin.md")), None);
        // 名前が違う / 場所が $TMPDIR の下でない。
        assert_eq!(owned_stdin_dir(&canonical(std::env::temp_dir()).join("other/stdin.md")), None);
        assert_eq!(owned_stdin_dir(Path::new("/etc/hosts")), None);
    }

    #[test]
    fn the_gate_takes_the_file_not_the_directory() {
        // 転送（#31）は送り側と受け側が**同じ値を同じ門に通す**ことで成り立つ。
        // ワイヤに載せるのはファイルで、消してよい親は門が返す。
        let doc = spooled("42");
        let dir = owned_stdin_dir(&doc).expect("ファイルなら通る");

        // その返り値（ディレクトリ）をもう一度門へ入れると必ず弾かれる。門は親を
        // 見るので $TMPDIR にぶつかるため。ここを取り違えると、受け側が引き取れず
        // 転送したぶんの一時ファイルが黙って漏れる（実際に一度漏らした）。
        assert_eq!(owned_stdin_dir(&dir), None, "ディレクトリを載せてはいけない");
    }

    #[test]
    fn quick_access_reaches_the_page_in_order_with_its_kind() {
        use crate::quick_access::Entry;
        let list = vec![
            Entry { path: "/a/proj".into(), is_dir: true },
            Entry { path: "/a/n\"1.md".into(), is_dir: false },
        ];
        assert_eq!(
            quick_json(&list),
            r#"{"path":"/a/proj","dir":true},{"path":"/a/n\"1.md","dir":false}"#
        );
    }

    #[test]
    fn nothing_pinned_is_an_empty_array_not_a_hole() {
        assert_eq!(quick_json(&[]), "");
    }

    #[test]
    fn plan_paths_opens_a_dir_without_initial_files() {
        let dir = std::env::temp_dir().join(format!("md-plan-dir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "# a\n").unwrap();

        let arg = dir.to_string_lossy().into_owned();
        let (root, ids) = plan_paths(&[arg], &None);
        assert_eq!(root, dir.canonicalize().unwrap());
        assert!(ids.is_empty(), "フォルダ指定で初期表示するファイルは無い: {ids:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plan_paths_makes_an_out_of_cwd_file_a_folder_rooted_at_its_parent() {
        // 単一ファイルモードを畳んだ結果、cwd の外のファイルも「親フォルダを root に
        // したツリー付きの表示」で開く（識別子は canonicalize 済みの絶対パス）。
        let dir = std::env::temp_dir().join(format!("md-plan-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.md");
        std::fs::write(&file, "# note\n").unwrap();

        let cwd = Some(PathBuf::from("/definitely/not/here"));
        let (root, ids) = plan_paths(&[file.to_string_lossy().into_owned()], &cwd);
        assert_eq!(root, dir.canonicalize().unwrap());
        assert_eq!(ids, vec![file.canonicalize().unwrap().to_string_lossy().into_owned()]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
