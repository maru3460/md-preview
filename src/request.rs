use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::html::{attr_escape, build_html, html_escape, json_string, parse_frontmatter, render_body_in, render_frontmatter_html, DRAWIO_JS, MERMAID_JS};
use crate::urlpath::{asset_url, display_id, DocBase, ABS_PREFIX};
pub use crate::urlpath::{file_id, percent_decode};


type Response = wry::http::Response<Cow<'static, [u8]>>;

/// すべての応答に付ける。WebKit にディスクキャッシュを作らせないため。
///
/// 付けないと、ヘッダを見て貯めるかどうかをブラウザが自分で決める（実測で
/// `~/Library/Caches/md` が 15MB まで育っていた）。ここで貯める価値は無い:
/// vendor の JS はバイナリ埋め込みでメモリから配っており、キャッシュから読むより速い。
/// むしろ古い内容が返ってホットリロードが効かなくなる方が困る。
const NO_STORE: (&str, &str) = ("Cache-Control", "no-store");

pub fn ok_response(content_type: &str, body: Vec<u8>) -> Response {
    wry::http::Response::builder()
        .header("Content-Type", content_type)
        .header(NO_STORE.0, NO_STORE.1)
        .body(Cow::Owned(body))
        .unwrap()
}

pub fn not_found_response() -> Response {
    wry::http::Response::builder()
        .status(404)
        .header(NO_STORE.0, NO_STORE.1)
        .body(Cow::Borrowed(b"Not Found" as &[u8]))
        .unwrap()
}

pub fn guess_mime(path: &Path) -> &'static str {
    // 判定側（is_html / is_md / JS の isHtmlPath）は拡張子を case-insensitive に見るので、
    // 配信側も小文字化して揃える。揃えないと `.HTML` が octet-stream で返り iframe が空白になる。
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("bmp") => "image/bmp",
        // html を iframe で描画するので、本体と、そこから参照される css/js/フォント/
        // 画像などのサブリソースも正しい Content-Type で返す（WKWebView は strict-MIME
        // で text/css・text/javascript 以外の CSS/JS 適用を拒否するため）。
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs" | "cjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        _ => "application/octet-stream",
    }
}

pub fn extension_to_hljs_lang(path: &Path) -> &'static str {
    // guess_mime と揃えて拡張子は case-insensitive に見る（`.RS` でも rust 扱い）。
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("md" | "markdown") => "markdown",
        Some("rs") => "rust",
        Some("js" | "mjs" | "cjs") => "javascript",
        Some("ts") => "typescript",
        Some("tsx" | "jsx") => "javascript",
        Some("py") => "python",
        Some("go") => "go",
        Some("java") => "java",
        Some("c" | "h") => "c",
        Some("cpp" | "cc" | "cxx" | "hpp") => "cpp",
        Some("cs") => "csharp",
        Some("rb") => "ruby",
        Some("sh" | "bash" | "zsh" | "fish") => "bash",
        Some("json") => "json",
        Some("yaml" | "yml") => "yaml",
        Some("toml") => "toml",
        Some("html" | "htm" | "xml") => "xml",
        Some("css") => "css",
        Some("scss" | "sass") => "scss",
        Some("sql") => "sql",
        Some("kt" | "kts") => "kotlin",
        Some("swift") => "swift",
        Some("lua") => "lua",
        Some("php") => "php",
        _ => "plaintext",
    }
}

pub fn list_dir_json(dir: &Path) -> Vec<u8> {
    let mut dirs: Vec<(String, String)> = Vec::new();
    let mut files: Vec<(String, String)> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let entry_path = entry.path();
            let id = file_id(&entry_path);

            if entry_path.is_dir() {
                dirs.push((name, id));
            } else {
                files.push((name, id));
            }
        }
    }

    dirs.sort_by(|a, b| a.0.cmp(&b.0));
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut items: Vec<String> = Vec::new();
    for (name, path) in dirs {
        items.push(format!(r#"{{"name":{},"path":{},"kind":"dir"}}"#, json_string(&name), json_string(&path)));
    }
    for (name, path) in files {
        items.push(format!(r#"{{"name":{},"path":{},"kind":"file"}}"#, json_string(&name), json_string(&path)));
    }

    format!("[{}]", items.join(",")).into_bytes()
}

/// ファイル検索（⌘P）へ渡す一覧の上限。巨大リポジトリで walk が返ってこなくなるのを
/// 防ぐため、件数と深さの両方で切る。打ち切ったら `truncated:true` を添えて返し、
/// クライアント側が「一部のみ」と明示できるようにする。
const FILE_LIST_MAX: usize = 20_000;
const FILE_LIST_MAX_DEPTH: usize = 16;
const FILE_LIST_MAX_DIRS: usize = 50_000;

/// ツリーのドット判定（`md_presence`）の上限。ファイル一覧と別の値なのは、走り方が
/// 違うから。一覧は ⌘P を開いた時に 1 本だけ走り、1 回で全部を出し切る必要がある。
/// ドットは折りたたみを開くたびにフォルダ行の数だけ走り、聞かれたディレクトリが毎回
/// 起点になる（1 段開けば予算が補充される）。だから浅く・少なく切ってよい。
/// 件数に相当する上限が無いのは、返すのが 3 値で「出力の大きさ」という概念が無いため。
/// 実コストは `read_dir` の回数にほぼ比例するので、それを直接縛れば足りる。
const HAS_MD_MAX_DEPTH: usize = 8;
const HAS_MD_MAX_DIRS: usize = 2_000;

/// 再帰探索から外すディレクトリ名。「読む対象ではないのに数万ファイルある」ものだけを
/// 挙げる（VCS の内部・依存物・ビルド生成物）。ここで外してもサイドバーのツリーは
/// 従来どおり全部見せるので、到達できなくなるファイルは無い。
///
/// 利用者は `collect_files`（⌘P の一覧）と `md_presence`（ツリーのドット）の 2 つ。
/// ドット側に通したことで、その意味は「配下のどこかに md」ではなく「除外リストの外の
/// どこかに md」になった。結果として `proj/node_modules/pkg/readme.md` だけを持つ
/// `proj` は暗いまま、その中の `node_modules` 行にはドットが点く、という見え方になる。
/// 親より子が明るいのは妙だが、ドットは「読む価値のある md の在り処」の印なので、
/// 依存物の同梱 README で親を光らせるほうが誤りだと判断した。
/// 逆に `.github` `.vscode` のような設定系は読みたい対象なので、隠しディレクトリを
/// 一律で外すことはしない。
fn is_skipped_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | ".next"
            | ".nuxt"
            | ".svelte-kit"
            | ".turbo"
            | ".parcel-cache"
            | ".cache"
            | ".venv"
            | "venv"
            | "__pycache__"
            | ".mypy_cache"
            | ".pytest_cache"
            | ".ruff_cache"
            | ".gradle"
            | ".idea"
            | ".terraform"
            | "Pods"
    )
}

/// 探索を打ち切った理由。どの上限に当たったかで警告の文面が変わるので、
/// `bool` に畳まず理由まで持ち帰る（「20,000 件で打ち切り」と出したのに実際は
/// 深さで切れていた、という嘘を防ぐため）。
#[derive(Debug, PartialEq, Eq)]
enum Truncation {
    /// ファイル件数の上限に当たって探索自体を止めた。
    Files,
    /// 読んだディレクトリ数の上限に当たって探索自体を止めた。
    Dirs,
    /// 深すぎる枝を刈った。探索自体は最後まで回っている。
    Depth,
}

/// root 以下のファイルを識別子（絶対パス）で `out` に集める。打ち切ったら理由を返す。
///
/// **幅優先で辿る**。深さ優先だと、上限に当たったときに「名前順で最初の枝だけ全部」という
/// 偏り方をする。例えば `md /` では `/Applications` の `.app` の中身で 20,000 件を使い切り、
/// `/Users` に一度も到達しない。幅優先なら上限は「深い階層」を削るだけなので、どこを
/// ルートにしても浅い階層は必ず一覧に入る（クエリ未入力の一覧も浅い順になる）。
///
/// 各階層は「ファイル（名前順）→ サブディレクトリ（名前順）」の順に処理する。
/// シンボリックリンクのディレクトリは辿らない（`file_type()` はリンクを追わないので
/// `is_dir()` が false になる）。循環でハングするのを構造的に防ぐため。
fn collect_files(root: &Path, out: &mut Vec<String>) -> Option<Truncation> {
    // (ディレクトリ, 深さ)。pop_front / push_back で階層順に処理する。
    let mut queue: std::collections::VecDeque<(PathBuf, usize)> = std::collections::VecDeque::new();
    queue.push_back((root.to_path_buf(), 0));
    let mut visited_dirs = 0usize;
    // 深さ上限で刈った枝があったか。刈っても探索は続くので、最後まで回ってから報告する。
    let mut deep_skipped = false;

    while let Some((dir, depth)) = queue.pop_front() {
        if out.len() >= FILE_LIST_MAX {
            return Some(Truncation::Files);
        }
        // ファイルが少なくディレクトリだけが膨大な木で、キューと read_dir が
        // 際限なく増えるのを防ぐ（ファイル件数の上限だけでは止まらないため）。
        visited_dirs += 1;
        if visited_dirs > FILE_LIST_MAX_DIRS {
            return Some(Truncation::Dirs);
        }

        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<String> = Vec::new();
        let mut subdirs: Vec<PathBuf> = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                // 除外リストは意図した除外なので報告しない。深さ上限は「見えていない
                // ファイルがある」ことを伝えたいので区別して記録する。
                if is_skipped_dir(&name) {
                    continue;
                }
                if depth + 1 > FILE_LIST_MAX_DEPTH {
                    deep_skipped = true;
                    continue;
                }
                subdirs.push(entry.path());
            } else {
                files.push(name);
            }
        }
        files.sort();
        subdirs.sort();

        for name in files {
            if out.len() >= FILE_LIST_MAX {
                return Some(Truncation::Files);
            }
            out.push(file_id(&dir.join(name)));
        }
        for sub in subdirs {
            queue.push_back((sub, depth + 1));
        }
    }
    if deep_skipped {
        Some(Truncation::Depth)
    } else {
        None
    }
}

/// 配下に Markdown があるか。予算を使い切って答えが出なければ `Unknown`。
///
/// `Truncation` を使い回さないのは、`Truncation::Files` がこちらでは絶対に起きない
/// ＝到達しない variant を型に抱えることになるため。`Option<bool>` も採らない
/// （`None` が「不明」なのか「読めなかった」なのか、呼ぶ側から区別できない）。
#[derive(Debug, PartialEq, Eq)]
enum MdPresence {
    Yes,
    No,
    /// 予算を使い切って答えが出なかった。「無い」と言い切らないための 3 つ目。
    Unknown,
}

