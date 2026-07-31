use std::borrow::Cow;
use std::path::{Path, PathBuf};

use crate::html::{attr_escape, html_escape, json_string, parse_frontmatter, render_body_in, render_frontmatter_html, render_full_document, DRAWIO_JS, MERMAID_JS};

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = from_hex(bytes[i + 1]);
            let lo = from_hex(bytes[i + 2]);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

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
        } else if is_md(&p) {
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

fn is_md(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("md") | Some("markdown")
    )
}

fn is_html(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("html") | Some("htm")
    )
}

/// root 相対パスを URL パスとして安全にエンコードする（`/` 区切りは残す）。
/// iframe の `src` に埋め込むため。空白・非ASCII・記号を percent-encode する。
fn encode_path(rel: &str) -> String {
    let mut out = String::with_capacity(rel.len());
    for b in rel.bytes() {
        match b {
            b'/' | b'-' | b'_' | b'.' | b'~'
            | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// html ファイルを iframe（ミニブラウザ）で描画するフラグメント（iframe 要素のみ）。
/// `rel` は root 相対パス。`src` を同一スキームのファイル URL にすることで、
/// iframe 内の相対リンク・画像・CSS が `mdpreview://localhost/<dir>/` 基準で解決される。
/// sandbox は付けない（ローカルファイル閲覧用途。JS 実行・同一オリジンを許可して忠実に描画）。
pub fn render_html_iframe(rel: &str) -> String {
    format!(
        r#"<iframe class="html-frame" src="/{src}" title="{title}" referrerpolicy="no-referrer"></iframe>"#,
        src = encode_path(rel),
        // title は属性値なのでクォートも潰す attr_escape を使う（html_escape は " を素通しし、
        // " 入りファイル名で親の信頼ドキュメントへ属性注入できてしまう）。
        title = attr_escape(rel),
    )
}

/// md ファイルを読み、frontmatter HTML と本文 HTML を組にして返す。読めなければ None。
/// `.markdown-body` ラッパを付けるかは呼び出し側で決める。
fn render_md_file(file_path: &Path) -> Option<(String, String)> {
    let content = std::fs::read_to_string(file_path).ok()?;
    let (fm_pairs, body) = parse_frontmatter(&content);
    // 単独行ファイルリンクの相対パスは、その md ファイルがある場所を基準に解決する。
    Some((
        render_frontmatter_html(&fm_pairs),
        render_body_in(body, file_path.parent()),
    ))
}

// フォルダモードのプレビュー枠用フラグメント。本文だけ `.markdown-body` で包む。
fn serve_md_fragment(file_path: &Path) -> Response {
    let Some((fm_html, body_html)) = render_md_file(file_path) else { return not_found_response() };
    let fragment = format!(r#"{}<div class="markdown-body">{}</div>"#, fm_html, body_html);
    ok_response("text/html; charset=utf-8", fragment.into_bytes())
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

fn serve_non_md_fragment(file_path: &Path) -> Response {
    let Ok(bytes) = std::fs::read(file_path) else { return not_found_response() };
    match String::from_utf8(bytes) {
        Ok(content) => {
            let fragment = format!(
                r#"<div class="markdown-body source-page">{}</div>"#,
                source_view_html(file_path, &content)
            );
            ok_response("text/html; charset=utf-8", fragment.into_bytes())
        }
        Err(_) => {
            let name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
            let fragment = format!(
                r#"<div class="markdown-body"><p class="binary-msg">バイナリファイルは表示できません: {}</p></div>"#,
                html_escape(name)
            );
            ok_response("text/html; charset=utf-8", fragment.into_bytes())
        }
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

fn handle_dir(rel_encoded: &str, root_dir: &Path) -> Response {
    let rel = percent_decode(rel_encoded);
    let target_dir = if rel.is_empty() {
        Some(root_dir.to_path_buf())
    } else {
        safe_join(root_dir, &rel)
    };
    match target_dir {
        Some(dir) if dir.is_dir() => ok_response(
            "application/json; charset=utf-8",
            list_dir_json(&dir, root_dir),
        ),
        _ => not_found_response(),
    }
}

fn handle_file(rel_encoded: &str, root_dir: &Path) -> Response {
    let rel = percent_decode(rel_encoded);
    let Some(file_path) = safe_join(root_dir, &rel) else { return not_found_response() };
    if is_md(&file_path) {
        serve_md_fragment(&file_path)
    } else if is_html(&file_path) {
        // html は通常表示をソースではなく iframe 描画にする（.md と同じ立ち位置）。
        let fragment = format!(
            r#"<div class="markdown-body html-page">{}</div>"#,
            render_html_iframe(&rel)
        );
        ok_response("text/html; charset=utf-8", fragment.into_bytes())
    } else {
        serve_non_md_fragment(&file_path)
    }
}

fn handle_asset(url_path: &str, root_dir: &Path, theme_css: &str, custom_css: &str) -> Response {
    let relative = url_path.strip_prefix('/').unwrap_or(url_path);
    let Some(file_path) = safe_join(root_dir, relative) else { return not_found_response() };
    if is_md(&file_path) {
        let Ok(content) = std::fs::read_to_string(&file_path) else { return not_found_response() };
        let file_title = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("Markdown Preview");
        // render_full_document を通すことで frontmatter も本来のページと同様に描画する。
        let rendered = render_full_document(&content, file_title, theme_css, custom_css, file_path.parent());
        ok_response("text/html; charset=utf-8", rendered.into_bytes())
    } else if is_html(&file_path) {
        // iframe に配信する html は、head 内 CSS が JS より先に適用されるよう
        // style-gate を注入してから返す（下記 inject_style_gate 参照）。
        let Ok(bytes) = std::fs::read(&file_path) else { return not_found_response() };
        ok_response("text/html; charset=utf-8", inject_style_gate(bytes))
    } else {
        let Ok(bytes) = std::fs::read(&file_path) else { return not_found_response() };
        ok_response(guess_mime(&file_path), bytes)
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

// 単一ファイルモードの body=1 用フラグメント。既存の .markdown-body 内に差し込むのでラッパ無し。
pub fn serve_single_file_body(file_path: &Path) -> Response {
    // html は iframe 描画。初期 article は html-page クラスを持つ（build_html 側）ので、
    // ここでは中身の iframe だけ返す。単一ファイルモードは root=親ディレクトリなので
    // rel はファイル名。
    if is_html(file_path) {
        let name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        return ok_response("text/html; charset=utf-8", render_html_iframe(name).into_bytes());
    }
    // その他の非 md は初回 build_html と同じソースビューを返す。これを md 描画にすると、
    // ホットリロードや diff/off の再取得（loadNormalBody）で全幅 Markdown に化ける。
    if !is_md(file_path) {
        return ok_response("text/html; charset=utf-8", render_raw_inner(file_path).into_bytes());
    }
    let Some((fm_html, body_html)) = render_md_file(file_path) else { return not_found_response() };
    let fragment = format!("{}{}", fm_html, body_html);
    ok_response("text/html; charset=utf-8", fragment.into_bytes())
}

// フォルダモードの diff 用フラグメント。プレビュー枠を丸ごと差し替えるので .markdown-body で包む。
// ソース差分なので md 以外のテキストファイルも対象にする（バイナリは中で弾く）。
fn serve_diff_fragment(rel_encoded: &str, root_dir: &Path) -> Response {
    let rel = percent_decode(rel_encoded);
    let Some(file_path) = safe_join(root_dir, &rel) else { return not_found_response() };
    let inner = crate::diff::render_diff_inner(&file_path);
    // diff はソース差分なので raw / 非mdソースビューと同じく全幅で出す（md/非md問わず）。
    let fragment = format!(r#"<div class="markdown-body source-page">{}</div>"#, inner);
    ok_response("text/html; charset=utf-8", fragment.into_bytes())
}

// 単一ファイルモードの diff 用フラグメント。既存の .markdown-body 内に差し込むのでラッパ無し。
fn serve_single_file_diff(file_path: &Path) -> Response {
    let inner = crate::diff::render_diff_inner(file_path);
    ok_response("text/html; charset=utf-8", inner.into_bytes())
}

// トグルボタンのバッジ用に、追加/削除行数だけを JSON で返す（軽量・非ブロッキング用途）。
fn diffstat_json(file_path: &Path) -> Response {
    let (add, del) = crate::diff::diff_stat(file_path);
    let body = format!(r#"{{"add":{},"del":{}}}"#, add, del).into_bytes();
    ok_response("application/json; charset=utf-8", body)
}

fn serve_diffstat(rel_encoded: &str, root_dir: &Path) -> Response {
    let rel = percent_decode(rel_encoded);
    let Some(file_path) = safe_join(root_dir, &rel) else { return not_found_response() };
    diffstat_json(&file_path)
}

/// ファイルのソースを、拡張子に応じた言語クラス付きの `<pre><code>` フラグメントにして
/// 返す。レンダリング結果ではなく生ソースなので、.md 以外のテキストファイルも対象。
/// 返すのは中身だけ（`.markdown-body` ラッパは呼び出し側で付ける）。ハイライトは
/// クライアント側の hljs が担当する。バイナリ / 読めない場合は通知メッセージを返す。
fn render_raw_inner(file_path: &Path) -> String {
    let Ok(bytes) = std::fs::read(file_path) else {
        return r#"<p class="diff-msg">ファイルを読み込めませんでした</p>"#.to_string();
    };
    match String::from_utf8(bytes) {
        Ok(content) => source_view_html(file_path, &content),
        Err(_) => {
            // バイナリ通知は他経路（serve_non_md_fragment / 単一ファイル / --html）と文言を揃える。
            let name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
            format!(
                r#"<p class="diff-msg">バイナリファイルは表示できません: {}</p>"#,
                html_escape(name)
            )
        }
    }
}

// フォルダモードの raw 用フラグメント。プレビュー枠を丸ごと差し替えるので .markdown-body で包む。
// raw はソース表示なので、非md のソースビューと同じく全幅（source-page）で出す。
fn serve_raw_fragment(rel_encoded: &str, root_dir: &Path) -> Response {
    let rel = percent_decode(rel_encoded);
    let Some(file_path) = safe_join(root_dir, &rel) else { return not_found_response() };
    let inner = render_raw_inner(&file_path);
    let fragment = format!(r#"<div class="markdown-body source-page">{}</div>"#, inner);
    ok_response("text/html; charset=utf-8", fragment.into_bytes())
}

// 単一ファイルモードの raw 用フラグメント。既存の .markdown-body 内に差し込むのでラッパ無し。
fn serve_single_file_raw(file_path: &Path) -> Response {
    let inner = render_raw_inner(file_path);
    ok_response("text/html; charset=utf-8", inner.into_bytes())
}

pub fn handle_request(
    url_path: &str,
    query: &str,
    root_dir: &Path,
    html_bytes: &[u8],
    theme_css: &str,
    custom_css: &str,
    single_file: Option<&Path>,
) -> Response {
    if let Some(name) = url_path.strip_prefix("/__lib/") {
        return serve_builtin_lib(name);
    }
    if let Some(rel_encoded) = query.strip_prefix("dir=") {
        return handle_dir(rel_encoded, root_dir);
    }
    // ファイル検索（⌘P）の一覧。`file=` の strip_prefix とは前方一致しない
    // （"files=1" は "file=" で始まらない）ので順序に依存しないが、dir= の隣に置く。
    if query == "files=1" {
        return handle_files(root_dir);
    }
    if let Some(rel_encoded) = query.strip_prefix("file=") {
        return handle_file(rel_encoded, root_dir);
    }
    if query == "body=1" {
        if let Some(path) = single_file {
            return serve_single_file_body(path);
        }
        return not_found_response();
    }
    // diff=1 は単一ファイルモードの番兵。folder モードの diff=<rel> より先に判定する
    // （strip_prefix("diff=") は "diff=1" も拾ってしまうため）。single_file が無い
    // （＝folder モード）なら番兵ではなく、"1" という名前のファイル指定として後続に流す。
    if query == "diff=1" {
        if let Some(path) = single_file {
            return serve_single_file_diff(path);
        }
    }
    if let Some(rel_encoded) = query.strip_prefix("diff=") {
        return serve_diff_fragment(rel_encoded, root_dir);
    }
    // diff と同じく diffstat=1 は単一ファイルの番兵、diffstat=<rel> は folder。
    if query == "diffstat=1" {
        if let Some(path) = single_file {
            return diffstat_json(path);
        }
    }
    if let Some(rel_encoded) = query.strip_prefix("diffstat=") {
        return serve_diffstat(rel_encoded, root_dir);
    }
    // diff と同じく raw=1 は単一ファイルモードの番兵、raw=<rel> は folder モード。
    if query == "raw=1" {
        if let Some(path) = single_file {
            return serve_single_file_raw(path);
        }
    }
    if let Some(rel_encoded) = query.strip_prefix("raw=") {
        return serve_raw_fragment(rel_encoded, root_dir);
    }
    if url_path == "/" {
        return ok_response("text/html; charset=utf-8", html_bytes.to_vec());
    }
    handle_asset(url_path, root_dir, theme_css, custom_css)
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
        let frag = render_html_iframe(r#"ev"il.html"#);
        assert!(!frag.contains(r#"title="ev"il"#), "title のクォートが素通し: {frag}");
        assert!(frag.contains("&quot;"), "title がエスケープされていない: {frag}");
    }

    #[test]
    fn is_html_detects_extensions() {
        assert!(is_html(Path::new("a.html")));
        assert!(is_html(Path::new("a.HTML")));
        assert!(is_html(Path::new("a.htm")));
        assert!(!is_html(Path::new("a.md")));
        assert!(!is_html(Path::new("a.txt")));
    }

    #[test]
    fn html_iframe_fragment_has_encoded_src() {
        let frag = render_html_iframe("sub dir/page.html");
        assert!(frag.contains(r#"class="html-frame""#), "{frag}");
        // 空白は %20、`/` は温存されていること。
        assert!(frag.contains(r#"src="/sub%20dir/page.html""#), "{frag}");
        // 引用符を割らない（属性脱出しない）こと。
        assert!(!frag.contains(r#"src="/sub dir"#), "{frag}");
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

        let resp = serve_md_fragment(&md);
        assert_eq!(resp.status(), 200);
        let body = String::from_utf8_lossy(resp.body());
        assert!(body.contains("code-embed"), "not embedded: {body}");
        assert!(body.contains(">two</code>"), "wrong line: {body}");
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
    fn file_list_query_is_not_shadowed_by_file_prefix() {
        // "files=1" が `file=` の前方一致に吸われないこと（吸われると一覧が404になる）。
        assert!("files=1".strip_prefix("file=").is_none());
    }

    #[test]
    fn single_file_body_html_returns_iframe() {
        let resp = serve_single_file_body(Path::new("/tmp/whatever/foo.html"));
        assert_eq!(resp.status(), 200);
        let body = String::from_utf8_lossy(resp.body());
        assert!(body.contains(r#"class="html-frame""#), "{body}");
        assert!(body.contains(r#"src="/foo.html""#), "{body}");
    }
}
