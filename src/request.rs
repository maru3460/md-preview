use std::borrow::Cow;
use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;

use crate::html::{build_html, html_escape, parse_frontmatter, render_body, render_frontmatter_html};

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
    match path.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

pub fn extension_to_hljs_lang(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
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
        } else if p.extension().and_then(|e| e.to_str()) == Some("md") {
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

    let mut items: Vec<serde_json::Value> = Vec::new();
    for (name, path) in dirs {
        items.push(serde_json::json!({ "name": name, "path": path, "kind": "dir" }));
    }
    for (name, path) in files {
        items.push(serde_json::json!({ "name": name, "path": path, "kind": "file" }));
    }

    serde_json::to_vec(&items).unwrap_or_else(|_| b"[]".to_vec())
}

pub fn safe_join(canonical_root: &Path, rel: &str) -> Option<PathBuf> {
    if rel.contains("..") {
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

fn decode(encoded: &str) -> String {
    percent_decode_str(encoded).decode_utf8_lossy().into_owned()
}

fn is_md(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("md")
}

fn serve_md_fragment(file_path: &Path) -> Response {
    let Ok(content) = std::fs::read_to_string(file_path) else { return not_found_response() };
    let (fm_pairs, body) = parse_frontmatter(&content);
    let fm_html = render_frontmatter_html(&fm_pairs);
    let fragment = format!(
        r#"{}<div class="markdown-body">{}</div>"#,
        fm_html,
        render_body(body)
    );
    ok_response("text/html; charset=utf-8", fragment.into_bytes())
}

fn serve_non_md_fragment(file_path: &Path) -> Response {
    let Ok(bytes) = std::fs::read(file_path) else { return not_found_response() };
    match String::from_utf8(bytes) {
        Ok(content) => {
            let lang = extension_to_hljs_lang(file_path);
            let fragment = format!(
                r#"<div class="markdown-body"><pre><code class="language-{}">{}</code></pre></div>"#,
                lang,
                html_escape(&content)
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

fn handle_dir(rel_encoded: &str, root_dir: &Path) -> Response {
    let rel = decode(rel_encoded);
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
    let rel = decode(rel_encoded);
    let Some(file_path) = safe_join(root_dir, &rel) else { return not_found_response() };
    if is_md(&file_path) {
        serve_md_fragment(&file_path)
    } else {
        serve_non_md_fragment(&file_path)
    }
}

fn handle_asset(url_path: &str, root_dir: &Path, custom_css: &str) -> Response {
    let relative = url_path.strip_prefix('/').unwrap_or(url_path);
    let file_path = root_dir.join(relative);
    if is_md(&file_path) {
        let Ok(content) = std::fs::read_to_string(&file_path) else { return not_found_response() };
        let file_title = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("Markdown Preview");
        let rendered = build_html(&render_body(&content), file_title, custom_css);
        ok_response("text/html; charset=utf-8", rendered.into_bytes())
    } else {
        let Ok(bytes) = std::fs::read(&file_path) else { return not_found_response() };
        ok_response(guess_mime(&file_path), bytes)
    }
}

pub fn handle_request(
    url_path: &str,
    query: &str,
    root_dir: &Path,
    html_bytes: &[u8],
    custom_css: &str,
) -> Response {
    if let Some(rel_encoded) = query.strip_prefix("dir=") {
        return handle_dir(rel_encoded, root_dir);
    }
    if let Some(rel_encoded) = query.strip_prefix("file=") {
        return handle_file(rel_encoded, root_dir);
    }
    if url_path == "/" {
        return ok_response("text/html; charset=utf-8", html_bytes.to_vec());
    }
    handle_asset(url_path, root_dir, custom_css)
}