/// `dir` の配下に Markdown があるかを、予算の範囲で調べる。
///
/// **幅優先で辿る**。理由は `collect_files` と共通（深さ優先は上限に当たったときに
/// 「名前順で最初の枝だけ深く」という偏り方をする）だが、こちらにはもう 1 つある。
/// 大多数のフォルダは直下に README.md を持つので、幅優先なら `read_dir` 1 回で
/// `Yes` が返る。深さ優先だと名前順で最初のサブディレクトリへ先に潜ってしまう。
///
/// 時間による打ち切りは置かない。マシンの負荷で同じフォルダの答えが変わると、
/// ドットが点いたり消えたりする。件数で切れば同じ木には必ず同じ答えが出る。
///
/// 走査の中断（`AtomicBool` 等）も持たない。畳んだフォルダの走査を止めたくなるが、
/// 予算が入った今は 1 本の最悪コストが `read_dir` を `HAS_MD_MAX_DIRS` 回で有界。
/// 止めるには wry のカスタムプロトコルとは別の通知経路と、パスごとのフラグ登録簿が
/// 要る（`RequestContext` に共有可変状態が生える）。割に合わないので、
/// 「要らなくなったものを投げない」側は `folder.js` の待ち行列が持つ。
fn md_presence(dir: &Path) -> MdPresence {
    // (ディレクトリ, 深さ)。pop_front / push_back で階層順に処理する。
    let mut queue: std::collections::VecDeque<(PathBuf, usize)> = std::collections::VecDeque::new();
    queue.push_back((dir.to_path_buf(), 0));
    let mut visited_dirs = 0usize;
    // 深さ上限で刈った枝があったか。刈っても他の枝は歩くので、最後まで回してから使う。
    let mut deep_skipped = false;

    while let Some((current, depth)) = queue.pop_front() {
        visited_dirs += 1;
        if visited_dirs > HAS_MD_MAX_DIRS {
            return MdPresence::Unknown;
        }
        // 読めないディレクトリは展開しても空なので、提示できる md は無い＝ No 側へ倒す。
        // Unknown にはしない。Unknown は「予算切れ」だけを指す語に保ちたいため
        // （引き換えに、権限で読めない枝は畳んで開き直しても再挑戦しない）。
        let Ok(entries) = std::fs::read_dir(&current) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                // 除外リストは意図した除外なので Unknown にしない。Unknown は
                // 「予算切れ」だけを指す語に保つ。起点そのものは除外しないので、
                // `?has_md=node_modules` と聞かれれば中身を見る。
                if is_skipped_dir(&name.to_string_lossy()) {
                    continue;
                }
                if depth + 1 > HAS_MD_MAX_DEPTH {
                    deep_skipped = true;
                    continue;
                }
                queue.push_back((entry.path(), depth + 1));
            } else if classify_ext(Path::new(&name)) == ViewKind::Markdown {
                return MdPresence::Yes;
            }
        }
    }

    // 刈った枝があっても、他の枝で見つかっていれば上で Yes を返している。
    // ここに来るのは「見つからなかった」場合だけなので、刈った＝答えが出ていない。
    if deep_skipped {
        MdPresence::Unknown
    } else {
        MdPresence::No
    }
}

/// ファイル検索（⌘P）用に、root 以下の全ファイルを識別子（絶対パス）の JSON で返す。
/// 画面に出す形へ畳むのは JS 側（`MdCommon.idToDisplay`）。
///
/// 打ち切った時は `reason` と、その理由に対応する上限値 `limit` を添える。上限の数値を
/// JS 側に二重定義せず、警告文を必ず実際の上限と一致させるため（片方だけ直して文面が
/// 嘘になるのを防ぐ）。
fn handle_files(root_dir: &Path) -> Response {
    let mut paths: Vec<String> = Vec::new();
    let trunc = collect_files(root_dir, &mut paths);
    let (reason, limit) = match trunc {
        None => ("", 0),
        Some(Truncation::Files) => ("files", FILE_LIST_MAX),
        Some(Truncation::Dirs) => ("dirs", FILE_LIST_MAX_DIRS),
        Some(Truncation::Depth) => ("depth", FILE_LIST_MAX_DEPTH),
    };
    let items: Vec<String> = paths.iter().map(|p| json_string(p)).collect();
    let body = format!(
        r#"{{"files":[{}],"truncated":{},"reason":"{}","limit":{}}}"#,
        items.join(","),
        trunc.is_some(),
        reason,
        limit
    );
    ok_response("application/json; charset=utf-8", body.into_bytes())
}

/// 変更のあるファイルの一覧（⌘P が変更のあるファイルを先に出すために使う）。
/// ファイル一覧（files=1）とは別エンドポイントにしてある。git を叩くので失敗しても
/// ファイル一覧の取得を巻き込まない（その場合パレットは並べ替えなしで普通に動く）。
///
/// パスは `?files=1` と同じ識別子（絶対パス）。⌘P は 2 つの一覧を突き合わせるので、
/// 片方だけ形が違うとバッジが一生付かず、並べ替えも効かない。
fn handle_changed(root_dir: &Path) -> Response {
    let items: Vec<String> = crate::diff::changed_files(root_dir)
        .iter()
        .map(|(path, add, del)| {
            format!(r#"{{"path":{},"add":{},"del":{}}}"#, json_string(path), add, del)
        })
        .collect();
    let body = format!(r#"{{"changed":[{}]}}"#, items.join(","));
    ok_response("application/json; charset=utf-8", body.into_bytes())
}

/// URL パス（`/docs/fig.png`）を root 配下の実ファイルへ畳む。
///
/// **識別子ではなく URL の関門**。識別子は [`id_to_path`] が受ける。こちらを残して
/// あるのは、root の中から外へ出る道が 2 つあるため——iframe 内の html が組んだ
/// `../` と、root の中にある root の外を指すシンボリックリンク。`canonicalize` して
/// から `starts_with` で見るので、どちらもここで 404 になる。
///
/// 結果として、そのシンボリックリンクは `?file=` なら本文として開けるのに、画像
/// としては引けない。非対称だが、URL の側を緩めると `/__abs/` を置いた意味
/// （root の外は「外だと分かる形」で配る）が消えるので、こちらに寄せてある。
fn safe_join(canonical_root: &Path, rel: &str) -> Option<PathBuf> {
    // 本物の親ディレクトリ参照（`..` パス要素）だけを拒否し、単に `..` を部分
    // 文字列として含むだけのファイル名（例: `my..file.md`）は拒否しない。
    if Path::new(rel).components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return None;
    }
    let candidate = canonical_root.join(rel);
    let canonical = candidate.canonicalize().ok()?;
    if canonical.starts_with(canonical_root) {
        Some(canonical)
    } else {
        None
    }
}

/// ファイルの見せ方。**拡張子と中身だけで決まる、この 4 択が唯一の分類**。
///
/// 以前は「.md はレンダリング / .html は iframe / それ以外はソース / 非 UTF-8 は通知」
/// という同じ判断が main.rs・request.rs・folder.js の 7 箇所に独立して書かれており、
/// コメントで「揃える」と human に保証させていた（実際 .markdown がウォッチャから
/// 漏れる回帰が起きた）。新しい表示対象を足すときは、この enum と `classify_ext` /
/// `render_file` だけを触ればよい。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewKind {
    /// Markdown としてレンダリングする。
    Markdown,
    /// iframe（ミニブラウザ）で描画する。
    HtmlPage,
    /// 行番号つきのソースビューで見せる。
    Source,
    /// 非 UTF-8。中身を見せずに通知だけ出す。
    Binary,
}

/// 通常表示がレンダリング結果になる拡張子。`raw` トグルが意味を持つ対象でもある。
/// JS 側（folder.js の `isRenderablePath`）へは起動時にこの配列をそのまま注入するので、
/// **拡張子の定義元はここ 1 箇所**。
pub const RENDERABLE_EXT: &[&str] = &["md", "markdown", "html", "htm"];

fn ext_lower(path: &Path) -> Option<String> {
    path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase())
}

/// 中身を読む前の分類。`Binary` は返さない（中身を見ないと判らないため）。
/// ウォッチャの対象判定や `raw` の可否など、ファイルを開かずに決めたい場面で使う。
pub fn classify_ext(path: &Path) -> ViewKind {
    match ext_lower(path).as_deref() {
        Some("md" | "markdown") => ViewKind::Markdown,
        Some("html" | "htm") => ViewKind::HtmlPage,
        _ => ViewKind::Source,
    }
}

/// 通常表示がレンダリング結果になるか（＝`RENDERABLE_EXT` に載っているか）。
pub fn is_renderable(path: &Path) -> bool {
    matches!(classify_ext(path), ViewKind::Markdown | ViewKind::HtmlPage)
}

/// 表示のしかた。`Normal` は拡張子どおり、`RawSource` は raw トグル用に
/// md / html でも必ずソースとして見せる。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Normal,
    RawSource,
}

/// 1 ファイルを描画した結果。ページ全体にもフラグメントにも使えるよう、
/// **ラッパ（`.markdown-body`）を付けない中身だけ**を持つ。
pub struct RenderedFile {
    pub kind: ViewKind,
    /// 本文 HTML。
    pub html: String,
    /// `.markdown-body` に足す追加クラス（`""` / `"source-page"` / `"html-page"`）。
    pub body_class: &'static str,
}

/// 表示できない・読めないときの通知段落。文言とクラスをここに一本化する
/// （以前は同じ日本語が 4 箇所にあり、クラスも `binary-msg` と `diff-msg` に割れていた。
/// 前者は CSS 規則が無く、素のままの段落として出ていた）。
pub fn notice_html(msg: &str) -> String {
    format!(r#"<p class="md-notice">{}</p>"#, html_escape(msg))
}

fn binary_notice(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    notice_html(&format!("バイナリファイルは表示できません: {}", name))
}

/// ファイルを読んで本文 HTML を組む。**すべての表示経路（`?file=` / `?raw=` /
/// md の直開き / `--html` ダンプ）がここを通る。**
///
/// `root` は配信ルート。相対 `src` / `href` の書き換え先 URL と、html を iframe 描画
/// するときの `src` をここから組む（[`crate::urlpath`] 参照）。相対パスの基準は root
/// ではなく `path` の親であることに注意（root 固定だと `docs/a.md` の `fig.png` が
/// root 直下を引きに行く）。
/// 読めなければ None（404 にするか通知を出すかは呼び出し側が決める）。
pub fn render_file(path: &Path, root: &Path, mode: ViewMode) -> Option<RenderedFile> {
    let kind = match mode {
        ViewMode::RawSource => ViewKind::Source,
        ViewMode::Normal => classify_ext(path),
    };

    // html の iframe 描画だけは中身を読まない（描くのは WebView 側）。
    if kind == ViewKind::HtmlPage {
        return Some(RenderedFile {
            kind,
            html: render_html_iframe(&asset_url(root, path), &display_id(root, &file_id(path))),
            body_class: "html-page",
        });
    }

    let bytes = std::fs::read(path).ok()?;
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        // 非 UTF-8 は source-page 扱いに揃える。raw トグルの可否判定（JS 側は
        // source-page の有無で見る）が経路によってブレないようにするため。
        Err(_) => {
            return Some(RenderedFile {
                kind: ViewKind::Binary,
                html: binary_notice(path),
                body_class: "source-page",
            })
        }
    };

    Some(match kind {
        ViewKind::Markdown => {
            let dir = path.parent().unwrap_or(root);
            let (fm_pairs, body) = parse_frontmatter(&text);
            // 行番号はファイルの行に揃える（フロントマターぶんを足す）。raw 表示（⌘R）の
            // 行番号や貼り付け先の file:line と一致させるため。
            let offset = crate::html::body_line_offset(&text, body);
            RenderedFile {
                kind,
                // 単独行ファイルリンクの相対パスは、その md ファイルがある場所を基準に解決する。
                html: format!(
                    "{}{}",
                    render_frontmatter_html(&fm_pairs, offset),
                    render_body_in(body, Some(&DocBase::new(dir, root)), offset)
                ),
                body_class: "",
            }
        }
        _ => RenderedFile {
            kind: ViewKind::Source,
            html: source_view_html(path, &text),
            body_class: "source-page",
        },
    })
}

