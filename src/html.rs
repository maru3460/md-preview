use pulldown_cmark::{html, BlockQuoteKind, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;

/// Markdown 中の改行位置を先にインデックス化し、バイトオフセットから 1 始まりの
/// 行番号を O(log n) で引く。コメント機能の `data-src-line`（file:line 解決の肝）で使う。
/// 末尾空白のトリム（`li` レンジは末尾の空行を含むため）に元ソースも保持する。
struct LineIndex<'a> {
    src: &'a str,
    starts: Vec<usize>,
}

impl<'a> LineIndex<'a> {
    fn new(s: &'a str) -> Self {
        let mut starts = vec![0usize];
        for (i, b) in s.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Self { src: s, starts }
    }

    /// バイトオフセットを含む行の 1 始まり行番号。
    fn line(&self, offset: usize) -> usize {
        match self.starts.binary_search(&offset) {
            Ok(i) => i + 1,
            Err(i) => i,
        }
    }
}

/// ソース範囲から `data-src-line`（＋複数行なら `data-src-end-line`）属性文字列を作る。
/// 開きタグに差し込むと、JS が `closest('[data-src-line]')` で最寄りの行番号を読める。
/// 終了行はレンジ末尾の空白を除いた「最後の非空白バイト」で算出する。pulldown の
/// `Item`（リスト項目）レンジは末尾の空行を含み、素直に `end-1` を使うと end-line が
/// 1 行過剰になるため。段落・見出し等は末尾空白を含まないので影響しない。
fn src_attrs(idx: &LineIndex, range: &Range<usize>) -> String {
    let start = idx.line(range.start);
    let slice_end = range.end.min(idx.src.len());
    let slice = idx.src.get(range.start..slice_end).unwrap_or("");
    let trimmed = slice.trim_end();
    let end_off = if trimmed.is_empty() {
        range.start
    } else {
        range.start + trimmed.len() - 1
    };
    let end = idx.line(end_off);
    if end > start {
        format!(" data-src-line=\"{}\" data-src-end-line=\"{}\"", start, end)
    } else {
        format!(" data-src-line=\"{}\"", start)
    }
}

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
            // `<` を潰しておくと、inline <script> に値を埋めても `</script>` を形成できない。
            // U+2028/U+2029 は pre-ES2019 で行終端子扱いになり構文を壊すため合わせて潰す。
            '<' => out.push_str("\\u003c"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
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
pub const CONTEXT_JS: &str = include_str!("contextmenu.js");
pub const DIFF_JS: &str = include_str!("diff.js");
pub const RAW_JS: &str = include_str!("raw.js");
pub const HELP_JS: &str = include_str!("help.js");
pub const KEYSCROLL_JS: &str = include_str!("keyscroll.js");
pub const COMMENT_JS: &str = include_str!("comment.js");

/// CSP の nonce を生成する。本文（untrusted な Markdown）に埋め込まれた inline
/// script を実行させないため、自前の inline script だけにこの nonce を付ける。
/// 攻撃者は静的なファイルなので nonce を読めず（ブラウザが nonce 属性を隠す）、
/// script を走らせて nonce を得ることもできない（鶏卵）ため、時刻由来で十分。
fn make_nonce() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:032x}", n)
}

pub const MD_OPTIONS: Options = Options::ENABLE_TABLES
    .union(Options::ENABLE_TASKLISTS)
    .union(Options::ENABLE_STRIKETHROUGH)
    .union(Options::ENABLE_FOOTNOTES)
    .union(Options::ENABLE_GFM)
    .union(Options::ENABLE_WIKILINKS);

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
    let idx = LineIndex::new(markdown);
    let parser = Parser::new_ext(markdown, MD_OPTIONS).into_offset_iter();
    let events = transform_events(parser, &idx);
    let mut body = String::new();
    html::push_html(&mut body, events.into_iter());
    body
}

/// fence 直後の Text イベントを CodeBlock の End まで集めて生テキストを返す。
/// mermaid / drawio / filename 付きコードブロックで共通して使う。
fn collect_code_text<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>>(
    iter: &mut std::iter::Peekable<I>,
) -> String {
    let mut content = String::new();
    for (next, _) in iter.by_ref() {
        match next {
            Event::Text(t) => content.push_str(&t),
            Event::End(TagEnd::CodeBlock) => break,
            _ => {}
        }
    }
    content
}

