use std::path::Path;

use pulldown_cmark::{html, BlockQuoteKind, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};

use crate::embed;
use std::ops::Range;

/// Markdown 中の改行位置を先にインデックス化し、バイトオフセットから 1 始まりの
/// 行番号を O(log n) で引く。コメント機能の `data-src-line`（file:line 解決の肝）で使う。
/// 末尾空白のトリム（`li` レンジは末尾の空行を含むため）に元ソースも保持する。
///
/// `base` は本文がファイルの何行目から始まるか（フロントマターで飛ばした行数）。
/// レンダリング結果の行番号は**ファイルの行**でなければならない——raw 表示（⌘R）や
/// 貼り付け先の `file:line` は元ファイルの行を指すため。
struct LineIndex<'a> {
    src: &'a str,
    starts: Vec<usize>,
    base: usize,
}

impl<'a> LineIndex<'a> {
    fn new(s: &'a str, base: usize) -> Self {
        let mut starts = vec![0usize];
        for (i, b) in s.bytes().enumerate() {
            if b == b'\n' {
                starts.push(i + 1);
            }
        }
        Self { src: s, starts, base }
    }

    /// バイトオフセットを含む行の 1 始まり行番号（ファイル基準）。
    fn line(&self, offset: usize) -> usize {
        self.base + match self.starts.binary_search(&offset) {
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

pub const BASE_CSS: &str = include_str!("assets/css/base.css");
pub const HLJS_JS: &str = include_str!("assets/vendor/highlight.min.js");
pub const MERMAID_JS: &str = include_str!("assets/vendor/mermaid.min.js");
pub const DRAWIO_JS: &str = include_str!("assets/vendor/drawio-viewer.min.js");

pub const INIT_JS: &str = include_str!("assets/js/init.js");

/// ページの `<head>` に inline する自前スクリプト。**この配列の順序が読み込み順**で、
/// モジュールを足すときはここに 1 行加えるだけでよい。
///
/// 順序に意味があるのは先頭の 2 つだけ:
///   - `keymap.js` … ショートカットの単一定義元。各モジュールが評価時に
///                   `MdKeymap.on()` でハンドラを差し込むので、いちばん先に置く。
///                   この中で `MdCommon` を触るのは全て呼び出し時なので、
///                   common.js より前でも問題ない。
///   - `common.js` … 共有ヘルパとオーバーレイ レジストリ。評価時に
///                   `MdCommon.registerOverlay` を呼ぶモジュールより前に置く。
/// 残りは互いに依存しない（初期化は DOMContentLoaded 後に走る）。
///
/// モードごとの起動スクリプト（`INIT_JS` / `FOLDER_JS`）はここには含めない。
/// あちらは `<head>` ではなく初期化スクリプトとして注入される。
const PAGE_SCRIPTS: &[(&str, &str)] = &[
    ("keymap.js", include_str!("assets/js/keymap.js")),
    ("common.js", include_str!("assets/js/common.js")),
    ("highlight.min.js", include_str!("assets/vendor/highlight.min.js")),
    ("search.js", include_str!("assets/js/search.js")),
    ("palette.js", include_str!("assets/js/palette.js")),
    ("toc.js", include_str!("assets/js/toc.js")),
    ("contextmenu.js", include_str!("assets/js/contextmenu.js")),
    ("viewmode.js", include_str!("assets/js/viewmode.js")),
    ("help.js", include_str!("assets/js/help.js")),
    ("keyscroll.js", include_str!("assets/js/keyscroll.js")),
    ("comment.js", include_str!("assets/js/comment.js")),
];

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

pub const FOLDER_JS: &str = include_str!("assets/js/folder.js");

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
    render_body_in(markdown, None, 0)
}

/// `base_dir` は単独行ファイルリンクをコード埋め込みに展開するときの相対パス解決の
/// 基準ディレクトリ（通常は描画中の md ファイルがある場所）。None なら展開しない。
/// `line_offset` は本文の前に飛ばした行数（フロントマター）。`data-src-line` を
/// ファイルの行番号に揃えるために足す（`body_line_offset` で求める）。
pub fn render_body_in(markdown: &str, base_dir: Option<&Path>, line_offset: usize) -> String {
    let idx = LineIndex::new(markdown, line_offset);
    let parser = Parser::new_ext(markdown, MD_OPTIONS).into_offset_iter();
    // 裸 URL のリンク化を transform_events の前に一段挟む。
    let linkified = linkify_bare_urls(parser);
    let events = transform_events(linkified.into_iter(), &idx, base_dir);
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

/// 裸 URL のリンク化で認識するスキーム。`www.` 始まりや裸のメールアドレスは
/// 誤検知が増えるだけなので対象にしない。
const URL_SCHEMES: [&str; 2] = ["https://", "http://"];

/// 裸の `http(s)://` URL をリンクのイベント列へ分割する（GFM の autolink literals 相当）。
/// pulldown-cmark にはこの拡張のフラグが無いので、`Event::Text` を自前で加工する。
/// コードブロック・リンク・画像の中では何もしない（二重リンクや alt 属性の破壊になる）。
fn linkify_bare_urls<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>>(
    parser: I,
) -> Vec<(Event<'a>, Range<usize>)> {
    let mut out: Vec<(Event<'a>, Range<usize>)> = Vec::new();
    // リンク化を止める文脈の深さ。画像の中にリンクが入るなど入れ子があり得るので数で持つ。
    let mut skip_depth = 0usize;
    // 連続する Text をためる。pulldown は強調にならなかった `_` や `*` の位置でも
    // Text を切るので（`…/Foo_(bar)` が 3 イベントになる）、連結してから走査する。
    // HTML 出力上、隣り合う Text をひとつにまとめても結果は変わらない。
    let mut pending: Option<(String, Range<usize>)> = None;
    for (ev, range) in parser {
        let is_plain_text = matches!(ev, Event::Text(_)) && skip_depth == 0;
        if is_plain_text {
            let Event::Text(t) = &ev else { unreachable!() };
            match &mut pending {
                Some((buf, _)) => buf.push_str(t),
                None => pending = Some((t.to_string(), range)),
            }
            continue;
        }
        flush_linkified(&mut out, pending.take());
        match &ev {
            Event::Start(Tag::CodeBlock(_))
            | Event::Start(Tag::Link { .. })
            | Event::Start(Tag::Image { .. }) => skip_depth += 1,
            Event::End(TagEnd::CodeBlock)
            | Event::End(TagEnd::Link)
            | Event::End(TagEnd::Image) => skip_depth = skip_depth.saturating_sub(1),
            _ => {}
        }
        // インラインコード（Event::Code）と生 HTML は Text ではないので素通りする。
        out.push((ev, range));
    }
    flush_linkified(&mut out, pending.take());
    out
}

/// ためた Text を裸 URL で分割して吐く。分割した各イベントには元 Text のレンジを
/// 複製して付ける。transform_events がレンジを使うのはブロック要素の data-src-line
/// だけなので実害は無い。
fn flush_linkified<'a>(
    out: &mut Vec<(Event<'a>, Range<usize>)>,
    pending: Option<(String, Range<usize>)>,
) {
    let Some((text, range)) = pending else { return };
    let spans = if text.contains("http") { find_bare_urls(&text) } else { Vec::new() };
    if spans.is_empty() {
        out.push((Event::Text(text.into()), range));
        return;
    }
    let mut cursor = 0usize;
    for span in spans {
        if span.start > cursor {
            out.push((Event::Text(text[cursor..span.start].to_string().into()), range.clone()));
        }
        let url = &text[span.clone()];
        out.push((
            Event::Start(Tag::Link {
                link_type: LinkType::Autolink,
                dest_url: url.to_string().into(),
                title: "".into(),
                id: "".into(),
            }),
            range.clone(),
        ));
        out.push((Event::Text(url.to_string().into()), range.clone()));
        out.push((Event::End(TagEnd::Link), range.clone()));
        cursor = span.end;
    }
    if cursor < text.len() {
        out.push((Event::Text(text[cursor..].to_string().into()), range.clone()));
    }
}

/// `text` の中の裸 URL の範囲（バイト）を前から順に返す。
fn find_bare_urls(text: &str) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut i = 0usize;
    while i < text.len() {
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let rest = &text[i..];
        let Some(scheme) = URL_SCHEMES.iter().find(|s| rest.starts_with(**s)) else {
            i += 1;
            continue;
        };
        let body_start = i + scheme.len();
        if !start_allowed(text, i) {
            i = body_start;
            continue;
        }
        // 空白・山カッコ・非 ASCII 文字で URL を打ち切る。非 ASCII で切るのは、日本語の
        // 地の文に URL を直接埋めても「を参照」まで飲み込まないようにするため。山カッコで
        // 切るのは `&lt;` `&gt;` を URL に含めないため（実体参照は pulldown が復号済みで、
        // ここに届く時点では生の `<` `>` になっている）。
        let mut end = body_start;
        for (off, c) in text[body_start..].char_indices() {
            if c.is_whitespace() || !c.is_ascii() || c == '<' || c == '>' {
                break;
            }
            end = body_start + off + c.len_utf8();
        }
        let url = trim_url_end(&text[i..end]);
        if url.len() <= scheme.len() {
            i = body_start;
            continue;
        }
        out.push(i..i + url.len());
        i += url.len();
    }
    out
}

/// URL を始めてよい位置か。GFM の規定（行頭・空白・`*` `_` `~` `(`）に加えて、
/// 非 ASCII 文字の直後も許す。「詳細はhttps://example.com/aを参照」のように
/// スペース無しで書かれる日本語の文書でリンクになるようにするため。
fn start_allowed(text: &str, i: usize) -> bool {
    match text[..i].chars().next_back() {
        None => true,
        Some(c) => c.is_whitespace() || matches!(c, '*' | '_' | '~' | '(') || !c.is_ascii(),
    }
}

/// URL の末尾から、URL に含めるべきでない文字を剥がす。
fn trim_url_end(url: &str) -> &str {
    const TRAILING: [char; 11] = ['?', '!', '.', ',', ':', ';', '*', '_', '~', '\'', '"'];
    let mut end = url.len();
    loop {
        let before = end;
        let s = &url[..end];
        end -= s.len() - s.trim_end_matches(TRAILING).len();
        // 括弧は対応を取る。`(https://example.com/a)` の閉じ括弧は外し、
        // `https://ja.wikipedia.org/wiki/Foo_(bar)` の閉じ括弧は残す。
        while url[..end].ends_with(')') {
            let s = &url[..end];
            if s.matches(')').count() <= s.matches('(').count() {
                break;
            }
            end -= 1;
        }
        if end == before {
            return &url[..end];
        }
    }
}

fn transform_events<'a, I: Iterator<Item = (Event<'a>, Range<usize>)>>(
    parser: I,
    idx: &LineIndex<'_>,
    base_dir: Option<&Path>,
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
            // 段落は End までいったんバッファする。中身が「単独のファイルリンク」
            // だけなら、そのファイルの中身を展開したコード埋め込みに置き換える
            // （GitHub のパーマリンク埋め込みのローカル版）。段落は入れ子にならない
            // ので、次に来る End(Paragraph) が対になる。
            Event::Start(Tag::Paragraph) => {
                let mut buf: Vec<Event<'a>> = Vec::new();
                for (next, _) in iter.by_ref() {
                    if matches!(next, Event::End(TagEnd::Paragraph)) {
                        break;
                    }
                    buf.push(next);
                }
                let attrs = src_attrs(idx, &range);
                // 折りたたみ id 用の一意な番号として段落の開始行を使う（1 行に 2 つの段落は無い）。
                let uid = idx.line(range.start);
                match base_dir.and_then(|dir| embed::try_embed(dir, &buf, &attrs, uid)) {
                    Some(html) => out.push(Event::Html(html.into())),
                    None => {
                        out.push(Event::Html(format!("<p{}>", attrs).into()));
                        out.extend(buf);
                        out.push(Event::End(TagEnd::Paragraph));
                    }
                }
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

/// 共通の `<head>` 中身を組み立てる。base/theme/custom の CSS と、`PAGE_SCRIPTS` の
/// 各モジュールを nonce 付きで inline する。
/// `extra_head` は呼び出し側で追加したい追記（folder 用の INITIAL_FILE 等）を末尾に差し込む。
fn head(title: &str, theme_css: &str, custom_css: &str, extra_head: &str, nonce: &str) -> String {
    let scripts: String = PAGE_SCRIPTS
        .iter()
        .map(|(_name, src)| format!("<script nonce=\"{}\">{}</script>\n", nonce, src))
        .collect();
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
{scripts}{extra_head}"#,
        nonce = nonce,
        title = html_escape(title),
        base_css = BASE_CSS,
        theme_css = theme_css,
        custom_css = custom_css,
        scripts = scripts,
        extra_head = extra_head,
    )
}

/// markdown 文字列を frontmatter 込みで完全な HTML ページに変換する。
/// stdin / 単一ファイル / アセット直開きの本文生成を 1 本にまとめた共通パス。
/// `base_dir` は単独行ファイルリンクの展開に使う基準ディレクトリ（None なら展開しない）。
pub fn render_full_document(
    markdown: &str,
    title: &str,
    theme_css: &str,
    custom_css: &str,
    base_dir: Option<&Path>,
) -> String {
    let (fm_pairs, body) = parse_frontmatter(markdown);
    let offset = body_line_offset(markdown, body);
    let fm_html = render_frontmatter_html(&fm_pairs, offset);
    build_html(
        &format!("{}{}", fm_html, render_body_in(body, base_dir, offset)),
        title,
        theme_css,
        custom_css,
        "",
    )
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

/// `parse_frontmatter` が返した本文が、元ファイルの何行目から始まるか（0 なら 1 行目）。
/// `body` は `src` の末尾スライスであること（前半の改行数がそのまま飛ばした行数になる）。
/// 引数を逆に渡すと減算がアンダーフローするので、前提をアサートで見えるようにしておく。
pub fn body_line_offset(src: &str, body: &str) -> usize {
    debug_assert!(body.len() <= src.len(), "body は src の末尾スライスであること");
    let skipped = src.len().saturating_sub(body.len());
    src.as_bytes()[..skipped].iter().filter(|&&b| b == b'\n').count()
}

/// `lines` はフロントマターが占めるファイルの行数（`body_line_offset` の値）。
/// ブロック全体を 1 コメント単位にするため `data-src-line`（1 行目〜）を振る。
/// これが無いと、raw 表示でフロントマターの行に付けたコメントがプレビューで錨を失う
/// （本文側に対応するユニットが 1 つも無くなるため）。0 なら属性を出さない。
pub fn render_frontmatter_html(pairs: &[(String, String)], lines: usize) -> String {
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
    let attrs = match lines {
        0 => String::new(),
        1 => r#" data-src-line="1""#.to_string(),
        n => format!(r#" data-src-line="1" data-src-end-line="{}""#, n),
    };
    format!(r#"<div class="frontmatter"{}>{}</div>"#, attrs, rows)
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
    fn frontmatter_shifts_src_line_to_file_lines() {
        // フロントマターがあっても data-src-line は「ファイルの行」を指す。ここがずれると
        // raw 表示（1 行 1 ユニット）とプレビューで同じコメントが別の行を指してしまう。
        let md = "---\ntitle: t\n---\n\n# 見出し\n\n段落。\n";
        let (_, body) = parse_frontmatter(md);
        assert_eq!(body_line_offset(md, body), 3);
        let html = render_body_in(body, None, body_line_offset(md, body));
        assert!(html.contains("<h1 data-src-line=\"5\">"), "heading: {html}");
        assert!(html.contains("<p data-src-line=\"7\">"), "paragraph: {html}");
    }

    #[test]
    fn frontmatter_block_is_one_unit() {
        // フロントマターのブロック自体もコメント単位（1 行目〜終わりの `---`）。
        // raw でフロントマターの行に付けたコメントの錨がここになる。
        let md = "---\ntitle: t\nauthor: z\n---\n\n# 見出し\n";
        let (pairs, body) = parse_frontmatter(md);
        let html = render_frontmatter_html(&pairs, body_line_offset(md, body));
        assert!(
            html.contains(r#"<div class="frontmatter" data-src-line="1" data-src-end-line="4">"#),
            "frontmatter unit: {html}"
        );
    }

    #[test]
    fn frontmatter_src_line_handles_crlf() {
        // CRLF でも飛ばした行数の数え方（\n の個数）は変わらない。
        let md = "---\r\ntitle: t\r\n---\r\n\r\n# 見出し\r\n";
        let (_, body) = parse_frontmatter(md);
        assert_eq!(body_line_offset(md, body), 3);
        let html = render_body_in(body, None, body_line_offset(md, body));
        assert!(html.contains("<h1 data-src-line=\"5\">"), "crlf heading: {html}");
    }


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

    // ── 単独行ファイルリンクのコード埋め込み ──

    /// 一時ディレクトリに 3 行のファイルを置き、その中で markdown を描画する。
    fn render_in_tempdir(md: &str, name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("md-html-embed-{}", name));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("f.txt"), "one\ntwo\nthree\n").unwrap();
        render_body_in(md, Some(&dir), 0)
    }

    #[test]
    fn standalone_file_link_becomes_code_embed() {
        let body = render_in_tempdir("[f](./f.txt#L2)\n", "standalone");
        assert!(body.contains("class=\"code-wrapper has-filename code-embed\""), "{body}");
        // 段落ではなくコード埋め込みになり、リンクは残らない。
        assert!(!body.contains("<a href"), "link should be replaced: {body}");
        assert!(body.contains(">two</code>"), "wrong line embedded: {body}");
        // コードブロックや表と同じくブロック単位でコメントできる。
        assert!(body.contains("code-embed\" data-src-line=\"1\""), "src-line missing: {body}");
    }

    #[test]
    fn inline_file_link_stays_a_link() {
        let body = render_in_tempdir("見て [f](./f.txt#L2) なのだ。\n", "inline");
        assert!(body.contains("<a href=\"./f.txt#L2\">"), "{body}");
        assert!(!body.contains("code-embed"), "should not embed inline link: {body}");
    }

    #[test]
    fn embedding_is_off_without_base_dir() {
        // base_dir が無ければ（fuzz や単体テストの render_body 経路）展開しない。
        let body = render_body("[f](./f.txt#L2)\n");
        assert!(body.contains("<a href=\"./f.txt#L2\">"), "{body}");
        assert!(!body.contains("code-embed"), "{body}");
    }

    #[test]
    fn paragraph_with_two_links_is_not_embedded() {
        let body = render_in_tempdir("[a](./f.txt#L1)[b](./f.txt#L2)\n", "twolinks");
        assert!(!body.contains("code-embed"), "{body}");
        assert_eq!(body.matches("<a href").count(), 2, "{body}");
    }

    // ── 裸 URL の自動リンク化 ──

    fn hrefs(body: &str) -> Vec<String> {
        body.match_indices("<a href=\"")
            .map(|(i, m)| {
                let rest = &body[i + m.len()..];
                rest[..rest.find('"').unwrap()].to_string()
            })
            .collect()
    }

    #[test]
    fn bare_url_becomes_a_link() {
        let body = render_body("見て https://example.com/a なのだ。\n");
        assert_eq!(hrefs(&body), vec!["https://example.com/a"], "{body}");
    }

    #[test]
    fn bare_url_in_various_containers() {
        let cases = [
            "- https://example.com/a\n",
            "| a |\n| --- |\n| https://example.com/a |\n",
            "**https://example.com/a**\n",
            "> [!NOTE]\n> https://example.com/a\n",
            "# https://example.com/a\n",
        ];
        for md in cases {
            let body = render_body(md);
            assert_eq!(hrefs(&body), vec!["https://example.com/a"], "md={md:?} body={body}");
        }
    }

    #[test]
    fn url_in_inline_code_stays_text() {
        let body = render_body("`https://example.com/a` なのだ。\n");
        assert!(hrefs(&body).is_empty(), "{body}");
        assert!(body.contains("<code>https://example.com/a</code>"), "{body}");
    }

    #[test]
    fn url_in_code_block_stays_text() {
        let body = render_body("```\nsee https://example.com/a\n```\n");
        assert!(hrefs(&body).is_empty(), "{body}");
        // インデントされたコードブロックも同じ。
        let body = render_body("    see https://example.com/a\n");
        assert!(hrefs(&body).is_empty(), "{body}");
    }

    #[test]
    fn angle_autolink_is_not_double_linked() {
        let body = render_body("<https://example.com/a>\n");
        assert_eq!(hrefs(&body), vec!["https://example.com/a"], "{body}");
        assert_eq!(body.matches("<a ").count(), 1, "{body}");
    }

    #[test]
    fn inline_link_with_url_text_is_not_nested() {
        let body = render_body("[https://example.com/a](https://example.com/b)\n");
        assert_eq!(hrefs(&body), vec!["https://example.com/b"], "{body}");
        assert_eq!(body.matches("<a ").count(), 1, "{body}");
    }

    #[test]
    fn trailing_punctuation_is_excluded() {
        for suffix in ["。", ".", ",", "!", "?", ":", ";", "、", "」", "&gt;", "*", "'"] {
            let md = format!("x https://example.com/a{suffix} y\n");
            let body = render_body(&md);
            assert_eq!(hrefs(&body), vec!["https://example.com/a"], "suffix={suffix:?} {body}");
        }
    }

    #[test]
    fn parens_are_balanced() {
        let body = render_body("(https://example.com/a) なのだ。\n");
        assert_eq!(hrefs(&body), vec!["https://example.com/a"], "{body}");
        let body = render_body("https://ja.wikipedia.org/wiki/Foo_(bar) なのだ。\n");
        assert_eq!(hrefs(&body), vec!["https://ja.wikipedia.org/wiki/Foo_(bar)"], "{body}");
    }

    #[test]
    fn full_width_neighbors_are_handled() {
        // 直前が全角でも開始を許し、全角が来たら URL を打ち切る。
        let body = render_body("詳細はhttps://example.com/aを参照。\n");
        assert_eq!(hrefs(&body), vec!["https://example.com/a"], "{body}");
        assert!(body.contains("を参照。"), "trailing text lost: {body}");
        // 全角括弧も打ち切りの規則ひとつで外れる。
        let body = render_body("（https://example.com/a）\n");
        assert_eq!(hrefs(&body), vec!["https://example.com/a"], "{body}");
    }

    #[test]
    fn image_alt_with_url_is_not_broken() {
        let body = render_body("![https://example.com/a](./x.png)\n");
        assert!(hrefs(&body).is_empty(), "{body}");
        assert!(body.contains("alt=\"https://example.com/a\""), "{body}");
    }

    #[test]
    fn multiple_urls_in_one_paragraph() {
        let body = render_body("a https://example.com/1 b http://example.com/2 c\n");
        assert_eq!(hrefs(&body), vec!["https://example.com/1", "http://example.com/2"], "{body}");
    }

    #[test]
    fn url_query_ampersand_is_escaped() {
        let body = render_body("https://example.com/a?x=1&y=2\n");
        assert_eq!(hrefs(&body), vec!["https://example.com/a?x=1&amp;y=2"], "{body}");
    }

    #[test]
    fn bare_url_paragraph_is_not_code_embedded() {
        // 単独リンクだけの段落に見えるが、スキーム付きなのでコード埋め込みは走らない。
        let body = render_in_tempdir("https://example.com/f.txt\n", "bareurl");
        assert!(!body.contains("code-embed"), "{body}");
        assert_eq!(hrefs(&body), vec!["https://example.com/f.txt"], "{body}");
    }
}
