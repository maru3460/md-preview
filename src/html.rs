use pulldown_cmark::{html, BlockQuoteKind, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub const BASE_CSS: &str = include_str!("base.css");
pub const HLJS_JS: &str = include_str!("highlight.min.js");
pub const MERMAID_JS: &str = include_str!("mermaid.min.js");
pub const DRAWIO_JS: &str = include_str!("drawio-viewer.min.js");

pub const INIT_JS: &str = include_str!("init.js");
pub const SEARCH_JS: &str = include_str!("search.js");
pub const TOC_JS: &str = include_str!("toc.js");
pub const COMMON_JS: &str = include_str!("common.js");

pub const MD_OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_FOOTNOTES)
    .union(Options::ENABLE_GFM);

pub const FOLDER_JS: &str = include_str!("folder.js");

const ICON_NOTE: &str = r##"<svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor" aria-hidden="true"><path d="M0 8a8 8 0 1 1 16 0A8 8 0 0 1 0 8Zm8-6.5a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM6.5 7.75A.75.75 0 0 1 7.25 7h1a.75.75 0 0 1 .75.75v2.75h.25a.75.75 0 0 1 0 1.5h-2a.75.75 0 0 1 0-1.5h.25v-2h-.25a.75.75 0 0 1-.75-.75ZM8 6a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z"/></svg>"##;
const ICON_TIP: &str = r##"<svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor" aria-hidden="true"><path d="M8 1.5c-2.363 0-4 1.69-4 3.75 0 .984.424 1.625.984 2.304l.214.253c.223.264.47.556.673.848.284.411.537.896.621 1.49a.75.75 0 0 1-1.484.211c-.04-.282-.163-.547-.37-.847a8.456 8.456 0 0 0-.542-.68c-.084-.1-.173-.205-.268-.32C3.201 7.75 2.5 6.766 2.5 5.25 2.5 2.31 4.863 0 8 0s5.5 2.31 5.5 5.25c0 1.516-.701 2.5-1.328 3.259-.095.115-.184.22-.268.319-.207.245-.383.453-.541.681-.208.3-.33.565-.37.847a.751.751 0 0 1-1.485-.212c.084-.593.337-1.078.621-1.489.203-.292.45-.584.673-.848.075-.088.147-.173.213-.253.561-.679.985-1.32.985-2.304 0-2.06-1.637-3.75-4-3.75ZM5.75 12h4.5a.75.75 0 0 1 0 1.5h-4.5a.75.75 0 0 1 0-1.5ZM6 15.25a.75.75 0 0 1 .75-.75h2.5a.75.75 0 0 1 0 1.5h-2.5a.75.75 0 0 1-.75-.75Z"/></svg>"##;
const ICON_IMPORTANT: &str = r##"<svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor" aria-hidden="true"><path d="M0 1.75C0 .784.784 0 1.75 0h12.5C15.216 0 16 .784 16 1.75v9.5A1.75 1.75 0 0 1 14.25 13H8.06l-2.573 2.573A1.458 1.458 0 0 1 3 14.543V13H1.75A1.75 1.75 0 0 1 0 11.25Zm1.75-.25a.25.25 0 0 0-.25.25v9.5c0 .138.112.25.25.25h2a.75.75 0 0 1 .75.75v2.19l2.72-2.72a.749.749 0 0 1 .53-.22h6.5a.25.25 0 0 0 .25-.25v-9.5a.25.25 0 0 0-.25-.25Zm7 2.25v2.5a.75.75 0 0 1-1.5 0v-2.5a.75.75 0 0 1 1.5 0ZM9 9a1 1 0 1 1-2 0 1 1 0 0 1 2 0Z"/></svg>"##;
const ICON_WARNING: &str = r##"<svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor" aria-hidden="true"><path d="M6.457 1.047c.659-1.234 2.427-1.234 3.086 0l6.082 11.378A1.75 1.75 0 0 1 14.082 15H1.918a1.75 1.75 0 0 1-1.543-2.575Zm1.763.707a.25.25 0 0 0-.44 0L1.698 13.132a.25.25 0 0 0 .22.368h12.164a.25.25 0 0 0 .22-.368Zm.53 3.996v2.5a.75.75 0 0 1-1.5 0v-2.5a.75.75 0 0 1 1.5 0ZM9 11a1 1 0 1 1-2 0 1 1 0 0 1 2 0Z"/></svg>"##;
const ICON_CAUTION: &str = r##"<svg viewBox="0 0 16 16" width="16" height="16" fill="currentColor" aria-hidden="true"><path d="M4.47.22A.749.749 0 0 1 5 0h6c.199 0 .389.079.53.22l4.25 4.25c.141.14.22.331.22.53v6a.749.749 0 0 1-.22.53l-4.25 4.25A.749.749 0 0 1 11 16H5a.749.749 0 0 1-.53-.22L.22 11.53A.749.749 0 0 1 0 11V5c0-.199.079-.389.22-.53Zm.84 1.28L1.5 5.31v5.38l3.81 3.81h5.38l3.81-3.81V5.31L10.69 1.5ZM8 4a.75.75 0 0 1 .75.75v3.5a.75.75 0 0 1-1.5 0v-3.5A.75.75 0 0 1 8 4Zm0 8a1 1 0 1 1 0-2 1 1 0 0 1 0 2Z"/></svg>"##;