/// html ファイルを iframe（ミニブラウザ）で描画するフラグメント（iframe 要素のみ）。
/// `url` は [`asset_url`] が組んだ絶対 URL パス。`src` を同一スキームのファイル URL に
/// することで、iframe 内の相対リンク・画像・CSS がその html の場所を基準に解決される。
/// root の外の html も `/__abs/` 配下で実パスの階層を保つので、同じ理屈で通る。
/// sandbox は付けない（ローカルファイル閲覧用途。JS 実行・同一オリジンを許可して忠実に描画）。
pub fn render_html_iframe(url: &str, title: &str) -> String {
    format!(
        r#"<iframe class="html-frame" src="{src}" title="{title}" referrerpolicy="no-referrer"></iframe>"#,
        src = attr_escape(url),
        // title は属性値なのでクォートも潰す attr_escape を使う（html_escape は " を素通しし、
        // " 入りファイル名で親の信頼ドキュメントへ属性注入できてしまう）。
        title = attr_escape(title),
    )
}

/// このサイズ / 行数を超えるソースは hljs のハイライトを切る（プレーン表示）。
/// 巨大ファイルを全文同期ハイライトするとメインスレッドが数百 ms 凍結し、
/// `<code>` が数十万ノードに膨張して検索まで実質使用不能になるのを避けるため。
/// VSCode の largeFileOptimizations と同じ割り切り（大ファイルは色なし）。
const HIGHLIGHT_MAX_BYTES: usize = 1_000_000;
const HIGHLIGHT_MAX_LINES: usize = 10_000;

/// 生ソースを VSCode 風のソースビュー（ファイル名バー + 行番号ガター + コード）にする。
/// 行番号ガターはクライアント側（MdCommon.addLineNumbers）が .source-main に後付けする。
pub fn source_view_html(file_path: &Path, content: &str) -> String {
    let lang = extension_to_hljs_lang(file_path);
    let fname = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    // 閾値超過なら hljs をスキップさせる。`nohighlight` は hljs の noHighlightRe に
    // マッチするため highlightAll/highlightElement 双方が処理を飛ばし、<code> が
    // 1 テキストノードのまま残る（凍結・ノード爆発・検索激重を一括で回避）。
    // 行数は改行数で近似する（末尾改行の有無で ±1 ブレるがソフト閾値なので許容）。
    // バイト側と演算子を `>` で揃える。
    let too_big = content.len() > HIGHLIGHT_MAX_BYTES
        || content.bytes().filter(|&b| b == b'\n').count() > HIGHLIGHT_MAX_LINES;
    let code_class = if too_big {
        "nohighlight".to_string()
    } else {
        format!("language-{}", lang)
    };
    let label = if too_big {
        format!("{} · 大ファイルのためハイライト無効", lang_label(lang))
    } else {
        lang_label(lang)
    };
    format!(
        r#"<div class="source-view"><div class="source-titlebar"><span class="source-fname">{fname}</span><span class="source-lang">{label}</span></div><div class="source-main"><pre><code class="{code_class}">{code}</code></pre></div></div>"#,
        fname = html_escape(fname),
        label = html_escape(&label),
        code_class = code_class,
        code = html_escape(content),
    )
}

