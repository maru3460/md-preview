use std::borrow::Cow;
use std::path::{Path, PathBuf};

use crate::html::{attr_escape, build_html, html_escape, json_string, parse_frontmatter, render_body_in, render_frontmatter_html, DRAWIO_JS, MERMAID_JS};
use crate::urlpath::{asset_url, DocBase, ABS_PREFIX};
pub use crate::urlpath::{file_id, percent_decode};


type Response = wry::http::Response<Cow<'static, [u8]>>;

pub fn ok_response(content_type: &str, body: Vec<u8>) -> Response {
    wry::http::Response::builder()
        .header("Content-Type", content_type)
        .body(Cow::Owned(body))
        .unwrap()
}

pub fn not_found_response() -> Response {
    wry::http::Response::builder()
        .status(404)
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

pub fn has_md_descendant(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    for entry in entries.flatten() {
        let p = entry.path();
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            if has_md_descendant(&p) {
                return true;
            }
        } else if classify_ext(&p) == ViewKind::Markdown {
            return true;
        }
    }
    false
}

pub fn list_dir_json(dir: &Path, root_dir: &Path) -> Vec<u8> {
    let mut dirs: Vec<(String, String)> = Vec::new();
    let mut files: Vec<(String, String)> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let entry_path = entry.path();
            let rel = entry_path
                .strip_prefix(root_dir)
                .unwrap_or(&entry_path)
                .to_string_lossy()
                .into_owned();

            if entry_path.is_dir() {
                dirs.push((name, rel));
            } else {
                files.push((name, rel));
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

/// 再帰探索から外すディレクトリ名。「読む対象ではないのに数万ファイルある」ものだけを
/// 挙げる（VCS の内部・依存物・ビルド生成物）。ここで外してもサイドバーのツリーは
/// 従来どおり全部見せるので、到達できなくなるファイルは無い。
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

/// root 以下のファイルを root 相対パスで `out` に集める。打ち切ったら理由を返す。
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

        let prefix = dir.strip_prefix(root).unwrap_or(Path::new("")).to_string_lossy().into_owned();
        for name in files {
            if out.len() >= FILE_LIST_MAX {
                return Some(Truncation::Files);
            }
            out.push(if prefix.is_empty() { name } else { format!("{}/{}", prefix, name) });
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

/// ファイル検索（⌘P）用に、root 以下の全ファイルを root 相対パスの JSON で返す。
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

pub fn safe_join(canonical_root: &Path, rel: &str) -> Option<PathBuf> {
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

/// ファイルを読んで本文 HTML を組む。**すべての表示経路（初期ページ / フォルダの
/// `?file=` / 単一ファイルの `?body=1` / `raw` / `--html` ダンプ）がここを通る。**
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
            html: render_html_iframe(&asset_url(root, path), &file_id(root, path)),
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

fn handle_dir(rel: &str, root_dir: &Path) -> Response {
    let target_dir = if rel.is_empty() {
        Some(root_dir.to_path_buf())
    } else {
        safe_join(root_dir, rel)
    };
    match target_dir {
        Some(dir) if dir.is_dir() => ok_response(
            "application/json; charset=utf-8",
            list_dir_json(&dir, root_dir),
        ),
        _ => not_found_response(),
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
        // iframe に配信する html は、head 内 CSS が JS より先に適用されるよう
        // style-gate を注入してから返す（下記 inject_style_gate 参照）。
        ViewKind::HtmlPage => {
            let Ok(bytes) = std::fs::read(&file_path) else { return not_found_response() };
            ok_response("text/html; charset=utf-8", inject_style_gate(bytes))
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

/// 本物の `</head>` の位置を返す。`<script>...</script>` とコメント `<!-- ... -->` の中身は
/// スキップするので、そこに現れる文字列 `</head>` を誤検出しない（誤検出して script 内へ
/// 注入すると、注入した `</script>` がページの script を途中で閉じて壊すため）。
/// 見つからなければ None。閉じられていない script/コメントがあれば安全側に None。
fn find_head_close(hay: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < hay.len() {
        if starts_ci_at(hay, i, b"<script") {
            let rel = find_ci_ascii(&hay[i..], b"</script>")?;
            i += rel + b"</script>".len();
        } else if starts_ci_at(hay, i, b"<!--") {
            let rel = find_ci_ascii(&hay[i..], b"-->")?;
            i += rel + b"-->".len();
        } else if starts_ci_at(hay, i, b"</head>") {
            return Some(i);
        } else {
            i += 1;
        }
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

/// カスタムプロトコルのハンドラが 1 リクエストを処理するのに必要なもの一式。
///
/// ハンドラは別スレッドで走る（`main.rs` 参照）ので、`Arc` で包んで共有できるよう
/// 参照ではなく所有した値で持つ。引数を 7 つ引き回していた頃と違い、配信に必要な
/// ものを足すときの変更がこの構造体の中だけで済む。
pub struct RequestContext {
    /// 配信を許可する範囲の頂点。`safe_join` はここから出るパスを拒否する。
    pub root_dir: PathBuf,
    /// `/` で返す初期ページ。起動時に組み立て済み。
    pub index_html: Vec<u8>,
    pub theme_css: String,
    pub custom_css: String,
    /// 単一ファイルモードで開いているファイル。フォルダモードでは None。
    /// `?body=1` などの「引数なし番兵」クエリの対象になる。
    pub single_file: Option<PathBuf>,
}

impl RequestContext {
    fn single(&self) -> Option<&Path> {
        self.single_file.as_deref()
    }
}

/// 操作対象のファイルの指し方。
///
/// 単一ファイルモードは開いているファイルが決まっているので `?raw=1` のように
/// 引数を取らない。フォルダモードは `?raw=<rel>` で相対パスを渡す。この 2 通りを
/// ここで吸収することで、配信側は「単一用」と「フォルダ用」を 2 本持たなくてよくなる。
#[derive(Debug, PartialEq, Eq)]
enum Target {
    /// 単一ファイルモードで開いているファイル。
    Single,
    /// root 相対パス（percent-decode 済み）。
    Rel(String),
}

impl Target {
    /// プレビュー枠を丸ごと差し替えるか（＝`.markdown-body` ラッパを付けるか）。
    /// 単一ファイルモードは既存の `.markdown-body` の中へ差し込むので付けない。
    fn wrapped(&self) -> bool {
        matches!(self, Target::Rel(_))
    }
}

/// URL とクエリが指す処理。
///
/// 以前はクエリ文字列を `strip_prefix` で順番に舐めており、`file=` が `files=1` を
/// 拾わないことや `diff=1` を `diff=<rel>` より先に見ることが、**判定の順序**によって
/// 保たれていた（そのためのテストまであった）。キーで厳密に分けることで、
/// エンドポイントを足しても前方一致の衝突が起きえなくなる。
#[derive(Debug, PartialEq, Eq)]
enum Route<'a> {
    BuiltinLib(&'a str),
    Dir(String),
    HasMd(String),
    Files,
    Changed,
    /// 通常表示（フォルダの `?file=` / 単一の `?body=1`）。
    View(Target),
    /// raw（ソース）表示。
    Raw(Target),
    Diff(Target),
    DiffStat(Target),
    /// 起動時に組み立てた初期ページ。
    Index,
    /// 画像・CSS・フォントなどの実ファイル配信。
    Asset(&'a str),
}

/// `?raw=1` 形式の「引数なし番兵」を解釈する。単一ファイルモードでは開いている
/// ファイルが対象。フォルダモードに番兵は無いので `"1"` という名前の相対パス指定
/// として扱う（従来どおり。実際には該当ファイルが無く 404 になる）。
fn sentinel_target(value: &str, has_single: bool) -> Target {
    if has_single && value == "1" {
        Target::Single
    } else {
        Target::Rel(percent_decode(value))
    }
}

fn parse_route<'a>(url_path: &'a str, query: &str, has_single: bool) -> Route<'a> {
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
            "file" => return Route::View(Target::Rel(percent_decode(value))),
            "body" => return Route::View(Target::Single),
            "raw" => return Route::Raw(sentinel_target(value, has_single)),
            "diff" => return Route::Diff(sentinel_target(value, has_single)),
            "diffstat" => return Route::DiffStat(sentinel_target(value, has_single)),
            _ => {}
        }
    }
    if url_path == "/" {
        return Route::Index;
    }
    Route::Asset(url_path)
}

/// `?file=` などの識別子を実ファイルへ解決する。root 相対の識別子は `safe_join` で
/// root 内に限定し、絶対パス（先頭 `/`）の識別子は root の外でも開く。
/// 開くのはこの明示ルート（`?file=` / `?raw=` / `?diff=`）と `/__abs/` 配下だけで、
/// サイドバーのツリー（`?dir=` / `?has_md=`）は root 内に留める。
pub fn id_to_path(root: &Path, id: &str) -> Option<PathBuf> {
    if id.starts_with('/') {
        PathBuf::from(id).canonicalize().ok()
    } else {
        safe_join(root, id)
    }
}

/// 対象を実パスへ解決する。開けないもの・単一ファイルモードでないのに `Single` を
/// 指すものは None。
fn resolve(target: &Target, ctx: &RequestContext) -> Option<PathBuf> {
    match target {
        Target::Single => ctx.single().map(|p| p.to_path_buf()),
        Target::Rel(id) => id_to_path(&ctx.root_dir, id),
    }
}

/// 本文 HTML を、対象に応じてラッパ有り / 無しで返す。
fn respond_fragment(target: &Target, body_class: &str, html: String) -> Response {
    let body = if target.wrapped() {
        let class = if body_class.is_empty() {
            "markdown-body".to_string()
        } else {
            format!("markdown-body {}", body_class)
        };
        format!(r#"<div class="{}">{}</div>"#, class, html)
    } else {
        html
    };
    ok_response("text/html; charset=utf-8", body.into_bytes())
}

fn serve_view(ctx: &RequestContext, target: &Target, mode: ViewMode) -> Response {
    let Some(path) = resolve(target, ctx) else { return not_found_response() };
    let Some(r) = render_file(&path, &ctx.root_dir, mode) else { return not_found_response() };
    respond_fragment(target, r.body_class, r.html)
}

/// diff はレンダリング結果ではなくソース差分なので、md / 非md を問わず全幅
/// （`source-page`）で出す。バイナリ・巨大ファイルは diff 側が中で弾く。
fn serve_diff(ctx: &RequestContext, target: &Target) -> Response {
    let Some(path) = resolve(target, ctx) else { return not_found_response() };
    respond_fragment(target, "source-page", crate::diff::render_diff_inner(&path))
}

/// トグルボタンのバッジ用に、追加/削除行数だけを返す（軽量・非ブロッキング用途）。
fn serve_diffstat(ctx: &RequestContext, target: &Target) -> Response {
    let Some(path) = resolve(target, ctx) else { return not_found_response() };
    let (add, del) = crate::diff::diff_stat(&path);
    ok_response(
        "application/json; charset=utf-8",
        format!(r#"{{"add":{},"del":{}}}"#, add, del).into_bytes(),
    )
}

pub fn handle_request(ctx: &RequestContext, url_path: &str, query: &str) -> Response {
    match parse_route(url_path, query, ctx.single().is_some()) {
        Route::BuiltinLib(name) => serve_builtin_lib(name),
        Route::Dir(rel) => handle_dir(&rel, &ctx.root_dir),
        // サイドバーの「md を含むフォルダ」の点。深さ無制限の全走査になりうるが、
        // ハンドラ自体が別スレッドで走るのでここで完結してよい。
        Route::HasMd(rel) => handle_has_md(&rel, &ctx.root_dir),
        Route::Files => handle_files(&ctx.root_dir),
        Route::Changed => handle_changed(&ctx.root_dir),
        Route::View(t) => serve_view(ctx, &t, ViewMode::Normal),
        Route::Raw(t) => serve_view(ctx, &t, ViewMode::RawSource),
        Route::Diff(t) => serve_diff(ctx, &t),
        Route::DiffStat(t) => serve_diffstat(ctx, &t),
        Route::Index => ok_response("text/html; charset=utf-8", ctx.index_html.clone()),
        Route::Asset(p) => handle_asset(p, &ctx.root_dir, &ctx.theme_css, &ctx.custom_css),
    }
}

/// サイドバーのフォルダに「中に md がある」点を出すかの判定。
fn handle_has_md(rel: &str, root_dir: &Path) -> Response {
    let found = safe_join(root_dir, rel).map(|p| has_md_descendant(&p)).unwrap_or(false);
    ok_response(
        "application/json; charset=utf-8",
        format!(r#"{{"has_md":{}}}"#, found).into_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn find_ci_ascii_edges() {
        assert_eq!(find_ci_ascii(b"", b"x"), None);          // hay 空
        assert_eq!(find_ci_ascii(b"ab", b"abc"), None);      // needle > hay（アンダーフロー防止）
        assert_eq!(find_ci_ascii(b"abc", b""), None);        // needle 空
        assert_eq!(find_ci_ascii(b"aXaX", b"ax"), Some(0));  // 最初の一致・大小無視
    }

    #[test]
    fn md_fragment_embeds_file_link_relative_to_the_md_file() {
        // フォルダモードの ?file= 経路でも、単独行ファイルリンクが
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

        // 幅優先なので浅い階層が必ず先。各階層は名前順。
        assert_eq!(out[0], "a.md");
        assert_eq!(out[1], "b.md");
        // 深さ 1 のファイルは、深さ 2 のファイルより必ず前に来る。
        let i_shallow = out.iter().position(|p| p == "sub/c.txt").unwrap();
        let i_deep = out.iter().position(|p| p == "sub/deep/d.md").unwrap();
        assert!(i_shallow < i_deep, "{out:?}");
        // 依存物・VCS 内部は除外、設定系の隠しディレクトリは残す。
        assert!(!out.iter().any(|p| p.starts_with("node_modules/")), "{out:?}");
        assert!(!out.iter().any(|p| p.starts_with(".git/")), "{out:?}");
        assert!(out.contains(&".github/ci.yml".to_string()), "{out:?}");
        // 入れ子も root 相対パスで拾う。
        assert!(out.contains(&"sub/c.txt".to_string()), "{out:?}");
        assert!(out.contains(&"sub/deep/d.md".to_string()), "{out:?}");

        // レスポンスは {"files":[...],"truncated":false}。
        let resp = handle_files(&root);
        assert_eq!(resp.status(), 200);
        let body = String::from_utf8_lossy(resp.body()).into_owned();
        assert!(body.starts_with(r#"{"files":["#), "{body}");
        assert!(body.contains(r#""truncated":false"#), "{body}");
        assert!(body.contains(r#""sub/deep/d.md""#), "{body}");

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
        assert!(out.contains(&"top.md".to_string()), "{out:?}");
        assert!(!out.iter().any(|p| p.ends_with("buried.md")), "{out:?}");

        let body = String::from_utf8_lossy(handle_files(&root).body()).into_owned();
        assert!(body.contains(r#""truncated":true"#), "{body}");
        assert!(body.contains(r#""reason":"depth""#), "{body}");
        assert!(body.contains(&format!(r#""limit":{}"#, FILE_LIST_MAX_DEPTH)), "{body}");

        let _ = std::fs::remove_dir_all(&root);
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
        assert_eq!(out, vec!["a.md".to_string()]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn routes_are_keyed_not_prefix_matched() {
        // 似た名前のキーが互いを食わないこと（以前は strip_prefix の順序で守っていた）。
        assert_eq!(parse_route("/", "files=1", false), Route::Files);
        assert_eq!(parse_route("/", "changed=1", false), Route::Changed);
        assert_eq!(
            parse_route("/", "file=a.md", false),
            Route::View(Target::Rel("a.md".to_string()))
        );
        assert_eq!(parse_route("/", "diffstat=1", true), Route::DiffStat(Target::Single));
        // 値は percent-decode される。
        assert_eq!(
            parse_route("/", "file=sub%20dir%2Fa.md", false),
            Route::View(Target::Rel("sub dir/a.md".to_string()))
        );
    }

    #[test]
    fn sentinel_only_applies_in_single_file_mode() {
        // 単一ファイルモードの `=1` は「開いているファイル」。
        assert_eq!(parse_route("/", "raw=1", true), Route::Raw(Target::Single));
        assert_eq!(parse_route("/", "diff=1", true), Route::Diff(Target::Single));
        // フォルダモードには番兵が無いので "1" という名前の相対パス指定になる。
        assert_eq!(parse_route("/", "raw=1", false), Route::Raw(Target::Rel("1".to_string())));
        assert_eq!(parse_route("/", "diff=1", false), Route::Diff(Target::Rel("1".to_string())));
        // body=1 は常に単一ファイル向け（フォルダモードでは resolve が失敗して 404）。
        assert_eq!(parse_route("/", "body=1", false), Route::View(Target::Single));
    }

    #[test]
    fn non_query_routes() {
        assert_eq!(parse_route("/__lib/mermaid.min.js", "", false), Route::BuiltinLib("mermaid.min.js"));
        assert_eq!(parse_route("/", "", false), Route::Index);
        assert_eq!(parse_route("/img/a.png", "", false), Route::Asset("/img/a.png"));
        // 知らないキーはアセット配信に落ちる（クエリ付きの画像 URL 等）。
        assert_eq!(parse_route("/img/a.png", "v=2", false), Route::Asset("/img/a.png"));
    }

    #[test]
    fn single_target_is_not_wrapped_but_rel_is() {
        // 単一ファイルは既存の .markdown-body の中へ差し込むのでラッパ無し、
        // フォルダはプレビュー枠を丸ごと差し替えるのでラッパ有り。
        assert!(!Target::Single.wrapped());
        assert!(Target::Rel("a.md".to_string()).wrapped());
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