/// 生 HTML ブロック（`<details>` / `<summary>` など）の開きタグに data-src-line を差し込む。
/// pulldown はインライン HTML を Event::Html で素通しするため、アコーディオンのトグルは
/// そのままだとコメント対象にならない。チャンク内の各開きタグの位置から行番号を算出して注入する。
fn inject_html_src_line(html: &str, base: usize, idx: &LineIndex<'_>) -> String {
    let mut out = String::with_capacity(html.len() + 32);
    let mut rest = html;
    let mut consumed = 0usize; // 元 html から消費したバイト数（行番号算出の起点）
    loop {
        let d = rest.find("<details");
        let s = rest.find("<summary");
        let next = match (d, s) {
            (None, None) => None,
            (Some(a), None) => Some((a, "<details".len())),
            (None, Some(b)) => Some((b, "<summary".len())),
            (Some(a), Some(b)) => {
                if a <= b {
                    Some((a, "<details".len()))
                } else {
                    Some((b, "<summary".len()))
                }
            }
        };
        match next {
            None => {
                out.push_str(rest);
                break;
            }
            Some((pos, taglen)) => {
                let after_tag = pos + taglen;
                // 開きタグと確定できる場合（次が区切り文字）だけ注入する。
                let delim_ok = rest[after_tag..]
                    .chars()
                    .next()
                    .map_or(false, |c| matches!(c, ' ' | '>' | '\t' | '\n' | '\r' | '/'));
                out.push_str(&rest[..after_tag]);
                if delim_ok {
                    let line = idx.line(base + consumed + pos);
                    out.push_str(&format!(" data-src-line=\"{}\"", line));
                }
                consumed += after_tag;
                rest = &rest[after_tag..];
            }
        }
    }
    out
}

/// alert 引用の class 名。pulldown の既定出力（`class="markdown-alert-note"` 等）に合わせる。
fn alert_class(kind: BlockQuoteKind) -> &'static str {
    match kind {
        BlockQuoteKind::Note => "markdown-alert-note",
        BlockQuoteKind::Tip => "markdown-alert-tip",
        BlockQuoteKind::Important => "markdown-alert-important",
        BlockQuoteKind::Warning => "markdown-alert-warning",
        BlockQuoteKind::Caution => "markdown-alert-caution",
    }
}

