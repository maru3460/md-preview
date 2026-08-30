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

/// ウィンドウ起動に必要な、入力から決まる設定一式。
pub struct AppConfig {
    pub title: String,
    pub html_bytes: Vec<u8>,
    pub root_dir: PathBuf,
    /// stdin を実体化した一時ディレクトリ。ウィンドウを閉じるときに消す。
    /// stdin 以外では None。
    pub stdin_dir: Option<PathBuf>,
}

impl AppConfig {
    /// ページの JS から読む起動時グローバル。ウィンドウ表示では初期化スクリプト
    /// （WKUserScript）として注入される。これはページのスクリプトより先に走り、
    /// ページの CSP の対象外なので、本文の inline script が禁止されていても
    /// 各モジュールはこれらを読める。
    ///
    /// - `MD_APPEARANCE`     解決済みテーマの外観。JS で描く図（mermaid）を OS 設定では
    ///                       なくテーマに追従させる。
    /// - `MD_RENDERABLE_EXT` レンダリング対象の拡張子。定義元は `request::RENDERABLE_EXT`。
    /// - `MD_ROOT_DIR`       配信ルートの絶対パス。JS が root 相対の識別子と絶対パスを
    ///                       行き来するために要る（root の外を指すリンクを、黙って root で
    ///                       止めずに絶対パスとして開くため）。
    pub fn page_globals(&self, appearance: crate::theme::Appearance) -> String {
        let renderable = request::RENDERABLE_EXT
            .iter()
            .map(|e| json_string(e))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "window.MD_APPEARANCE = {}; window.MD_RENDERABLE_EXT = [{}]; window.MD_ROOT_DIR = {};",
            json_string(appearance.as_str()),
            renderable,
            json_string(&self.root_dir.to_string_lossy()),
        )
    }

    /// ツリー付きのページ。`initial_files` は起動時にタブとして開く識別子
    /// （root 相対パス、または root の外なら絶対パス。先頭が最初に表示される）。
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
        let id = file_id(&root, &doc);
        let mut config = Self::folder(root, theme_css, custom_css, &[id]);
        config.stdin_dir = stdin_dir_to_clean(&doc);
        config
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

/// ウィンドウのタイトルに使うディレクトリ名。
fn dir_name(p: &Path) -> String {
    p.file_name().and_then(|n| n.to_str()).unwrap_or(".").to_string()
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

    let paths = resolve_file_args(args);
    let root = files_root(&paths, current_dir);
    let ids = paths.iter().map(|p| file_id(&root, p)).collect();
    (root, ids)
}