fn alert_meta(kind: BlockQuoteKind) -> (&'static str, &'static str) {
    match kind {
        BlockQuoteKind::Note => ("Note", ICON_NOTE),
        BlockQuoteKind::Tip => ("Tip", ICON_TIP),
        BlockQuoteKind::Important => ("Important", ICON_IMPORTANT),
        BlockQuoteKind::Warning => ("Warning", ICON_WARNING),
        BlockQuoteKind::Caution => ("Caution", ICON_CAUTION),
    }
}

pub fn render_body(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, MD_OPTIONS);
    let events = transform_events(parser);
    let mut body = String::new();
    html::push_html(&mut body, events.into_iter());
    body
}

/// fence 直後の Text イベントを CodeBlock の End まで集めて生テキストを返す。
/// mermaid / drawio / filename 付きコードブロックで共通して使う。
fn collect_code_text<'a, I: Iterator<Item = Event<'a>>>(
    iter: &mut std::iter::Peekable<I>,
) -> String {
    let mut content = String::new();
    for next in iter.by_ref() {
        match next {
            Event::Text(t) => content.push_str(&t),
            Event::End(TagEnd::CodeBlock) => break,
            _ => {}
        }
    }
    content
}

fn transform_events<'a, I: Iterator<Item = Event<'a>>>(parser: I) -> Vec<Event<'a>> {
    let mut out: Vec<Event<'a>> = Vec::new();
    let mut iter = parser.peekable();
    while let Some(ev) = iter.next() {
        match ev {
            Event::Start(Tag::BlockQuote(Some(kind))) => {
                out.push(Event::Start(Tag::BlockQuote(Some(kind))));
                let (label, icon) = alert_meta(kind);
                let title = format!(
                    "<div class=\"markdown-alert-title\">{}<span>{}</span></div>",
                    icon, label
                );
                out.push(Event::Html(title.into()));
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                let info_str = info.to_string();
                let mut parts = info_str.splitn(2, ':');
                let lang = parts.next().unwrap_or("").trim().to_string();
                let filename = parts.next().map(|s| s.trim().to_string());

                if lang == "mermaid" {
                    let content = collect_code_text(&mut iter);
                    let html = format!("<pre class=\"mermaid\">{}</pre>", html_escape(&content));
                    out.push(Event::Html(html.into()));
                    continue;
                }

                if lang == "drawio" {
                    let content = collect_code_text(&mut iter);
                    let config = format!(
                        r##"{{"highlight":"#0066cc","nav":true,"resize":true,"lightbox":true,"toolbar":"lightbox","xml":{}}}"##,
                        json_string(content.trim())
                    );
                    let html = format!(
                        "<div class=\"drawio-wrap\"><div class=\"mxgraph\" data-mxgraph=\"{}\"></div></div>",
                        attr_escape(&config)
                    );
                    out.push(Event::Html(html.into()));
                    continue;
                }

                if let Some(fname) = filename.filter(|s| !s.is_empty()) {
                    let content = collect_code_text(&mut iter);
                    let lang_class = if lang.is_empty() {
                        String::new()
                    } else {
                        format!(" class=\"language-{}\"", html_escape(&lang))
                    };
                    let html = format!(
                        "<div class=\"code-wrapper has-filename\"><div class=\"code-filename\">{}</div><pre><code{}>{}</code></pre></div>",
                        html_escape(&fname),
                        lang_class,
                        html_escape(&content)
                    );
                    out.push(Event::Html(html.into()));
                    continue;
                }

                out.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))));
            }
            other => out.push(other),
        }
    }
    out
}