fn transform_events<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>>(
    parser: I,
    idx: &LineIndex<'_>,
) -> Vec<Event<'a>> {
    let mut out: Vec<Event<'a>> = Vec::new();
    let mut iter = parser.peekable();
    // 開いたコードブロックを code-wrapper div で包んだかどうか。End(CodeBlock) で
    // 閉じ div を出すべきか判定する（mermaid/drawio/filename は End を自前で消費するので
    // ここには来ない。来るのは素の fenced / indented ブロックだけ）。
    let mut code_wrap_open = false;
    while let Some((ev, range)) = iter.next() {
        match ev {
            // 各ユニットの開きタグを自前生成し、コメント用の data-src-line を注入する。
            // 中身のインラインイベントと End はそのまま流れ、push_html が閉じタグを出す。
            // 注: 見出しの id/classes/attrs は破棄している。現状 MD_OPTIONS に
            // ENABLE_HEADING_ATTRIBUTES が無く pulldown も id を出さないので実害は無いが、
            // その拡張を有効化する場合は `{#id .class}` やアンカー id がここで落ちる点に注意。
            Event::Start(Tag::Heading { level, .. }) => {
                out.push(Event::Html(format!("<{}{}>", level, src_attrs(idx, &range)).into()));
            }
            Event::Start(Tag::Paragraph) => {
                out.push(Event::Html(format!("<p{}>", src_attrs(idx, &range)).into()));
            }
            Event::Start(Tag::Item) => {
                out.push(Event::Html(format!("<li{}>", src_attrs(idx, &range)).into()));
            }
            Event::Start(Tag::BlockQuote(kind)) => {
                let attrs = src_attrs(idx, &range);
                match kind {
                    Some(k) => {
                        out.push(Event::Html(
                            format!("<blockquote class=\"{}\"{}>\n", alert_class(k), attrs).into(),
                        ));
                        let (label, icon) = alert_meta(k);
                        out.push(Event::Html(
                            format!(
                                "<div class=\"markdown-alert-title\">{}<span>{}</span></div>",
                                icon, label
                            )
                            .into(),
                        ));
                    }
                    None => {
                        out.push(Event::Html(format!("<blockquote{}>\n", attrs).into()));
                    }
                }
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                let info_str = info.to_string();
                let mut parts = info_str.splitn(2, ':');
                let lang = parts.next().unwrap_or("").trim().to_string();
                let filename = parts.next().map(|s| s.trim().to_string());
                let attrs = src_attrs(idx, &range);

                if lang == "mermaid" {
                    let content = collect_code_text(&mut iter);
                    let html = format!(
                        "<pre class=\"mermaid\"{}>{}</pre>",
                        attrs,
                        html_escape(&content)
                    );
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
                        "<div class=\"drawio-wrap\"{}><div class=\"mxgraph\" data-mxgraph=\"{}\"></div></div>",
                        attrs,
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
                        // 属性値なので attr_escape（" もエスケープ）を使う。
                        format!(" class=\"language-{}\"", attr_escape(&lang))
                    };
                    let html = format!(
                        "<div class=\"code-wrapper has-filename\"{}><div class=\"code-filename\">{}</div><pre><code{}>{}</code></pre></div>",
                        attrs,
                        html_escape(&fname),
                        lang_class,
                        html_escape(&content)
                    );
                    out.push(Event::Html(html.into()));
                    continue;
                }

                // 素の fenced ブロック: pulldown の <pre><code> を code-wrapper で包み、
                // ブロック単位でコメントできるよう外枠に data-src-line を振る。
                out.push(Event::Html(
                    format!("<div class=\"code-wrapper\"{}>", attrs).into(),
                ));
                out.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))));
                code_wrap_open = true;
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Indented)) => {
                out.push(Event::Html(
                    format!("<div class=\"code-wrapper\"{}>", src_attrs(idx, &range)).into(),
                ));
                out.push(Event::Start(Tag::CodeBlock(CodeBlockKind::Indented)));
                code_wrap_open = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                out.push(Event::End(TagEnd::CodeBlock));
                if code_wrap_open {
                    out.push(Event::Html("</div>".into()));
                    code_wrap_open = false;
                }
            }
            // テーブルは読み幅(720px)を飛び出して広く使えるよう、スクロール用の
            // ラッパー div で囲む。長い file:line などがはみ出しても本文ごとでは
            // なく表の中だけ横スクロールになる。
            //
            // 行番号(data-src-line)はラッパー div に付け、テーブル全体を 1 ユニットとして
            // コメント対象にする（コードブロックと同じブロック単位の割り切り）。
            // `Start(TableHead)`/`Start(TableRow)` を自前 HTML に置換すると push_html の
            // テーブル状態機械（table_state / table_cell_index）が更新されず、本文セルの
            // アライメント消失・2 つ目以降のテーブルでヘッダが <td> 化する回帰が出るため、
            // 行/セルの Start は一切いじらず pulldown にそのまま描かせる。
            Event::Start(Tag::Table(align)) => {
                out.push(Event::Html(
                    format!("<div class=\"table-wrap\"{}>", src_attrs(idx, &range)).into(),
                ));
                out.push(Event::Start(Tag::Table(align)));
            }
            Event::End(TagEnd::Table) => {
                out.push(Event::End(TagEnd::Table));
                out.push(Event::Html("</div>".into()));
            }
            // 生 HTML ブロック。details / summary が含まれていればトグルをコメント可能にする。
            Event::Html(html) => {
                if html.contains("<details") || html.contains("<summary") {
                    out.push(Event::Html(inject_html_src_line(&html, range.start, idx).into()));
                } else {
                    out.push(Event::Html(html));
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// 共通の `<head>` 中身を組み立てる。base/theme/custom の CSS と、
/// common.js（init/folder より前に評価させたい共有ヘルパ）・hljs・search・toc を inline する。
/// `extra_head` は呼び出し側で追加したい追記（folder 用の INITIAL_FILE 等）を末尾に差し込む。
fn head(title: &str, theme_css: &str, custom_css: &str, extra_head: &str, nonce: &str) -> String {
    // CSP: script は nonce 付きの自前 script と同一オリジン（mdpreview://localhost、
    // /__lib/ の mermaid・drawio 等）のみ許可。'unsafe-inline' を入れないことで、
    // 本文に書かれた <script> や on* 属性は実行されない。'unsafe-eval' は mermaid/drawio
    // 用の保険だが、nonce 無しでは攻撃者 script 自体が走らないため eval には到達できない。
    format!(
        r#"<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self' 'unsafe-eval' 'nonce-{nonce}'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https: http:; font-src 'self' data:; connect-src 'self'; media-src 'self' data:; frame-src 'self'; worker-src 'self' blob:">
<title>{title}</title>
<style>{base_css}</style>
<style>{theme_css}</style>
<style>{custom_css}</style>
<script nonce="{nonce}">{common_js}</script>
<script nonce="{nonce}">{hljs_js}</script>
<script nonce="{nonce}">{search_js}</script>
<script nonce="{nonce}">{toc_js}</script>
<script nonce="{nonce}">{context_js}</script>
<script nonce="{nonce}">{diff_js}</script>
<script nonce="{nonce}">{raw_js}</script>
<script nonce="{nonce}">{help_js}</script>
<script nonce="{nonce}">{keyscroll_js}</script>
<script nonce="{nonce}">{comment_js}</script>
{extra_head}"#,
        nonce = nonce,
        title = html_escape(title),
        base_css = BASE_CSS,
        theme_css = theme_css,
        custom_css = custom_css,
        common_js = COMMON_JS,
        hljs_js = HLJS_JS,
        search_js = SEARCH_JS,
        toc_js = TOC_JS,
        context_js = CONTEXT_JS,
        diff_js = DIFF_JS,
        raw_js = RAW_JS,
        help_js = HELP_JS,
        keyscroll_js = KEYSCROLL_JS,
        comment_js = COMMENT_JS,
        extra_head = extra_head,
    )
}

/// markdown 文字列を frontmatter 込みで完全な HTML ページに変換する。
/// stdin / 単一ファイル / アセット直開きの本文生成を 1 本にまとめた共通パス。
pub fn render_full_document(markdown: &str, title: &str, theme_css: &str, custom_css: &str) -> String {
    let (fm_pairs, body) = parse_frontmatter(markdown);
    let fm_html = render_frontmatter_html(&fm_pairs);
    build_html(&format!("{}{}", fm_html, render_body(body)), title, theme_css, custom_css, "")
}

/// `body_class` は `.markdown-body` に足す追加クラス（空文字なら無し）。
/// ソース表示は `"source-page"` を渡して 720px 中央制約を外し全幅にする。
pub fn build_html(body: &str, title: &str, theme_css: &str, custom_css: &str, body_class: &str) -> String {
    let nonce = make_nonce();
    let class_attr = if body_class.is_empty() {
        "markdown-body".to_string()
    } else {
        format!("markdown-body {}", body_class)
    };
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
{head}
</head>
<body>
<article class="{class_attr}">
{body}
</article>
</body>
</html>"#,
        head = head(title, theme_css, custom_css, "", &nonce),
        class_attr = class_attr,
        body = body,
    )
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn attr_escape(s: &str) -> String {
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
    let nonce = make_nonce();
    let initial_file_script = format!(
        "<script nonce=\"{}\">var INITIAL_FILE = {};</script>",
        nonce, initial_file_json
    );
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
{head}
</head>
<body class="folder-mode">
<div class="folder-layout">
  <div id="sidebar" tabindex="-1"></div>
  <div id="resizer"></div>
  <div id="preview-pane" tabindex="-1"><div class="markdown-body"></div></div>
</div>
</body>
</html>"#,
        head = head(title, theme_css, custom_css, &initial_file_script, &nonce),
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
        // `<` は json_string が `<` に、`>` は attr_escape が `&gt;` にする。どちらも
        // ブラウザの JSON パースで `<`/`>` に戻るので drawio には正しく渡り、かつ属性を
        // 生の山括弧で抜け出さない（`</script>` や属性破りを防ぐ）。
        assert!(body.contains("\\u003cmxGraphModel&gt;"), "xml not escaped: {body}");
        assert!(!body.contains("<mxGraphModel>"), "raw angle brackets leaked: {body}");
        assert!(body.contains("&quot;xml&quot;"), "json quotes not escaped: {body}");
        // 属性から抜け出すような、エスケープされていない生の二重引用符が無いこと
        assert!(!body.contains("data-mxgraph=\"{\"highlight"), "attr not escaped: {body}");
    }

    #[test]
    fn mermaid_block_still_works() {
        let body = render_body("```mermaid\ngraph LR\nA-->B\n```\n");
        assert!(body.contains("class=\"mermaid\""), "{body}");
    }

    #[test]
    fn build_html_no_longer_inlines_diagram_libs() {
        // mermaid だけで約 3MB。遅延ロードにより、ページは小さく保たれるはず。
        let page = build_html("<p>hi</p>", "t", "", "", "");
        assert!(page.len() < 1_000_000, "page too large, libs likely inlined: {} bytes", page.len());
        assert!(page.contains("<p>hi</p>"));
    }

    // ── コメント機能: data-src-line 注入 ─────────────────────────

    #[test]
    fn src_line_on_common_units() {
        // 1:見出し 3:段落 5,6:リスト 8〜:コードブロック
        let md = "# H\n\npara\n\n- a\n- b\n\n```\ncode\n```\n";
        let body = render_body(md);
        assert!(body.contains("<h1 data-src-line=\"1\">"), "heading: {body}");
        assert!(body.contains("<p data-src-line=\"3\">"), "paragraph: {body}");
        assert!(body.contains("<li data-src-line=\"5\">"), "li a: {body}");
        assert!(body.contains("<li data-src-line=\"6\">"), "li b: {body}");
        assert!(body.contains("class=\"code-wrapper\" data-src-line=\"8\""), "code: {body}");
    }

    #[test]
    fn src_end_line_multiline_paragraph() {
        // 複数行にまたがる段落は data-src-end-line を持つ。
        let md = "line one\nline two\nline three\n";
        let body = render_body(md);
        assert!(
            body.contains("<p data-src-line=\"1\" data-src-end-line=\"3\">"),
            "multiline paragraph end-line: {body}"
        );
    }

    #[test]
    fn src_line_li_not_off_by_one() {
        // loose list（項目間に空行）でも li の end-line が末尾空行を含まないこと。
        let md = "- a\n\n- b\n";
        let body = render_body(md);
        // 項目 a は 1 行だけ。:1-2 のように空行を巻き込んではならない。
        assert!(body.contains("<li data-src-line=\"1\">"), "li a should be single-line: {body}");
        assert!(!body.contains("data-src-end-line=\"2\""), "li a must not include blank line: {body}");
    }

    // ── コメント機能: テーブルの非回帰（push_html 状態機械を壊さない） ──

    #[test]
    fn table_alignment_preserved_in_body() {
        // アライメント付きテーブルは body セルにも text-align が残る。
        let md = "| A | B |\n|:--|--:|\n| 1 | 2 |\n";
        let body = render_body(md);
        assert!(body.contains("<td style=\"text-align: left\">"), "body left align lost: {body}");
        assert!(body.contains("<td style=\"text-align: right\">"), "body right align lost: {body}");
        // テーブル全体がブロック単位のコメント対象になる。
        assert!(body.contains("class=\"table-wrap\" data-src-line=\"1\""), "table-wrap src-line: {body}");
    }

    #[test]
    fn second_table_header_is_th() {
        // 2 つ目のテーブルでもヘッダが <th> のまま（<td> に化けない）。
        let md = "| A | B |\n|---|---|\n| 1 | 2 |\n\npara\n\n| C | D |\n|---|---|\n| 3 | 4 |\n";
        let body = render_body(md);
        // ヘッダセルの <td> 化が起きていないこと。
        assert!(!body.contains("<thead><tr><td"), "2nd table header became <td>: {body}");
        assert_eq!(body.matches("<th>").count(), 4, "expected 4 header cells: {body}");
    }

    #[test]
    fn details_summary_get_src_line() {
        let md = "<details>\n<summary>x</summary>\n\nbody\n\n</details>\n";
        let body = render_body(md);
        assert!(body.contains("<details data-src-line=\"1\">"), "details: {body}");
        assert!(body.contains("<summary data-src-line=\"2\">"), "summary: {body}");
        // 閉じタグを誤爆していないこと。
        assert!(body.contains("</details>"), "closing details missing: {body}");
        assert!(!body.contains("</details data-src-line"), "closing tag wrongly injected: {body}");
    }

    #[test]
    fn crlf_line_numbers() {
        let md = "# H\r\n\r\npara\r\n";
        let body = render_body(md);
        assert!(body.contains("<h1 data-src-line=\"1\">"), "crlf heading: {body}");
        assert!(body.contains("<p data-src-line=\"3\">"), "crlf paragraph: {body}");
    }

    #[test]
    fn empty_and_multibyte_no_panic() {
        // 空・マルチバイトでパニックしないこと（境界安全性）。
        let _ = render_body("");
        let _ = render_body("# 日本語の見出し\n\nマルチバイト段落なのだ。\n");
    }
}
