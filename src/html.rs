use pulldown_cmark::{html, Options, Parser};

pub const CSS: &str = include_str!("style.css");
pub const HLJS_JS: &str = include_str!("highlight.min.js");
pub const HLJS_LIGHT_CSS: &str = include_str!("hljs-light.min.css");
pub const HLJS_DARK_CSS: &str = include_str!("hljs-dark.min.css");

pub const INIT_JS: &str = include_str!("init.js");

pub const MD_OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_FOOTNOTES);

pub const FOLDER_JS: &str = include_str!("folder.js");

pub fn render_body(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, MD_OPTIONS);
    let mut body = String::new();
    html::push_html(&mut body, parser);
    body
}

pub fn build_html(body: &str, title: &str, custom_css: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>{css}</style>
<style>{hljs_light}</style>
<style>@media(prefers-color-scheme:dark){{{hljs_dark}}}</style>
<style>{custom_css}</style>
<script>{hljs_js}</script>
</head>
<body>
<article class="markdown-body">
{body}
</article>
</body>
</html>"#,
        title = title,
        css = CSS,
        hljs_light = HLJS_LIGHT_CSS,
        hljs_dark = HLJS_DARK_CSS,
        custom_css = custom_css,
        hljs_js = HLJS_JS,
        body = body,
    )
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn parse_frontmatter(s: &str) -> (Vec<(String, String)>, &str) {
    let after_open = if s.starts_with("---\r\n") {
        &s[5..]
    } else if s.starts_with("---\n") {
        &s[4..]
    } else {
        return (Vec::new(), s);
    };

    let close_pos = after_open.find("\n---\r\n")
        .map(|i| (i, i + 6))
        .or_else(|| after_open.find("\n---\n").map(|i| (i, i + 5)))
        .or_else(|| {
            after_open.strip_suffix("\n---").map(|_| {
                let i = after_open.len() - 4;
                (i, after_open.len())
            })
        });

    let (fm_end, body_start) = match close_pos {
        Some(v) => v,
        None => return (Vec::new(), s),
    };

    let fm_content = &after_open[..fm_end];
    let body = &after_open[body_start..];

    let pairs: Vec<(String, String)> = fm_content
        .lines()
        .filter_map(|line| {
            let colon = line.find(':')?;
            let key = line[..colon].trim().to_string();
            let val = line[colon + 1..].trim().to_string();
            if key.is_empty() { None } else { Some((key, val)) }
        })
        .collect();

    (pairs, body)
}

pub fn render_frontmatter_html(pairs: &[(String, String)]) -> String {
    if pairs.is_empty() {
        return String::new();
    }
    let mut rows = String::new();
    for (k, v) in pairs {
        rows.push_str(&format!(
            r#"<div class="fm-row"><span class="fm-key">{}</span><span class="fm-val">{}</span></div>"#,
            html_escape(k),
            html_escape(v)
        ));
    }
    format!(r#"<div class="frontmatter">{}</div>"#, rows)
}

pub fn build_folder_html(title: &str, custom_css: &str, initial_file: Option<&str>) -> String {
    let initial_file_script = format!(
        "<script>var INITIAL_FILE = {};</script>",
        serde_json::to_string(&initial_file).unwrap_or_else(|_| "null".to_string())
    );
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>{css}</style>
<style>{hljs_light}</style>
<style>@media(prefers-color-scheme:dark){{{hljs_dark}}}</style>
<style>{custom_css}</style>
<script>{hljs_js}</script>
{initial_file_script}
</head>
<body class="folder-mode">
<div class="folder-layout">
  <div id="sidebar"></div>
  <div id="resizer"></div>
  <div id="preview-pane"><div class="markdown-body"></div></div>
</div>
</body>
</html>"#,
        title = title,
        css = CSS,
        hljs_light = HLJS_LIGHT_CSS,
        hljs_dark = HLJS_DARK_CSS,
        custom_css = custom_css,
        hljs_js = HLJS_JS,
        initial_file_script = initial_file_script,
    )
}