/// hljs の言語 ID をファイル名バー右端に出す表示名にする（"rust" → "Rust"）。
/// 頭文字を大文字にするだけでは崩れる略語・記号入りの言語名は個別に対応する。
fn lang_label(hljs_lang: &str) -> String {
    match hljs_lang {
        "plaintext" => return "Text".to_string(),
        "cpp" => return "C++".to_string(),
        "csharp" => return "C#".to_string(),
        "javascript" => return "JavaScript".to_string(),
        "typescript" => return "TypeScript".to_string(),
        "xml" => return "XML".to_string(),
        "css" => return "CSS".to_string(),
        "scss" => return "SCSS".to_string(),
        "sql" => return "SQL".to_string(),
        "json" => return "JSON".to_string(),
        "yaml" => return "YAML".to_string(),
        "toml" => return "TOML".to_string(),
        "php" => return "PHP".to_string(),
        _ => {}
    }
    let mut chars = hljs_lang.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn serve_builtin_lib(name: &str) -> Response {
    let js = match name {
        "mermaid.min.js" => MERMAID_JS,
        "drawio-viewer.min.js" => DRAWIO_JS,
        _ => return not_found_response(),
    };
    ok_response("application/javascript; charset=utf-8", js.as_bytes().to_vec())
}

/// ツリーの 2 つの経路（`?dir=` と `?has_md=`）が見てよいディレクトリ。
///
/// 同じ入力に同じ挙動を返す、を人力のコメントではなく 1 つの関数で担保する。
/// 受けるのは識別子だけ（root 自身も識別子で名指す）。root の外とディレクトリで
/// ないものは None（＝ 404）。
///
/// root の外を断るのはセキュリティの境界ではない（`?file=` も `/__abs/` も外を開ける）。
/// ツリーは root を見せる部品だ、という表示上の取り決めを 1 か所で守っているだけ。
fn resolve_tree_dir(id: &str, root_dir: &Path) -> Option<PathBuf> {
    id_to_path(id).filter(|p| p.is_dir() && p.starts_with(root_dir))
}

fn handle_dir(id: &str, root_dir: &Path) -> Response {
    match resolve_tree_dir(id, root_dir) {
        Some(dir) => ok_response("application/json; charset=utf-8", list_dir_json(&dir)),
        None => not_found_response(),
    }
}

/// URL パス → 実ファイル。`/__abs/` 配下は root の外の実パスをそのまま指す
/// （画像も html も、root の外にあるものはこちらを通る）。それ以外は root 相対で、
/// `safe_join` が root の外へ出るものを弾く。
///
/// `/__abs/` を置くと、iframe で描画している html の JS が同一オリジンのまま
/// root の外のファイルを読めるようになる。それでも置いているのは、`../assets/fig.png`
/// のような日常的な相対参照を表示するのに他の道が無いため（開くファイルを絞っても、
/// 画像は結局 root の外を引く）。信頼できない html を開かない、が前提。
fn asset_path(url_path: &str, root_dir: &Path) -> Option<PathBuf> {
    match url_path.strip_prefix(ABS_PREFIX) {
        Some(rest) => PathBuf::from(format!("/{}", rest)).canonicalize().ok(),
        None => safe_join(root_dir, url_path.strip_prefix('/').unwrap_or(url_path)),
    }
}

fn handle_asset(url_path: &str, root_dir: &Path, theme_css: &str, custom_css: &str) -> Response {
    let Some(file_path) = asset_path(url_path, root_dir) else { return not_found_response() };
    match classify_ext(&file_path) {
        // md を直接開いた場合はページとして描画する（本来のページと同じ経路）。
        ViewKind::Markdown => {
            let title = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("Markdown Preview");
            let Some(r) = render_file(&file_path, root_dir, ViewMode::Normal) else {
                return not_found_response();
            };
            let page = build_html(&r.html, title, theme_css, custom_css, r.body_class);
            ok_response("text/html; charset=utf-8", page.into_bytes())
        }
        // iframe に配信する html は 2 つ通してから返す。行の錨（`inject_src_lines`）と、
        // head 内 CSS が JS より先に適用されるようにする style-gate（`inject_style_gate`）。
        // 順序はどちらでもよい（差し込む文字列が互いのパターンを含まず、改行も足さない）。
        ViewKind::HtmlPage => {
            let Ok(bytes) = std::fs::read(&file_path) else { return not_found_response() };
            ok_response("text/html; charset=utf-8", inject_style_gate(inject_src_lines(bytes)))
        }
        // 画像・CSS・フォントなど。iframe 内の html から参照されるサブリソースを含む。
        _ => {
            let Ok(bytes) = std::fs::read(&file_path) else { return not_found_response() };
            ok_response(guess_mime(&file_path), bytes)
        }
    }
}

/// ASCII パターンを大文字小文字無視で探し、元バイト列での開始位置を返す。
fn find_ci_ascii(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (0..=hay.len() - needle.len())
        .find(|&i| hay[i..i + needle.len()].iter().zip(needle).all(|(a, b)| a.eq_ignore_ascii_case(b)))
}

/// `hay[i..]` が ASCII パターンで（大文字小文字無視で）始まるか。
fn starts_ci_at(hay: &[u8], i: usize, needle: &[u8]) -> bool {
    i + needle.len() <= hay.len()
        && hay[i..i + needle.len()].iter().zip(needle).all(|(a, b)| a.eq_ignore_ascii_case(b))
}

/// 本物の `</head>` の位置を返す。中身をタグとして読まない区間（`<script>` / `<style>` /
/// `<title>` などとコメント）はスキップするので、そこに現れる文字列 `</head>` を誤検出
/// しない（誤検出して script 内へ注入すると、注入した `</script>` がページの script を
/// 途中で閉じて壊すため）。見つからなければ None。
///
/// 走査は `skip_opaque` に寄せてある。かつてはここが独自に `<script` と `<!--` だけを
/// 見ていたが、`</script >`（名前の後の空白）で抜けられず、`<title></head></title>` では
/// title の中へ gate を入れていた。同じ仕事をする走査を 2 つ持つと、片方だけ直したときに
/// 配信の 2 段（行の錨と gate）が違う世界を見ることになる。
fn find_head_close(hay: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < hay.len() {
        if let Some(end) = skip_opaque(hay, i) {
            // 閉じられていない script 等は末尾までスキップされるので、ここで打ち切られる。
            i = end;
            continue;
        }
        if starts_ci_at(hay, i, b"</head>") {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// html の `</head>` 直前へ、パーサーブロッキングな空 script を1つ挿入する。
///
/// なぜ必要か: `<script>` を `<link rel=stylesheet>` より前に置くページ（例: SimpleCov の
/// カバレッジレポート）は、CSS 適用前にページ初期化(`$(document).ready`)が走ると、まだ効いて
/// いない `.hide{display:none}` を見て「もう表示されている」と誤判定し、本体を表示する処理が
/// 空振りする。その後 CSS が適用されて本体が隠れ、白紙/黒画面のまま止まる。custom scheme の
/// 配信タイミングのばらつきで CSS が遅れると顕在化する「読み込み順レース」。
///
/// 対策: `</head>` 直前（＝head 内の全 stylesheet より後）に空 script を1つ挟むと、
/// 「先行する stylesheet が適用されるまで script 実行＝ページ初期化を待つ」というブラウザ標準の
/// 挙動が働き、CSS が必ずページ初期化より先に適用される。見た目は変えず、無害な1タグを足すだけ。
/// バイト列で処理するので文字コードに依存しない。`</head>` が無ければそのまま返す。
/// 注入位置は `find_head_close`（script/コメント内の擬似 `</head>` を避ける）で決める。
fn inject_style_gate(bytes: Vec<u8>) -> Vec<u8> {
    const GATE: &[u8] = b"<script>/*md:style-gate*/void 0</script>";
    match find_head_close(&bytes) {
        Some(pos) => {
            let mut out = Vec::with_capacity(bytes.len() + GATE.len());
            out.extend_from_slice(&bytes[..pos]);
            out.extend_from_slice(GATE);
            out.extend_from_slice(&bytes[pos..]);
            out
        }
        None => bytes,
    }
}

/// この大きさを超える html には行を刻まない。属性を差し込むぶん配信バイトが膨らむ上、
/// 受け取った側は 1 万件級のユニットを走査することになる。刻まなければ錨が 1 つも無い
/// 表示のままなので、`comment.js` は「付けられない」案内に落ちる（壊れはしない）。
const STAMP_MAX_BYTES: usize = 1_000_000;

/// 行を刻む対象。**ブロック要素だけ**に絞る。`div` や `span` を入れてはいけない
/// ——`closest('[data-src-line]')` が最内の要素を返して単語 1 個にコメントが付き、
/// レンジの引用が `<body>` 1 個に畳まれ、ユニットの走査が 1 万件級になる。
/// 「浅く縦に並ぶユニット列」という md 側の前提を再現できる粒度がここ。
const BLOCK_TAGS: &[&[u8]] = &[
    b"h1", b"h2", b"h3", b"h4", b"h5", b"h6", b"p", b"li", b"tr", b"td", b"th",
    b"blockquote", b"pre", b"figure", b"figcaption", b"dt", b"dd", b"details", b"summary",
];

/// 中身をタグとして読まない要素。ここに現れる `<p>` はタグではないので、刻むと本文に
/// 属性文字列がそのまま見えてしまう。
///
/// HTML 仕様の raw text にはこのほか `iframe` / `noembed` / `noframes` / `noscript` も
/// あるが入れない——どれも**対応ブラウザでは中身を描画しない**フォールバック用なので、
/// 刻んでも誰の目にも触れないし、錨として掴めても行き先が無い。入れる基準は
/// 「中身が画面に出るか」で、`textarea` と `xmp` / `plaintext` がそれに当たる。
const OPAQUE_TAGS: &[(&[u8], &[u8])] = &[
    (b"<script", b"</script"),
    (b"<style", b"</style"),
    (b"<textarea", b"</textarea"),
    (b"<title", b"</title"),
    (b"<xmp", b"</xmp"),
    // 閉じタグを持たない（以降すべてがプレーンテキストになる）。見つからないので
    // 下の「閉じられていなければ末尾まで」にそのまま乗る。
    (b"<plaintext", b"</plaintext"),
];

/// `i` から始まる「中身を読まない」区間の終端（直後の位置）を返す。閉じられていなければ
/// 末尾まで——安全側に倒して、閉じ忘れたページの残り全部を素通しにする。
///
/// SVG / MathML の中は見ていない。あそこの `<title>` は RCDATA ではなく普通の要素なので
/// `<title/>` が本当に自己終了するが、ここは閉じタグを探して見つけられず、以降を全部
/// 素通しにする（錨が減るだけで配信物は壊れない）。区別するには要素の入れ子を追って
/// foreign content に居るかを知る必要があり、バイト走査のまま扱える範囲を超える。
fn skip_opaque(bytes: &[u8], i: usize) -> Option<usize> {
    if starts_ci_at(bytes, i, b"<!--") {
        return Some(match find_ci_ascii(&bytes[i..], b"-->") {
            Some(rel) => i + rel + b"-->".len(),
            None => bytes.len(),
        });
    }
    for (open, close) in OPAQUE_TAGS {
        if !starts_ci_at(bytes, i, open) {
            continue;
        }
        // `<script` と `<scripting` を分ける（開きタグはここで区切られる）。
        let after = i + open.len();
        if !bytes.get(after).map_or(true, |&c| is_tag_delim(c)) {
            continue;
        }
        return Some(find_close_tag(bytes, after, close).unwrap_or(bytes.len()));
    }
    None
}

/// `</script` のような閉じタグ名を探し、そのタグの終端（`>` の直後）を返す。
///
/// `</script>` という並びを丸ごと探すのでは足りない。ブラウザは名前の後の空白も許すので、
/// `</script >` や `</script\n>` で閉じているページだと次の本物の `</script>` まで
/// 素通しになり、その間のブロック要素が刻まれないまま落ちる。
fn find_close_tag(bytes: &[u8], from: usize, name: &[u8]) -> Option<usize> {
    (from..bytes.len()).find(|&i| {
        starts_ci_at(bytes, i, name)
            && bytes.get(i + name.len()).map_or(true, |&c| is_tag_delim(c))
    })
    .map(|i| tag_end(bytes, i))
}

fn is_tag_delim(c: u8) -> bool {
    matches!(c, b' ' | b'>' | b'/' | b'\t' | b'\n' | b'\r')
}

/// `i`（`<`）がタグの始まりか。HTML のトークナイザが「タグ開始」と見なすのは英字・`/`・
/// `!`・`?` が続くときだけで、`a < b` や `5<6` の `<` はただの文字として本文に残る。
fn is_tag_start(bytes: &[u8], i: usize) -> bool {
    bytes.get(i + 1).map_or(false, |&c| {
        c.is_ascii_alphabetic() || c == b'/' || c == b'!' || c == b'?'
    })
}

/// `i`（`<`）から始まるタグの終端（`>` の直後）を返す。閉じられていなければ末尾。
///
/// 引用符の中の `>` ではタグを閉じない。ここを見ないと `data-tpl="<li>x</li>"` のような
/// **属性値の中身を走査してしまい**、そこを開きタグと誤認して `data-src-line="N"` を
/// 差し込む。差し込む文字列に `"` が入っているので属性値がそこで閉じ、タグごと壊れる
/// （Bootstrap の `data-bs-content`、Alpine / Vue のインラインテンプレートで実在する書き方）。
fn tag_end(bytes: &[u8], i: usize) -> usize {
    let mut j = i + 1;
    let mut quote = 0u8;
    // 直前の非空白が `=` か。引用が始まるのは属性値の先頭だけなので、そこだけ見る
    // ——タグの中のどこの `'` でも引用扱いにすると、`<p title=Bob's>` のような引用符無しの
    // 属性値に含まれるアポストロフィで次の `'` か EOF まで飲み込み、その間のブロック要素が
    // 刻まれないまま落ちる。
    let mut after_eq = false;
    while j < bytes.len() {
        let c = bytes[j];
        if quote != 0 {
            if c == quote {
                quote = 0;
            }
        } else if (c == b'"' || c == b'\'') && after_eq {
            quote = c;
        } else if c == b'>' {
            return j + 1;
        }
        if !c.is_ascii_whitespace() {
            after_eq = c == b'=';
        }
        j += 1;
    }
    bytes.len()
}

/// `i`（`<`）から始まるのがブロック要素の開きタグなら、タグ名の直後の位置を返す。
/// 属性はそこへ差し込む。閉じタグ（`</p>`）・宣言（`<!DOCTYPE>`）・対象外のタグは None。
fn block_tag_end(bytes: &[u8], i: usize) -> Option<usize> {
    let start = i + 1;
    let mut j = start;
    while j < bytes.len() && bytes[j].is_ascii_alphanumeric() {
        j += 1;
    }
    if j == start {
        return None;
    }
    // タグ名が区切りで終わっていること。`<p-custom>` のようなカスタム要素を `<p>` と
    // 取り違えない（`-` で止まった時点で区切りではないので弾ける）。
    if !bytes.get(j).map_or(false, |&c| is_tag_delim(c)) {
        return None;
    }
    let name = &bytes[start..j];
    if BLOCK_TAGS.iter().any(|t| t.eq_ignore_ascii_case(name)) {
        Some(j)
    } else {
        None
    }
}

/// iframe に配る html のブロック要素へ `data-src-line` を刻む。これが入ると iframe の
/// 中の要素が md の段落や `<li>` と同じ「ユニット」になり、コメントのデータ構造
/// （`file` + `startLine` + `endLine` + `quote`）を変えずに html へ広げられる。
///
/// なぜ属性か: 「N 番目の開始タグ = DOM の N 番目の要素」という対応表を別に持つ案は、
/// パーサが構造を作り替える（`<tbody>` の暗黙挿入、入れ子の修復）ので**表のあるページで
/// 静かにズレる**。属性なら要素に付いて回るし、JS が後から生成した DOM も最寄りの祖先の
/// 行へ落とせる。
///
/// バイト列で処理するので文字コードに依存しない（`inject_style_gate` と同じ筋）。
fn inject_src_lines(bytes: Vec<u8>) -> Vec<u8> {
    if bytes.len() > STAMP_MAX_BYTES {
        return bytes;
    }
    let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 16);
    let mut line = 1usize;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if b != b'<' {
            if b == b'\n' {
                line += 1;
            }
            out.push(b);
            i += 1;
            continue;
        }
        if let Some(end) = skip_opaque(&bytes, i) {
            line += bytes[i..end].iter().filter(|&&c| c == b'\n').count();
            out.extend_from_slice(&bytes[i..end]);
            i = end;
            continue;
        }
        // タグは丸ごと読み切ってから出す。中を 1 バイトずつ辿ると属性値の中の `<p>` まで
        // 拾ってしまう（`tag_end` の Why not を参照）。
        if !is_tag_start(&bytes, i) {
            out.push(b);
            i += 1;
            continue;
        }
        let end = tag_end(&bytes, i);
        match block_tag_end(&bytes, i) {
            // 属性は**タグ名の直後**へ入れる。末尾（`>` の手前）に置くと、閉じていない
            // 引用符や `/` の付いた自己終了タグで属性の外へ落ちることがある。
            Some(name_end) => {
                out.extend_from_slice(&bytes[i..name_end]);
                out.extend_from_slice(format!(r#" data-src-line="{line}""#).as_bytes());
                out.extend_from_slice(&bytes[name_end..end]);
            }
            None => out.extend_from_slice(&bytes[i..end]),
        }
        line += bytes[i..end].iter().filter(|&&c| c == b'\n').count();
        i = end;
    }
    out
}

/// カスタムプロトコルのハンドラが 1 リクエストを処理するのに必要なもの一式。
///
/// ハンドラは別スレッドで走る（`main.rs` 参照）ので、`Arc` で包んで共有できるよう
/// 参照ではなく所有した値で持つ。引数を 7 つ引き回していた頃と違い、配信に必要な
/// ものを足すときの変更がこの構造体の中だけで済む。
pub struct RequestContext {
    /// ツリーが見せる範囲の頂点。URL（`safe_join`）もここから出るものを拒否する。
    /// 識別子（`?file=`）は root の外も指せるので、これは配信の境界ではない。
    ///
    /// 実行中に動く（#34）。フォルダ移動はイベントループのスレッドから来て、
    /// リクエストは 1 本ずつ別スレッドで走るので、共有の可変にするしかない。
    root_dir: RwLock<PathBuf>,
    /// `/` で返す初期ページ。起動時に組み立て済みで、テーマの層だけが後から変わる
    /// （[`RequestContext::set_theme`]）。
    index_html: RwLock<Vec<u8>>,
    /// テーマの層（[`crate::theme::style_layer`]）。設定画面と `md theme` で変わる（#38）。
    theme_css: RwLock<String>,
    pub custom_css: String,
}

impl RequestContext {
    pub fn new(root_dir: PathBuf, index_html: Vec<u8>, theme_css: String, custom_css: String) -> Self {
        RequestContext {
            root_dir: RwLock::new(root_dir),
            index_html: RwLock::new(index_html),
            theme_css: RwLock::new(theme_css),
            custom_css,
        }
    }

    pub fn index_html(&self) -> Vec<u8> {
        read_lock(&self.index_html).clone()
    }

    pub fn theme_css(&self) -> String {
        read_lock(&self.theme_css).clone()
    }

    /// テーマの層を差し替える。開いているページへの反映は呼び出し側が別に送る
    /// （[`crate::html::theme_script`]）。ここが持つのは**この後に配信するもの**。
    ///
    /// 初期ページの中の層も書き換える。ページが読み込み直されることは普段無いが、
    /// WebContent プロセスが落ちたときに WKWebView が勝手に読み込み直すので、
    /// そこで起動時のテーマへ巻き戻らないようにする。
    pub fn set_theme(&self, css: String) {
        let mut theme = write_lock(&self.theme_css);
        let mut index = write_lock(&self.index_html);
        let page = String::from_utf8_lossy(&index);
        *index = crate::html::swap_theme_style(&page, &theme, &css).into_bytes();
        *theme = css;
    }

    /// いまの root。**借りるのではなく複製して返す。**
    ///
    /// 1 リクエストは `?files=1` のようにミリ秒では終わらないものを含むので、
    /// ガードを持ったまま処理へ入ると、その間フォルダ移動が固まる。`PathBuf` 1 つの
    /// 複製のほうが安い。
    ///
    /// 毒（poison）の枝は**いまは到達しない**——`set_root` は `PathBuf` を代入する
    /// だけで、代入も drop もパニックしないため。それでも `unwrap` にしないのは、
    /// ここが落ちると窓ごと消えるのに対し、root は「いまどこを見ているか」でしかなく、
    /// 古い値で 1 リクエスト返すほうが安いから。
    pub fn root(&self) -> PathBuf {
        read_lock(&self.root_dir).clone()
    }

    pub fn set_root(&self, root: PathBuf) {
        *write_lock(&self.root_dir) = root;
    }
}

/// 毒を無視して読む。理由は [`RequestContext::root`] の doc。テーマも同じで、
/// 古い層で 1 リクエスト返す方が窓ごと落ちるより安い。
fn read_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write_lock<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// URL とクエリが指す処理。
///
/// 以前はクエリ文字列を `strip_prefix` で順番に舐めており、`file=` が `files=1` を
/// 拾わないことや `diff=1` を `diff=<rel>` より先に見ることが、**判定の順序**によって
/// 保たれていた（そのためのテストまであった）。キーで厳密に分けることで、
/// エンドポイントを足しても前方一致の衝突が起きえなくなる。
///
/// 対象を持つルートが載せているのは識別子（常に絶対パス）。以前は 1 枚もの表示用の
/// 「引数なし番兵」（`?body=1` など）もあったが、起動モードがフォルダ 1 本に
/// なったので指し方も 1 通りになった。
#[derive(Debug, PartialEq, Eq)]
enum Route<'a> {
    BuiltinLib(&'a str),
    Dir(String),
    HasMd(String),
    Files,
    Changed,
    /// 通常表示。
    View(String),
    /// raw（ソース）表示。
    Raw(String),
    Diff(String),
    DiffStat(String),
    /// 起動時に組み立てた初期ページ。
    Index,
    /// 画像・CSS・フォントなどの実ファイル配信。
    Asset(&'a str),
}

fn parse_route<'a>(url_path: &'a str, query: &str) -> Route<'a> {
    if let Some(name) = url_path.strip_prefix("/__lib/") {
        return Route::BuiltinLib(name);
    }
    // クエリは常に「キー=値」1 組。値は percent-encode されて届く。
    if let Some((key, value)) = query.split_once('=') {
        match key {
            "dir" => return Route::Dir(percent_decode(value)),
            "has_md" => return Route::HasMd(percent_decode(value)),
            "files" => return Route::Files,
            "changed" => return Route::Changed,
            "file" => return Route::View(percent_decode(value)),
            "raw" => return Route::Raw(percent_decode(value)),
            "diff" => return Route::Diff(percent_decode(value)),
            "diffstat" => return Route::DiffStat(percent_decode(value)),
            _ => {}
        }
    }
    if url_path == "/" {
        return Route::Index;
    }
    Route::Asset(url_path)
}

/// 識別子を実ファイルへ解決する。**識別子とファイルの唯一の関門**で、
/// `?file=` / `?raw=` / `?diff=` / `?dir=` / メニューの IPC / 監視の追加登録が
/// 全部ここを通る。
///
/// 絶対パスでないものは弾く。root 相対を黙って受けると、root が動いたときに
/// 同じ文字列が別のファイルを指す（#33 が消したかった状態）。受けずに 404 にすれば、
/// 形の違う識別子を作った経路がその場で分かる。
///
/// `canonicalize` はここで済ませる。`/tmp` と `/private/tmp`、`$TMPDIR`、
/// symlink 経由のリポジトリが別物の識別子にならないのは、外から来たパスが
/// ここ（と `resolve_arg_path`）を通るため。
pub fn id_to_path(id: &str) -> Option<PathBuf> {
    if !id.starts_with('/') {
        return None;
    }
    PathBuf::from(id).canonicalize().ok()
}

/// 本文 HTML をプレビュー枠ごと差し替えるフラグメントにして返す。
/// プレビュー枠の中身は `.markdown-body` ラッパから丸ごと入れ替わるので、
/// 記事側のクラス（ソース表示の `source-page` など）もここで付ける。
fn respond_fragment(body_class: &str, html: String) -> Response {
    let class = if body_class.is_empty() {
        "markdown-body".to_string()
    } else {
        format!("markdown-body {}", body_class)
    };
    let body = format!(r#"<div class="{}">{}</div>"#, class, html);
    ok_response("text/html; charset=utf-8", body.into_bytes())
}

fn serve_view(ctx: &RequestContext, id: &str, mode: ViewMode) -> Response {
    let Some(path) = id_to_path(id) else { return not_found_response() };
    let Some(r) = render_file(&path, &ctx.root(), mode) else { return not_found_response() };
    respond_fragment(r.body_class, r.html)
}

/// diff はレンダリング結果ではなくソース差分なので、md / 非md を問わず全幅
/// （`source-page`）で出す。バイナリ・巨大ファイルは diff 側が中で弾く。
fn serve_diff(id: &str) -> Response {
    let Some(path) = id_to_path(id) else { return not_found_response() };
    respond_fragment("source-page", crate::diff::render_diff_inner(&path))
}

/// トグルボタンのバッジ用に、追加/削除行数だけを返す（軽量・非ブロッキング用途）。
fn serve_diffstat(id: &str) -> Response {
    let Some(path) = id_to_path(id) else { return not_found_response() };
    let (add, del) = crate::diff::diff_stat(&path);
    ok_response(
        "application/json; charset=utf-8",
        format!(r#"{{"add":{},"del":{}}}"#, add, del).into_bytes(),
    )
}

pub fn handle_request(ctx: &RequestContext, url_path: &str, query: &str) -> Response {
    match parse_route(url_path, query) {
        Route::BuiltinLib(name) => serve_builtin_lib(name),
        Route::Dir(id) => handle_dir(&id, &ctx.root()),
        Route::HasMd(id) => handle_has_md(&id, &ctx.root()),
        Route::Files => handle_files(&ctx.root()),
        Route::Changed => handle_changed(&ctx.root()),
        Route::View(id) => serve_view(ctx, &id, ViewMode::Normal),
        Route::Raw(id) => serve_view(ctx, &id, ViewMode::RawSource),
        Route::Diff(id) => serve_diff(&id),
        Route::DiffStat(id) => serve_diffstat(&id),
        Route::Index => ok_response("text/html; charset=utf-8", ctx.index_html()),
        Route::Asset(p) => handle_asset(p, &ctx.root(), &ctx.theme_css(), &ctx.custom_css),
    }
}

/// サイドバーのフォルダに「中に md がある」点を出すかの判定。
///
/// 返すのは `{"has_md":"yes"|"no"|"unknown"}`。**値は bool ではなく文字列**で、
/// JS では `"no"` も truthy になる。`if (data.has_md)` と書くと全フォルダに点が付く。
///
/// `handle_files` のように `reason` / `limit` は添えない。あちらは JS が数値入りの
/// 警告文を出すためだが、ここで JS がするのはクラスを足すか足さないかだけなので、
/// 渡しても表示するあてが無い。
fn handle_has_md(id: &str, root_dir: &Path) -> Response {
    match resolve_tree_dir(id, root_dir) {
        Some(dir) => {
            let md = match md_presence(&dir) {
                MdPresence::Yes => "yes",
                MdPresence::No => "no",
                MdPresence::Unknown => "unknown",
            };
            ok_response(
                "application/json; charset=utf-8",
                format!(r#"{{"has_md":"{}"}}"#, md).into_bytes(),
            )
        }
        None => not_found_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テーマを変えた後に配る初期ページと md の直開きが、新しいテーマの層を持つこと
    /// （#38）。フィールドを書き換えただけで、配る側が古い値を掴んでいる形を見逃さない。
    #[test]
    fn a_new_theme_is_served_from_then_on() {
        let dir = std::env::temp_dir().canonicalize().unwrap().join("md-theme-swap-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "# a").unwrap();

        let index = crate::html::build_folder_html("t", "THEME_OLD", "", &[]).into_bytes();
        let ctx = RequestContext::new(dir.clone(), index, "THEME_OLD".to_string(), String::new());
        let body = |path: &str| {
            String::from_utf8(handle_request(&ctx, path, "").body().to_vec()).unwrap()
        };

        ctx.set_theme("THEME_NEW".to_string());

        let page = body("/");
        assert!(page.contains("THEME_NEW") && !page.contains("THEME_OLD"), "初期ページ");
        let doc = body("/a.md");
        assert!(doc.contains("THEME_NEW") && !doc.contains("THEME_OLD"), "md の直開き");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn swapping_the_root_changes_what_the_tree_can_reach() {
        // #34。root は実行中に動くので、`RequestContext` を組んだ時点の値に
        // 引きずられないことを、配信の答えで確かめる（フィールドを読むだけだと
        // 「読む側が別の値を掴んでいる」形の壊れ方を見逃す）。
        let base = std::env::temp_dir().canonicalize().unwrap().join("md-root-swap-test");
        let inner = base.join("inner");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&inner).unwrap();

        let ctx = RequestContext::new(base.clone(), Vec::new(), String::new(), String::new());
        let ask = |ctx: &RequestContext, dir: &Path| {
            handle_request(ctx, "/", &format!("dir={}", dir.to_string_lossy())).status()
        };

        assert_eq!(ask(&ctx, &base), 200, "起動時の root は読める");
        assert_eq!(ask(&ctx, &inner), 200, "その配下も読める");

        ctx.set_root(inner.clone());

        assert_eq!(ask(&ctx, &inner), 200, "新しい root は読める");
        assert_eq!(ask(&ctx, &base), 404, "外に出た旧 root はもう読めない");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn every_response_forbids_disk_caching() {
        // 応答の生成口はこの 2 つだけ。どちらから出ても no-store が付く。
        let ok = ok_response("text/plain", b"hi".to_vec());
        assert_eq!(ok.headers().get("Cache-Control").unwrap(), "no-store");
        let nf = not_found_response();
        assert_eq!(nf.headers().get("Cache-Control").unwrap(), "no-store");
    }

    #[test]
    fn builtin_lib_served_for_known_names() {
        for name in ["mermaid.min.js", "drawio-viewer.min.js"] {
            let resp = serve_builtin_lib(name);
            assert_eq!(resp.status(), 200, "{name} should be 200");
            assert!(resp.body().len() > 100_000, "{name} body too small");
            assert!(resp.headers()["Content-Type"].to_str().unwrap().contains("javascript"));
        }
    }

    #[test]
    fn builtin_lib_404_for_unknown() {
        assert_eq!(serve_builtin_lib("../secret.js").status(), 404);
        assert_eq!(serve_builtin_lib("nope.js").status(), 404);
    }

    #[test]
    fn html_mime_is_text_html() {
        assert_eq!(guess_mime(Path::new("a.html")), "text/html; charset=utf-8");
        // 大文字拡張子も html として配信する（判定側と揃える。揃わないと iframe が空白になる）。
        assert_eq!(guess_mime(Path::new("a.HTM")), "text/html; charset=utf-8");
        assert_eq!(guess_mime(Path::new("a.HTML")), "text/html; charset=utf-8");
        assert!(guess_mime(Path::new("a.css")).starts_with("text/css"));
        assert!(guess_mime(Path::new("a.js")).starts_with("text/javascript"));
    }

    #[test]
    fn html_iframe_title_escapes_quotes() {
        // " 入りファイル名で title 属性から脱出できないこと（親ドキュメントへの属性注入防止）。
        let frag = render_html_iframe("/ev%22il.html", r#"ev"il.html"#);
        assert!(!frag.contains(r#"title="ev"il"#), "title のクォートが素通し: {frag}");
        assert!(frag.contains("&quot;"), "title がエスケープされていない: {frag}");
    }

    #[test]
    fn classify_ext_is_case_insensitive() {
        use ViewKind::*;
        for (name, want) in [
            ("a.md", Markdown),
            ("a.MARKDOWN", Markdown),
            ("a.html", HtmlPage),
            ("a.HTM", HtmlPage),
            ("a.txt", Source),
            ("a.rs", Source),
            ("noext", Source),
        ] {
            assert_eq!(classify_ext(Path::new(name)), want, "{name}");
        }
        // RENDERABLE_EXT（JS 側へ注入する一覧）と判定が一致していること。
        for ext in RENDERABLE_EXT {
            assert!(is_renderable(Path::new(&format!("a.{ext}"))), "{ext}");
        }
        assert!(!is_renderable(Path::new("a.txt")));
    }

    #[test]
    fn binary_is_source_page_on_every_path() {
        // 非 UTF-8 は経路によらず source-page 扱い（JS 側の raw 可否判定がブレないように）。
        let dir = std::env::temp_dir().join(format!("md-binary-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("blob.bin");
        std::fs::write(&file, [0xff, 0xfe, 0x00]).unwrap();

        for mode in [ViewMode::Normal, ViewMode::RawSource] {
            let r = render_file(&file, &dir, mode).unwrap();
            assert_eq!(r.kind, ViewKind::Binary);
            assert_eq!(r.body_class, "source-page");
            assert!(r.html.contains("md-notice"), "{}", r.html);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn html_iframe_fragment_has_encoded_src() {
        let root = Path::new("/proj");
        let frag = render_html_iframe(&asset_url(root, Path::new("/proj/sub dir/page.html")), "sub dir/page.html");
        assert!(frag.contains(r#"class="html-frame""#), "{frag}");
        // 空白は %20、`/` は温存されていること。
        assert!(frag.contains(r#"src="/sub%20dir/page.html""#), "{frag}");
        // 引用符を割らない（属性脱出しない）こと。
        assert!(!frag.contains(r#"src="/sub dir"#), "{frag}");
        // root の外は /__abs/ 配下に、実パスの階層のまま出る。
        let out = render_html_iframe(&asset_url(root, Path::new("/other/page.html")), "/other/page.html");
        assert!(out.contains(r#"src="/__abs/other/page.html""#), "{out}");
    }

    #[test]
    fn style_gate_inserted_before_head_close() {
        let out = inject_style_gate(b"<html><head><link rel=stylesheet></HEAD><body>x</body></html>".to_vec());
        let s = String::from_utf8(out).unwrap();
        // gate は </head>（大文字小文字問わず）直前に入る。
        assert!(s.contains("<script>/*md:style-gate*/void 0</script></HEAD>"), "{s}");
        // stylesheet より後に入る（順序が逆転しない）。
        assert!(s.find("stylesheet").unwrap() < s.find("md:style-gate").unwrap(), "{s}");
    }

    #[test]
    fn style_gate_noop_without_head() {
        let src = b"<div>no head here</div>".to_vec();
        assert_eq!(inject_style_gate(src.clone()), src);
    }

    #[test]
    fn style_gate_skips_script_and_comment_pseudo_head() {
        // head 内の script 文字列やコメントに現れる "</head>" は無視し、本物の直前へ入れる。
        let src = br#"<head><script>var s="</head>";</script><!-- </head> --><link></head><body>x</body>"#.to_vec();
        let out = String::from_utf8(inject_style_gate(src)).unwrap();
        // gate は1個だけ、しかも本物の </head>（<link> の後）直前に入る。
        assert_eq!(out.matches("md:style-gate").count(), 1, "{out}");
        assert!(out.contains("<link><script>/*md:style-gate*/void 0</script></head>"), "{out}");
        // script の中身は壊されない（元の script がそのまま残る）。
        assert!(out.contains(r#"<script>var s="</head>";</script>"#), "{out}");
    }

    #[test]
    fn style_gate_handles_non_utf8() {
        // 非 UTF-8 バイト（0xFF）が混じってもパニックせず、本物の </head> 直前へ入る。
        let mut src = b"<head>".to_vec();
        src.push(0xFF);
        src.extend_from_slice(b"</head><body>x</body>");
        let out = inject_style_gate(src);
        assert!(find_ci_ascii(&out, b"md:style-gate").is_some());
    }

    /// 刻まれた行番号を (タグ名, 行) の並びで拾う。順序が本文どおりであることも見る。
    fn stamps(out: &[u8]) -> Vec<(String, usize)> {
        let s = String::from_utf8_lossy(out).into_owned();
        let mut found = Vec::new();
        let mut rest = s.as_str();
        while let Some(pos) = rest.find(" data-src-line=\"") {
            let head = &rest[..pos];
            let tag = head.rsplit('<').next().unwrap_or("").to_string();
            let after = &rest[pos + " data-src-line=\"".len()..];
            let end = after.find('"').unwrap();
            found.push((tag, after[..end].parse().unwrap()));
            rest = &after[end..];
        }
        found
    }

    #[test]
    fn src_lines_stamp_block_tags_only() {
        let src = b"<html>\n<body>\n<p>a</p>\n<div><span>s</span><li>b</li></div>\n<h2>t</h2>\n</body>\n</html>".to_vec();
        let out = inject_src_lines(src);
        // div / span / html / body は対象外。ブロック要素だけが、本文の行番号で刻まれる。
        assert_eq!(stamps(&out), vec![
            ("p".to_string(), 3),
            ("li".to_string(), 4),
            ("h2".to_string(), 5),
        ]);
    }

    #[test]
    fn src_lines_stamp_table_rows_and_cells() {
        // 表は tr / td / th が錨になる（table 自体は掴む単位として大きすぎる）。
        let out = inject_src_lines(b"<table>\n<tr><th>h</th></tr>\n<tr><td>d</td></tr>\n</table>".to_vec());
        assert_eq!(stamps(&out), vec![
            ("tr".to_string(), 2),
            ("th".to_string(), 2),
            ("tr".to_string(), 3),
            ("td".to_string(), 3),
        ]);
    }

    #[test]
    fn src_lines_insert_before_existing_attributes() {
        let out = inject_src_lines(br#"<p class="a" id="b">x</p>"#.to_vec());
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s, r#"<p data-src-line="1" class="a" id="b">x</p>"#);
    }

    #[test]
    fn src_lines_match_whole_tag_name() {
        // `<pre>` を見て `<p` と早合点しない。カスタム要素も別物として扱う。
        let out = inject_src_lines(b"<pre>c</pre>\n<p-custom>x</p-custom>\n<paragraph>y</paragraph>".to_vec());
        assert_eq!(stamps(&out), vec![("pre".to_string(), 1)]);
    }

    #[test]
    fn src_lines_ignore_closing_tags() {
        let out = inject_src_lines(b"<p>a</p><p>b</p>".to_vec());
        // 開きタグ 2 つだけ。`</p>` には刻まない。
        assert_eq!(stamps(&out), vec![("p".to_string(), 1), ("p".to_string(), 1)]);
    }

    #[test]
    fn src_lines_uppercase_tags() {
        let out = inject_src_lines(b"<P CLASS=x>y</P>".to_vec());
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s, r#"<P data-src-line="1" CLASS=x>y</P>"#);
    }

    #[test]
    fn src_lines_skip_raw_text_and_comments() {
        // script / style / textarea / title の中身と HTML コメントに現れる `<p>` は
        // タグではない。刻むと本文に属性文字列が見えてしまう。
        let src = br#"<script>var s = "<p>x</p>";</script>
<style>/* <li>y</li> */</style>
<textarea><p>z</p></textarea>
<title><p>t</p></title>
<!-- <p>c</p> -->
<p>real</p>"#
            .to_vec();
        let out = inject_src_lines(src);
        assert_eq!(stamps(&out), vec![("p".to_string(), 6)]);
        // 素通しした中身は 1 バイトも変わっていない。
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains(r#"<script>var s = "<p>x</p>";</script>"#), "{s}");
        assert!(s.contains("<textarea><p>z</p></textarea>"), "{s}");
    }

    #[test]
    fn src_lines_count_lines_inside_skipped_regions() {
        // 素通しした区間の改行も数えないと、以降の行番号が全部ずれる。
        let out = inject_src_lines(b"<script>\n\n</script>\n<!--\n-->\n<p>x</p>".to_vec());
        assert_eq!(stamps(&out), vec![("p".to_string(), 6)]);
    }

    #[test]
    fn src_lines_unclosed_raw_text_swallows_rest() {
        // 閉じ忘れた script は末尾まで素通し（安全側）。後続に刻まない。
        let out = inject_src_lines(b"<script>\n<p>x</p>".to_vec());
        assert!(stamps(&out).is_empty());
    }

    #[test]
    fn src_lines_noop_for_huge_html() {
        let mut src = b"<p>x</p>".to_vec();
        src.resize(STAMP_MAX_BYTES + 1, b' ');
        assert_eq!(inject_src_lines(src.clone()), src);
    }

    #[test]
    fn src_lines_handle_non_utf8() {
        // 非 UTF-8 バイトが混じってもパニックせず、行は刻まれる。
        let mut src = b"<p>".to_vec();
        src.push(0xFF);
        src.extend_from_slice(b"</p>");
        let out = inject_src_lines(src);
        assert_eq!(find_ci_ascii(&out, br#"<p data-src-line="1">"#), Some(0));
        assert!(out.contains(&0xFF));
    }

    #[test]
    fn src_lines_do_not_read_inside_attribute_values() {
        // 属性値の中の `<li>` をタグと見なすと、差し込む `"` が引用値を閉じてタグごと壊れる。
        // Bootstrap の data-bs-content / Alpine・Vue のインラインテンプレートで実在する書き方。
        let out = inject_src_lines(br#"<td data-tpl="<li>x</li>">v</td>"#.to_vec());
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s, r#"<td data-src-line="1" data-tpl="<li>x</li>">v</td>"#);

        // 刻まない側のタグでも同じ（`<a>` の属性値が本文へ漏れない）。
        let out = inject_src_lines(br#"<a title="<p>">v</a>"#.to_vec());
        assert_eq!(String::from_utf8(out).unwrap(), r#"<a title="<p>">v</a>"#);

        // 単引用でも、引用の中の `>` でタグを閉じないことでも同じ。
        let out = inject_src_lines(br#"<p title='a>b'>x</p>"#.to_vec());
        assert_eq!(String::from_utf8(out).unwrap(), r#"<p data-src-line="1" title='a>b'>x</p>"#);
    }

    #[test]
    fn src_lines_keep_anchors_after_attribute_values_that_look_opaque() {
        // 属性値の中の `<script>` を本物と誤認すると、そこから本物の `</script>` までが
        // 素通しになって、間のブロック要素が刻まれないまま落ちる。
        let src = b"<div data-x=\"<script>\"></div>\n<p>one</p>\n<script>s</script>\n<p>two</p>".to_vec();
        assert_eq!(stamps(&inject_src_lines(src)), vec![
            ("p".to_string(), 2),
            ("p".to_string(), 4),
        ]);

        // `<!--` も同じ。閉じが無いので、誤認すると以降が丸ごと落ちる。
        let src = b"<a title=\"<!--\">x</a>\n<p>one</p>".to_vec();
        assert_eq!(stamps(&inject_src_lines(src)), vec![("p".to_string(), 2)]);
    }

    #[test]
    fn src_lines_accept_close_tags_with_trailing_space() {
        // ブラウザは終了タグ名の後の空白を許す。`</script>` という並びだけを探すと
        // 次の本物まで素通しになり、間の行が刻まれない。
        let src = b"<script>1</script >\n<p>a</p>\n<script>2</script>\n<p>b</p>".to_vec();
        assert_eq!(stamps(&inject_src_lines(src)), vec![
            ("p".to_string(), 2),
            ("p".to_string(), 4),
        ]);
    }

    #[test]
    fn src_lines_skip_text_that_shows_through() {
        // 中身がそのまま画面に出る要素は素通しする（textarea と同じ理由）。
        let out = inject_src_lines(b"<xmp><p>a</p></xmp>\n<p>b</p>".to_vec());
        assert_eq!(stamps(&out), vec![("p".to_string(), 2)]);
        // plaintext は閉じタグを持たない。以降すべてがプレーンテキストになる。
        let out = inject_src_lines(b"<plaintext><p>a</p>\n<p>b</p>".to_vec());
        assert!(stamps(&out).is_empty());
    }

    #[test]
    fn src_lines_win_over_an_existing_attribute() {
        // md-preview 自身の --html ダンプを .html として開くとこの入力になる。
        // 重複属性は先勝ちなので、こちらが差した値が採用される。
        let out = inject_src_lines(br#"<p data-src-line="42">a</p>"#.to_vec());
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s, r#"<p data-src-line="1" data-src-line="42">a</p>"#);
    }

    #[test]
    fn src_lines_and_style_gate_compose_either_way() {
        // 本番経路は 2 つを順に通す。差し込む文字列が互いのパターンを含まず、改行も
        // 足さないので順序に依存しない——依存し始めたらここが落ちる。
        let src = b"<html>\n<head><link></head>\n<body>\n<p>x</p>\n</body>".to_vec();
        let a = inject_style_gate(inject_src_lines(src.clone()));
        let b = inject_src_lines(inject_style_gate(src));
        assert_eq!(a, b);
        let s = String::from_utf8(a).unwrap();
        assert!(s.contains(r#"<p data-src-line="4">"#), "{s}");
        assert_eq!(s.matches("md:style-gate").count(), 1, "{s}");
    }

    #[test]
    fn src_lines_count_crlf_as_one_line() {
        let out = inject_src_lines(b"<p>a</p>\r\n<p>b</p>\r\n<p>c</p>".to_vec());
        assert_eq!(stamps(&out), vec![
            ("p".to_string(), 1),
            ("p".to_string(), 2),
            ("p".to_string(), 3),
        ]);
    }

    #[test]
    fn src_lines_keep_scanning_after_an_unquoted_apostrophe() {
        // 引用が始まるのは属性値の先頭だけ。タグの中のどの `'` でも引用扱いにすると、
        // 引用符無しの属性値に含まれるアポストロフィで次の `'` か EOF まで飲み込む。
        let out = inject_src_lines(b"<p title=Bob's>a</p>\n<p>b</p>\n<p>c</p>".to_vec());
        assert_eq!(stamps(&out), vec![
            ("p".to_string(), 1),
            ("p".to_string(), 2),
            ("p".to_string(), 3),
        ]);
    }

    #[test]
    fn src_lines_degrade_without_breaking_the_page() {
        // 走査が誤っても配信物を壊さないこと。差し込みは常にタグ名の直後なので、
        // 飲み込んだ区間はそのまま出る＝錨が減るだけで済む。この性質が崩れると、
        // 走査の穴が「刻み漏れ」ではなく「他人のページの破壊」になる。
        let src = br#"<p data-x="<b>" title='a>b' onclick="if(x<y){}">t</p>"#.to_vec();
        let out = inject_src_lines(src.clone());
        let s = String::from_utf8(out).unwrap();
        // 刻んだ属性を抜くと元のバイト列に戻る。
        assert_eq!(s.replace(r#" data-src-line="1""#, ""), String::from_utf8(src).unwrap());
    }

    #[test]
    fn style_gate_accepts_close_tags_with_trailing_space() {
        // gate 側の走査も `</script >` を終端と認める（行の錨と同じ規則で読む）。
        let src = b"<head><script>var s=\"</head>\";</script ><link></head><body>x</body>".to_vec();
        let out = String::from_utf8(inject_style_gate(src)).unwrap();
        assert_eq!(out.matches("md:style-gate").count(), 1, "{out}");
        assert!(out.contains("<link><script>/*md:style-gate*/void 0</script></head>"), "{out}");
    }

    #[test]
    fn style_gate_skips_pseudo_head_inside_title() {
        // `<title>` の中身はタグとして読まない。ここへ gate を入れるとタイトルが壊れる。
        let src = b"<head><title></head></title><link></head><body>x</body>".to_vec();
        let out = String::from_utf8(inject_style_gate(src)).unwrap();
        assert_eq!(out.matches("md:style-gate").count(), 1, "{out}");
        assert!(out.contains("<title></head></title>"), "{out}");
        assert!(out.contains("<link><script>/*md:style-gate*/void 0</script></head>"), "{out}");
    }

    #[test]
    fn src_lines_leave_bare_angle_brackets_alone() {
        // `a < b` の `<` はタグではない。読み飛ばすと後続のブロックまで落ちる。
        let out = inject_src_lines(b"a < b 5<6\n<p>x</p>".to_vec());
        let s = String::from_utf8(out).unwrap();
        assert_eq!(s, "a < b 5<6\n<p data-src-line=\"2\">x</p>");
    }

    #[test]
    fn find_ci_ascii_edges() {
        assert_eq!(find_ci_ascii(b"", b"x"), None);          // hay 空
        assert_eq!(find_ci_ascii(b"ab", b"abc"), None);      // needle > hay（アンダーフロー防止）
        assert_eq!(find_ci_ascii(b"abc", b""), None);        // needle 空
        assert_eq!(find_ci_ascii(b"aXaX", b"ax"), Some(0));  // 最初の一致・大小無視
    }

    #[test]
    fn md_fragment_embeds_file_link_relative_to_the_md_file() {
        // ?file= の配信経路でも、単独行ファイルリンクが
        // 「その md ファイルがある場所」基準で展開されること。
        let dir = std::env::temp_dir().join("md-req-embed-test/sub");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("code.txt"), "one\ntwo\nthree\n").unwrap();
        let md = dir.join("doc.md");
        std::fs::write(&md, "[code](./code.txt#L2)\n").unwrap();

        let r = render_file(&md, &dir, ViewMode::Normal).unwrap();
        assert_eq!(r.kind, ViewKind::Markdown);
        assert!(r.html.contains("code-embed"), "not embedded: {}", r.html);
        assert!(r.html.contains(">two</code>"), "wrong line: {}", r.html);
    }

    #[test]
    fn file_list_is_recursive_shallow_first_and_skips_heavy_dirs() {
        let root = std::env::temp_dir().join("md-filelist-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub/deep")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::create_dir_all(root.join(".git/objects")).unwrap();
        std::fs::create_dir_all(root.join(".github")).unwrap();
        std::fs::write(root.join("b.md"), "x").unwrap();
        std::fs::write(root.join("a.md"), "x").unwrap();
        std::fs::write(root.join("sub/c.txt"), "x").unwrap();
        std::fs::write(root.join("sub/deep/d.md"), "x").unwrap();
        std::fs::write(root.join("node_modules/pkg/index.js"), "x").unwrap();
        std::fs::write(root.join(".git/objects/blob"), "x").unwrap();
        std::fs::write(root.join(".github/ci.yml"), "x").unwrap();

        let mut out = Vec::new();
        assert_eq!(collect_files(&root, &mut out), None);

        // 一覧が返すのは識別子（絶対パス）。root を剥いで中身を見る。
        let ids: Vec<String> = out.iter().map(|p| display_id(&root, p)).collect();
        // 幅優先なので浅い階層が必ず先。各階層は名前順。
        assert_eq!(ids[0], "a.md");
        assert_eq!(ids[1], "b.md");
        // 深さ 1 のファイルは、深さ 2 のファイルより必ず前に来る。
        let i_shallow = ids.iter().position(|p| p == "sub/c.txt").unwrap();
        let i_deep = ids.iter().position(|p| p == "sub/deep/d.md").unwrap();
        assert!(i_shallow < i_deep, "{ids:?}");
        // 依存物・VCS 内部は除外、設定系の隠しディレクトリは残す。
        assert!(!ids.iter().any(|p| p.starts_with("node_modules/")), "{ids:?}");
        assert!(!ids.iter().any(|p| p.starts_with(".git/")), "{ids:?}");
        assert!(ids.contains(&".github/ci.yml".to_string()), "{ids:?}");
        // 入れ子も拾う。
        assert!(ids.contains(&"sub/c.txt".to_string()), "{ids:?}");
        assert!(ids.contains(&"sub/deep/d.md".to_string()), "{ids:?}");

        // レスポンスは {"files":[...],"truncated":false}。
        let resp = handle_files(&root);
        assert_eq!(resp.status(), 200);
        let body = String::from_utf8_lossy(resp.body()).into_owned();
        assert!(body.starts_with(r#"{"files":["#), "{body}");
        assert!(body.contains(r#""truncated":false"#), "{body}");
        assert!(body.contains(&json_string(&root.join("sub/deep/d.md").to_string_lossy())), "{body}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn file_list_reports_depth_truncation_with_matching_limit() {
        // 深さ上限より深い枝を刈ったら、黙って落とさず Depth として報告すること。
        // 警告文の数値は JSON の limit を使うので、理由と上限が対応していることも見る。
        let root = std::env::temp_dir().join("md-filelist-depth-test");
        let _ = std::fs::remove_dir_all(&root);
        let mut deep = root.clone();
        for i in 0..(FILE_LIST_MAX_DEPTH + 2) {
            deep = deep.join(format!("d{i}"));
        }
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("buried.md"), "x").unwrap();
        std::fs::write(root.join("top.md"), "x").unwrap();

        let mut out = Vec::new();
        assert_eq!(collect_files(&root, &mut out), Some(Truncation::Depth));
        assert!(out.contains(&file_id(&root.join("top.md"))), "{out:?}");
        assert!(!out.iter().any(|p| p.ends_with("buried.md")), "{out:?}");

        let body = String::from_utf8_lossy(handle_files(&root).body()).into_owned();
        assert!(body.contains(r#""truncated":true"#), "{body}");
        assert!(body.contains(r#""reason":"depth""#), "{body}");
        assert!(body.contains(&format!(r#""limit":{}"#, FILE_LIST_MAX_DEPTH)), "{body}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn has_md_finds_markdown_directly_below_and_nested() {
        let root = std::env::temp_dir().join("md-hasmd-yes-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("nested/sub/deep")).unwrap();
        std::fs::write(root.join("a.md"), "x").unwrap();
        // 拡張子は .markdown も Markdown 扱い（classify_ext と揃っていること）。
        std::fs::write(root.join("nested/sub/deep/d.markdown"), "x").unwrap();

        assert_eq!(md_presence(&root), MdPresence::Yes);
        assert_eq!(md_presence(&root.join("nested")), MdPresence::Yes);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn has_md_answers_no_for_a_tree_without_markdown() {
        // root は正規化済みであることが前提（`resolve_tree_dir` が識別子と突き合わせる）。
        let root = std::env::temp_dir().canonicalize().unwrap().join("md-hasmd-no-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub/deep")).unwrap();
        std::fs::write(root.join("note.txt"), "x").unwrap();
        std::fs::write(root.join("sub/deep/main.rs"), "x").unwrap();

        assert_eq!(md_presence(&root), MdPresence::No);

        let resp = handle_has_md(&file_id(&root), &root);
        assert_eq!(resp.status(), 200);
        let body = String::from_utf8_lossy(resp.body()).into_owned();
        assert!(body.contains(r#"{"has_md":"no"}"#), "{body}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn has_md_answers_unknown_when_the_depth_budget_cuts_the_branch() {
        let root = std::env::temp_dir().canonicalize().unwrap().join("md-hasmd-depth-test");
        let _ = std::fs::remove_dir_all(&root);
        let mut deep = root.clone();
        for _ in 0..(HAS_MD_MAX_DEPTH + 2) {
            deep = deep.join("d");
        }
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("buried.md"), "x").unwrap();

        assert_eq!(md_presence(&root), MdPresence::Unknown);

        let resp = handle_has_md(&file_id(&root), &root);
        let body = String::from_utf8_lossy(resp.body()).into_owned();
        assert!(body.contains(r#"{"has_md":"unknown"}"#), "{body}");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn has_md_still_reaches_the_deepest_allowed_level() {
        // 境界の内側。予算を締めすぎる回帰（`depth + 1 > MAX` を `depth >= MAX` に
        // するなど）は、超えた側のテストだけでは捕まらない。
        let root = std::env::temp_dir().join("md-hasmd-depth-edge-test");
        let _ = std::fs::remove_dir_all(&root);
        let mut deep = root.clone();
        for _ in 0..HAS_MD_MAX_DEPTH {
            deep = deep.join("d");
        }
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("edge.md"), "x").unwrap();

        assert_eq!(md_presence(&root), MdPresence::Yes);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn has_md_finishes_within_the_directory_budget() {
        // 境界の内側。予算ちょうどまでは Unknown にせず No と言い切ること。
        // 起点自身も 1 つと数えるので、サブディレクトリは上限より 1 つ少なくする。
        let root = std::env::temp_dir().join("md-hasmd-dirs-edge-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for i in 0..(HAS_MD_MAX_DIRS - 1) {
            std::fs::create_dir(root.join(format!("d{i:05}"))).unwrap();
        }

        assert_eq!(md_presence(&root), MdPresence::No);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn has_md_prefers_a_found_answer_over_a_cut_branch() {
        // 刈られた枝があっても、他の枝で見つかれば Yes。不明が答えを覆い隠さないこと。
        let root = std::env::temp_dir().join("md-hasmd-mixed-test");
        let _ = std::fs::remove_dir_all(&root);
        let mut deep = root.clone();
        for _ in 0..(HAS_MD_MAX_DEPTH + 2) {
            deep = deep.join("d");
        }
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("buried.md"), "x").unwrap();
        std::fs::write(root.join("top.md"), "x").unwrap();

        assert_eq!(md_presence(&root), MdPresence::Yes);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn has_md_answers_unknown_when_the_directory_budget_runs_out() {
        // 予算が実際に走査を止めること。深さ 1 に平らに並べるので、深さ予算は発火しない。
        let root = std::env::temp_dir().join("md-hasmd-dirs-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for i in 0..(HAS_MD_MAX_DIRS + 1) {
            std::fs::create_dir(root.join(format!("d{i:05}"))).unwrap();
        }

        assert_eq!(md_presence(&root), MdPresence::Unknown);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn has_md_ignores_markdown_inside_denylisted_dirs() {
        // 除外リストの中の md は数えない（⌘P の一覧と見え方を揃える）。
        // ただし聞かれた起点そのものは除外しない。
        let root = std::env::temp_dir().join("md-hasmd-deny-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::write(root.join("node_modules/pkg/readme.md"), "x").unwrap();

        assert_eq!(md_presence(&root), MdPresence::No);
        assert_eq!(md_presence(&root.join("node_modules")), MdPresence::Yes);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn has_md_does_not_follow_directory_symlinks() {
        // No が返ること。リンクを辿っていたら自己ループが深さ予算に当たって
        // Unknown になるので、この値が「辿っていない」を区別している。
        let root = std::env::temp_dir().join("md-hasmd-link-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("loop")).unwrap();
        std::os::unix::fs::symlink(root.join("loop"), root.join("loop/self")).unwrap();

        assert_eq!(md_presence(&root), MdPresence::No);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn has_md_rejects_paths_outside_the_root() {
        // root が正規化済みであることを前提にする（実アプリの root は resolve_arg_path が
        // canonicalize している）。macOS の temp_dir は /var/folders/... で
        // /private/var/... へのリンクなので、ここで揃える。
        let root = std::env::temp_dir().canonicalize().unwrap().join("md-hasmd-guard-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(root.parent().unwrap().join("md-hasmd-guard-outside")).unwrap();
        std::fs::write(root.join("a.md"), "x").unwrap();

        let id = |rel: &str| file_id(&root.join(rel));
        // root の外のディレクトリは、実在していてもツリーには出さない。
        let outside = file_id(&root.parent().unwrap().join("md-hasmd-guard-outside"));
        assert_eq!(handle_has_md(&outside, &root).status(), 404);
        // 識別子でないもの（root 相対）は関門を通らない。
        assert_eq!(handle_has_md("sub", &root).status(), 404);
        // ディレクトリでないものを聞かれても 404（?dir= と同じゲート）。
        assert_eq!(handle_has_md(&id("a.md"), &root).status(), 404);
        assert_eq!(handle_has_md(&file_id(&root), &root).status(), 200);
        assert_eq!(handle_has_md(&id("sub"), &root).status(), 200);

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(root.parent().unwrap().join("md-hasmd-guard-outside"));
    }

    #[test]
    fn file_list_does_not_report_denylisted_dirs_as_truncation() {
        // 除外リストは意図した除外なので、警告（打ち切り）扱いにしないこと。
        let root = std::env::temp_dir().join("md-filelist-deny-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::write(root.join("node_modules/pkg/index.js"), "x").unwrap();
        std::fs::write(root.join("a.md"), "x").unwrap();

        let mut out = Vec::new();
        assert_eq!(collect_files(&root, &mut out), None);
        assert_eq!(out, vec![file_id(&root.join("a.md"))]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn routes_are_keyed_not_prefix_matched() {
        // 似た名前のキーが互いを食わないこと（以前は strip_prefix の順序で守っていた）。
        assert_eq!(parse_route("/", "files=1"), Route::Files);
        assert_eq!(parse_route("/", "changed=1"), Route::Changed);
        assert_eq!(parse_route("/", "file=a.md"), Route::View("a.md".to_string()));
        assert_eq!(parse_route("/", "diffstat=a.md"), Route::DiffStat("a.md".to_string()));
        assert_eq!(parse_route("/", "has_md=sub"), Route::HasMd("sub".to_string()));
        // 値は percent-decode される。
        assert_eq!(
            parse_route("/", "file=sub%20dir%2Fa.md"),
            Route::View("sub dir/a.md".to_string())
        );
    }

    #[test]
    fn every_target_route_takes_an_identifier() {
        // 対象の指し方は識別子 1 通りだけ（番兵クエリ `?raw=1` はもう無い）。
        // parse_route は形を見ない。絶対パスかどうかを見るのは id_to_path。
        assert_eq!(parse_route("/", "raw=a.md"), Route::Raw("a.md".to_string()));
        assert_eq!(parse_route("/", "diff=/out/a.md"), Route::Diff("/out/a.md".to_string()));
        // かつて番兵だった "1" は、いまはそういう名前のファイル指定（無ければ 404）。
        assert_eq!(parse_route("/", "raw=1"), Route::Raw("1".to_string()));
        // `?body=1` は消えたので、知らないキーとしてアセット配信に落ちる。
        assert_eq!(parse_route("/x.png", "body=1"), Route::Asset("/x.png"));
    }

    #[test]
    fn non_query_routes() {
        assert_eq!(parse_route("/__lib/mermaid.min.js", ""), Route::BuiltinLib("mermaid.min.js"));
        assert_eq!(parse_route("/", ""), Route::Index);
        assert_eq!(parse_route("/img/a.png", ""), Route::Asset("/img/a.png"));
        // 知らないキーはアセット配信に落ちる（クエリ付きの画像 URL 等）。
        assert_eq!(parse_route("/img/a.png", "v=2"), Route::Asset("/img/a.png"));
    }

    #[test]
    fn fragments_always_carry_the_markdown_body_wrapper() {
        // プレビュー枠は丸ごと差し替わるので、ラッパはフラグメント側が必ず付ける。
        let body = String::from_utf8_lossy(respond_fragment("", "x".into()).body()).into_owned();
        assert_eq!(body, r#"<div class="markdown-body">x</div>"#);
        let src = String::from_utf8_lossy(respond_fragment("source-page", "x".into()).body()).into_owned();
        assert_eq!(src, r#"<div class="markdown-body source-page">x</div>"#);
    }

    #[test]
    fn changed_json_is_empty_array_outside_repo() {
        let root = std::env::temp_dir().join(format!("md-changed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.md"), "x").unwrap();

        let resp = handle_changed(&root);
        assert_eq!(resp.status(), 200);
        assert_eq!(String::from_utf8_lossy(resp.body()), r#"{"changed":[]}"#);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn html_is_rendered_as_iframe_without_reading_the_file() {
        // html は中身を読まずに iframe を返す（描くのは WebView 側）。存在しない
        // パスでも 200 になるのはそのため。
        let root = Path::new("/tmp/whatever");
        let r = render_file(Path::new("/tmp/whatever/foo.html"), root, ViewMode::Normal).unwrap();
        assert_eq!(r.kind, ViewKind::HtmlPage);
        assert_eq!(r.body_class, "html-page");
        assert!(r.html.contains(r#"class="html-frame""#), "{}", r.html);
        assert!(r.html.contains(r#"src="/foo.html""#), "{}", r.html);
        // raw トグル時はソース扱いなので、読めなければ None（404）。
        assert!(render_file(Path::new("/tmp/whatever/foo.html"), root, ViewMode::RawSource).is_none());
    }
}