/// 共通の `<head>` 中身を組み立てる。base/theme/custom の CSS と、
/// common.js（init/folder より前に評価させたい共有ヘルパ）・hljs・search・toc を inline する。
/// `extra_head` は呼び出し側で追加したい追記（folder 用の INITIAL_FILE 等）を末尾に差し込む。
fn head(title: &str, theme_css: &str, custom_css: &str, extra_head: &str) -> String {
    format!(
        r#"<meta charset="utf-8">
<title>{title}</title>
<style>{base_css}</style>
<style>{theme_css}</style>
<style>{custom_css}</style>
<script>{common_js}</script>
<script>{hljs_js}</script>
<script>{search_js}</script>
<script>{toc_js}</script>
{extra_head}"#,
        title = html_escape(title),
        base_css = BASE_CSS,
        theme_css = theme_css,
        custom_css = custom_css,
        common_js = COMMON_JS,
        hljs_js = HLJS_JS,
        search_js = SEARCH_JS,
        toc_js = TOC_JS,
        extra_head = extra_head,
    )
}

/// markdown 文字列を frontmatter 込みで完全な HTML ページに変換する。
/// stdin / 単一ファイル / アセット直開きの本文生成を 1 本にまとめた共通パス。
pub fn render_full_document(markdown: &str, title: &str, theme_css: &str, custom_css: &str) -> String {
    let (fm_pairs, body) = parse_frontmatter(markdown);
    let fm_html = render_frontmatter_html(&fm_pairs);
    build_html(&format!("{}{}", fm_html, render_body(body)), title, theme_css, custom_css)
}

pub fn build_html(body: &str, title: &str, theme_css: &str, custom_css: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
{head}
</head>
<body>
<article class="markdown-body">
{body}
</article>
</body>
</html>"#,
        head = head(title, theme_css, custom_css, ""),
        body = body,
    )
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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

pub fn build_folder_html(title: &str, theme_css: &str, custom_css: &str, initial_file: Option<&str>) -> String {
    let initial_file_json = match initial_file {
        None => "null".to_string(),
        Some(s) => json_string(s),
    };
    let initial_file_script = format!("<script>var INITIAL_FILE = {};</script>", initial_file_json);
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
{head}
</head>
<body class="folder-mode">
<div class="folder-layout">
  <div id="sidebar"></div>
  <div id="resizer"></div>
  <div id="preview-pane"><div class="markdown-body"></div></div>
</div>
</body>
</html>"#,
        head = head(title, theme_css, custom_css, &initial_file_script),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawio_block_becomes_mxgraph_div() {
        let md = "```drawio\n<mxGraphModel><root>A &amp; \"B\"</root></mxGraphModel>\n```\n";
        let body = render_body(md);
        assert!(body.contains("class=\"mxgraph\""), "missing mxgraph class: {body}");
        assert!(body.contains("data-mxgraph=\""), "missing data attr: {body}");
        // angle brackets and quotes from the JSON config must be attribute-escaped
        assert!(body.contains("&lt;mxGraphModel&gt;"), "xml not escaped: {body}");
        assert!(body.contains("&quot;xml&quot;"), "json quotes not escaped: {body}");
        // no raw unescaped double-quote breaking out of the attribute
        assert!(!body.contains("data-mxgraph=\"{\"highlight"), "attr not escaped: {body}");
    }

    #[test]
    fn mermaid_block_still_works() {
        let body = render_body("```mermaid\ngraph LR\nA-->B\n```\n");
        assert!(body.contains("class=\"mermaid\""), "{body}");
    }

    #[test]
    fn build_html_no_longer_inlines_diagram_libs() {
        // mermaid alone is ~3MB; with lazy loading the page must stay small.
        let page = build_html("<p>hi</p>", "t", "", "");
        assert!(page.len() < 1_000_000, "page too large, libs likely inlined: {} bytes", page.len());
        assert!(page.contains("<p>hi</p>"));
    }
}