/// ファイル指定の引数を絶対パスへ解決する。重複は 1 つにまとめる。
fn resolve_file_args(args: &[String]) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for arg in args {
        let path = resolve_arg_path(arg);
        // フォルダは root を決める側なので、複数指定には混ぜられない
        // （2 つのツリーを同時に出す作りになっていない）。
        if path.is_dir() {
            eprintln!("md: 複数指定できるのはファイルだけです（'{}' はフォルダ）", arg);
            std::process::exit(1);
        }
        // タブの識別子はパスなので、同じファイルを 2 回渡されても 1 枚にまとめる。
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// タブに乗せるファイルたちを収める root を決める。
///
/// 全部が cwd 配下ならこれまでどおり cwd を root にし、そうでなければ指定された
/// ファイルたちの共通の親まで広げる（1 つだけなら、そのファイルの親ディレクトリ）。
fn files_root(paths: &[PathBuf], current_dir: &Option<PathBuf>) -> PathBuf {
    let root = current_dir
        .clone()
        .filter(|cwd| paths.iter().all(|p| p.starts_with(cwd)))
        .or_else(|| common_ancestor(paths))
        .unwrap_or_else(|| PathBuf::from("/"));
    // root がファイルシステムの根まで広がったら開かない。root は再帰監視され、
    // ツリーは各ディレクトリに has_md を投げて全走査するので、`/` を掴むと
    // `/System` などを舐めて固まる。
    if root.parent().is_none() {
        eprintln!("md: root がファイルシステムの根（'/'）に広がるため開けません");
        eprintln!("    同じフォルダのファイルを指定するか、フォルダごと開いてください");
        std::process::exit(1);
    }
    root
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

/// stdin を開くときの root。作業ディレクトリを使うが、そこが `/` のときだけは
/// 一時ファイルの置き場所へ逃がす。`/` を root にすると再帰監視とツリーの
/// 全走査が `/System` などを舐めて固まる（`files_root` の門と同じ理由）。
/// ファイル指定と違ってユーザーは root を指定していないので、ここは終了させずに畳む。
fn stdin_root(doc: &Path, current_dir: &Option<PathBuf>) -> PathBuf {
    let cwd = current_dir.clone().unwrap_or_else(|| PathBuf::from("."));
    if cwd.parent().is_none() {
        return doc.parent().unwrap_or(&cwd).to_path_buf();
    }
    cwd
}

/// ウィンドウを閉じるときに消してよい一時ディレクトリ。
///
/// `STDIN_FILE_ENV` は環境変数なので、外から任意の場所を指せる。`doc` の親を
/// 無条件に消すと `MD_STDIN_FILE=/etc/hosts` で `/etc` が飛ぶので、
/// 自分が掘る形（`$TMPDIR/md-stdin-<pid>/`）に一致するものだけを対象にする。
fn stdin_dir_to_clean(doc: &Path) -> Option<PathBuf> {
    let dir = doc.parent()?;
    if !dir.file_name()?.to_str()?.starts_with(STDIN_DIR_PREFIX) {
        return None;
    }
    if dir.parent()? != canonical(std::env::temp_dir()) {
        return None;
    }
    Some(dir.to_path_buf())
}

/// stdin の markdown を実体のファイルにする。自己デタッチした場合は親が読んで
/// 書き出しているので、そのパスをそのまま使う（子は標準入力を持たない）。
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
        // 共通の親が `/` まで広がるケースは値を返さずプロセスを終える（ツリーの
        // 全走査で固まるため）ので、ここでは呼ばない。
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
        let dir = stdin_dir_to_clean(&doc);
        assert_eq!(dir.as_deref(), doc.parent(), "自分が掘った場所を片付け対象にできていない");
        let _ = std::fs::remove_dir_all(dir.unwrap());
    }

    #[test]
    fn stdin_root_never_becomes_the_filesystem_root() {
        // cwd が `/` のときに root を `/` にすると、再帰監視とツリーの全走査で固まる。
        // ユーザーは root を指定していないので、終了させずに一時ファイルの場所へ逃がす。
        let doc = spooled("slash");
        assert_eq!(stdin_root(&doc, &Some(PathBuf::from("/"))), doc.parent().unwrap());
        // 普通の cwd はそのまま root。
        assert_eq!(stdin_root(&doc, &Some(PathBuf::from("/work"))), PathBuf::from("/work"));
    }

    #[test]
    fn only_our_own_spool_dir_is_ever_deleted() {
        // MD_STDIN_FILE は環境変数なので外から任意の場所を指せる。自分が掘る形
        // （$TMPDIR/md-stdin-*/）以外を片付け対象にすると、その親ごと消してしまう。
        assert!(stdin_dir_to_clean(&spooled("42")).is_some());
        // $TMPDIR 直下のファイル → $TMPDIR そのものを消してはいけない。
        assert_eq!(stdin_dir_to_clean(&canonical(std::env::temp_dir()).join("stdin.md")), None);
        // 名前が違う / 場所が $TMPDIR の下でない。
        assert_eq!(stdin_dir_to_clean(&canonical(std::env::temp_dir()).join("other/stdin.md")), None);
        assert_eq!(stdin_dir_to_clean(Path::new("/etc/hosts")), None);
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
        // したツリー付きの表示」で開く（識別子はその root 相対＝ファイル名）。
        let dir = std::env::temp_dir().join(format!("md-plan-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.md");
        std::fs::write(&file, "# note\n").unwrap();

        let cwd = Some(PathBuf::from("/definitely/not/here"));
        let (root, ids) = plan_paths(&[file.to_string_lossy().into_owned()], &cwd);
        assert_eq!(root, dir.canonicalize().unwrap());
        assert_eq!(ids, vec!["note.md".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
