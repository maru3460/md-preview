//! 外部の読み込みを止めているとき（[`crate::csp::Policy::trusted`] が OFF）、md の本文の
//! 外部の画像・iframe・動画・音声を、小さな「開く」リンクに置き換える。
//!
//! 守っているのは CSP で、ここは見た目のためだけにある。置き換えないと、止めた画像は壊れた
//! 画像の印に、iframe は空の枠になり、何があったのか分からない（止めたこと自体を知らせる
//! 帯などは出さないが、消えた中身の代わりは置く）。取りこぼしても読み込みは CSP が止める。
//!
//! 置き換えるのは http(s) の URL（`//host/…` を含む）だけ。手元のファイル（相対パス・`/…`）と
//! `data:` / `blob:`・アプリ自身（`mdpreview://localhost`）はそのまま残す。それ以外のスキーム
//! （`file:`・`vscode:`・`javascript:` など）は、押せる形で残す理由が無いので、置き換えずに消す。
//!
//! `.html` の中は描くのが WebView なので、同じことを `extmedia.js` が読み込み後の DOM でやる。
//! 対象と文字の選び方は同じ。違うのは次のところで、どれも `.html` 側が URL の読み方を WebView に
//! 任せられるのに対し、こちらは自前で読むことから来る:
//! - `about:` は向こうだけ残す（`about:blank` の iframe は `.html` にはよくあり、md には出ない）
//! - URL として読めないものは、向こうは消し、こちらは自前の緩い読み方で分ける
//! - 文書と同じ origin の http(s) は、向こうは手元として残す（開発用サーバは `.html` を http で
//!   配る）。こちらの手元の参照は `/…` のパスに畳まれているので、http(s) は常に外部
//! - `mdpreview:` は、向こうは文書と同じホストを、こちらは `localhost` を手元とする（同じ意味）
//!
//! md をここ（Rust）で置き換えるのは、`--html` のダンプとスナップショットに置き換えた後の形が
//! 出て、描いた後に差し替わるちらつきが無く、単体テストで確かめられるため。
//!
//! Why not pulldown-cmark のイベント（`Tag::Image` と生 HTML の属性）で書き換える:
//! 生 HTML は行ごとに別のイベントで来るので、複数行にまたがる `<picture>` や `<video>` を
//! 1 個にまとめられない。描き終えた本文の HTML を 1 回なめる形にした。本文の中の `<` は
//! pulldown-cmark がエスケープするので、ここで見えるタグは本物のタグだけになる。

use crate::html::{attr_escape, html_escape};

/// 本文の HTML の外部の画像などを置き換える。置き換えるものが無ければ None。
pub fn replace_external(html: &str) -> Option<String> {
    Scanner::new(html).run()
}

/// 置き換えたものの種類。リンクの文字の頭に出す。
#[derive(Clone, Copy)]
enum Media {
    Image,
    Video,
    Audio,
    Frame,
}

impl Media {
    fn label(self) -> &'static str {
        match self {
            Media::Image => "外部の画像",
            Media::Video => "外部の動画",
            Media::Audio => "外部の音声",
            Media::Frame => "外部の埋め込み",
        }
    }
}

/// URL の分け方。
#[derive(Debug, PartialEq)]
enum Src {
    /// 手元のもの・`data:`・`blob:`・属性が無い。そのまま残す。
    Local,
    /// http(s)。中身は読み直した URL（[`web_url`]）と、ホスト名。
    External { url: String, host: String },
    /// それ以外のスキーム。消す。
    Blocked,
}

/// 属性値（文字参照は解いたもの）を分ける。WebView の URL の読み方に寄せる。
/// 前後の空白と制御文字を落とし、途中のタブと改行は取り除く（`ht\ttps:` も https になる）。
fn classify(value: &str) -> Src {
    let cleaned: String = value
        .trim_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    // `//host/…`（`\\host` も）は、WebView が http(s) の文書の上で読むのと同じく、ウェブの
    // URL として扱う。アプリの文書（mdpreview:）の上では mdpreview://host になって外部と
    // 分からないが、書いた人の意図はウェブなので、https で開くリンクにする。
    let slash = |c: Option<char>| matches!(c, Some('/' | '\\'));
    let mut chars = cleaned.chars();
    if slash(chars.next()) && slash(chars.next()) {
        return web_url("https", &cleaned);
    }
    let Some(colon) = cleaned.find(':') else { return Src::Local };
    let scheme = &cleaned[..colon];
    let is_scheme = scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if !is_scheme {
        // `a/b:c.png` のような相対パス（最初の `:` より前に `/` などがある）。
        return Src::Local;
    }
    let rest = &cleaned[colon + 1..];
    match scheme.to_ascii_lowercase().as_str() {
        s @ ("http" | "https") => web_url(s, rest),
        "data" | "blob" => Src::Local,
        // アプリ自身（csp::APP_ORIGIN）。ほかのホストはアプリが配っていない住所。
        "mdpreview" => match split_authority(rest) {
            (_, host, _) if host.eq_ignore_ascii_case("localhost") => Src::Local,
            _ => Src::Blocked,
        },
        _ => Src::Blocked,
    }
}

/// スキームの `:` の後ろを (userinfo（`@` 込み）, ホスト（ポート込み）, パス以降) に分ける。
/// 頭の `/` と `\` は飛ばす（http(s) では `\` も `/` と同じに読まれる）。
fn split_authority(rest: &str) -> (&str, &str, &str) {
    let rest = rest.trim_start_matches(['/', '\\']);
    let end = rest.find(['/', '\\', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let (userinfo, host) = match authority.rfind('@') {
        Some(i) => (&authority[..=i], &authority[i + 1..]),
        None => ("", authority),
    };
    (userinfo, host, &rest[end..])
}

/// http(s) の URL を、WebView が読み直すのに近い形にする（`.html` 側の `URL.href` に寄せる）:
/// スキームとホストを小文字に、パスの `\` を `/` に、既定のポートを落とし、空のパスは `/`。
/// リンクの href と title にはこの形を出す。ホストが無ければ読み込みようが無いので Blocked。
fn web_url(scheme: &str, rest: &str) -> Src {
    let (userinfo, host, tail) = split_authority(rest);
    let mut host = host.to_ascii_lowercase();
    let default_port = if scheme == "http" { ":80" } else { ":443" };
    if let Some(h) = host.strip_suffix(default_port) {
        host.truncate(h.len());
    }
    if host.is_empty() {
        return Src::Blocked;
    }
    let q = tail.find(['?', '#']).unwrap_or(tail.len());
    let path = match tail[..q].replace('\\', "/") {
        p if p.is_empty() => "/".to_string(),
        p => p,
    };
    let url = format!("{scheme}://{userinfo}{host}{path}{}", &tail[q..]);
    Src::External { url, host }
}

/// 属性値の文字参照を解く。数値と、URL を隠すのに使える名前だけを知っている。
/// 知らない名前はそのまま残す（文字として出すときにエスケープされて見える）。
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let semi = rest[1..].find(';').map(|j| j + 1);
        let decoded = semi.and_then(|j| {
            let name = &rest[1..j];
            let c = if let Some(num) = name.strip_prefix('#') {
                let n = match num.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => num.parse::<u32>().ok(),
                }?;
                char::from_u32(n)?
            } else {
                match name {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    "nbsp" => '\u{a0}',
                    "colon" => ':',
                    "sol" => '/',
                    "period" => '.',
                    "Tab" => '\t',
                    "NewLine" => '\n',
                    _ => return None,
                }
            };
            Some((c, j + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `srcset` の候補（URL と、記述子込みの元の文字列）。HTML の読み方に寄せる:
/// URL は空白までで、末尾の `,` は区切り（`data:` の URL の中の `,` は区切りにしない）。
pub(crate) fn srcset_candidates(value: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut url_end = i;
        if value[start..url_end].ends_with(',') {
            while url_end > start && bytes[url_end - 1] == b',' {
                url_end -= 1;
            }
            out.push((&value[start..url_end], &value[start..url_end]));
            continue;
        }
        // 記述子（`2x`・`640w`）は次の `,` まで。
        while i < bytes.len() && bytes[i] != b',' {
            i += 1;
        }
        out.push((&value[start..url_end], value[start..i].trim_end()));
    }
    out
}

struct Attr {
    /// 小文字の名前。
    name: String,
    /// 文字参照を解いた値。値の無い属性は None。
    value: Option<String>,
    /// 前の空白から値の終わりまで。属性を外すときに切り取る範囲。
    span: (usize, usize),
}

struct Tag {
    name: String,
    attrs: Vec<Attr>,
    start: usize,
    /// `>` の次。
    end: usize,
    self_closing: bool,
}

impl Tag {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|a| a.name == name).and_then(|a| a.value.as_deref())
    }

    /// 置き換えたリンクの名前に使う文字。alt → title → aria-label の順で、空でない最初のもの。
    fn caption(&self) -> Option<String> {
        ["alt", "title", "aria-label"]
            .iter()
            .filter_map(|n| self.attr(n))
            .map(|v| v.trim().to_string())
            .find(|v| !v.is_empty())
    }
}

/// `s[start]` が `<` で、次が英字のときにタグを読む。閉じていなければ None。
fn parse_tag(s: &str, start: usize) -> Option<Tag> {
    let b = s.as_bytes();
    let mut i = start + 1;
    if !b.get(i)?.is_ascii_alphabetic() {
        return None;
    }
    while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'/' && b[i] != b'>' {
        i += 1;
    }
    let name = s[start + 1..i].to_ascii_lowercase();
    let mut attrs = Vec::new();
    loop {
        let attr_start = i;
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        if i >= b.len() {
            return None;
        }
        if b[i] == b'>' {
            let self_closing = i > start && b[i - 1] == b'/';
            return Some(Tag { name, attrs, start, end: i + 1, self_closing });
        }
        let name_start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        let attr_name = s[name_start..i].to_ascii_lowercase();
        let mut j = i;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let mut value = None;
        if j < b.len() && b[j] == b'=' {
            j += 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= b.len() {
                return None;
            }
            if b[j] == b'"' || b[j] == b'\'' {
                let q = b[j] as char;
                let close = s[j + 1..].find(q)? + j + 1;
                value = Some(decode_entities(&s[j + 1..close]));
                i = close + 1;
            } else {
                let v_start = j;
                while j < b.len() && !b[j].is_ascii_whitespace() && b[j] != b'>' {
                    j += 1;
                }
                value = Some(decode_entities(&s[v_start..j]));
                i = j;
            }
        }
        attrs.push(Attr { name: attr_name, value, span: (attr_start, i) });
    }
}

/// 名前の付いた要素の閉じタグ（`</name`）を探し、(開始, `>` の次) を返す。
fn find_close(lower: &str, from: usize, name: &str) -> Option<(usize, usize)> {
    let pat = format!("</{name}");
    let mut at = from;
    while let Some(rel) = lower[at..].find(&pat) {
        let p = at + rel;
        let after = p + pat.len();
        // `</videos>` のような別の名前を取り違えない。
        if lower[after..].starts_with(|c: char| c == '>' || c.is_ascii_whitespace()) {
            let gt = lower[after..].find('>')? + after;
            return Some((p, gt + 1));
        }
        at = after;
    }
    None
}

/// 中身を文字として読む要素（中のタグは本物のタグではない）。
const RAW_TEXT: [&str; 7] = ["script", "style", "textarea", "title", "xmp", "noscript", "noembed"];

struct Scanner<'a> {
    src: &'a str,
    /// 閉じタグを大文字小文字に関係なく探すための写し。ASCII だけを小文字にするので、
    /// バイトの位置は `src` と同じ。
    lower: String,
    out: String,
    copied: usize,
    pos: usize,
    /// いま `<a href>` の中か。中ならリンクを入れ子にせず、文字だけのチップにする。
    /// 深さを数えないのは、HTML の `<a>` は入れ子にならない（新しい `<a>` が前のを閉じる）ため。
    /// `<a name>` のような href の無いものは、押しても移動しないので数えない。
    in_link: bool,
    /// いま `<svg>` の中か。中の `<image>` は SVG の `<text>` で置き換える。
    svg_depth: usize,
    /// 名前ごとに、最後に閉じタグを探した (探し始めた位置, 結果)。閉じていないタグが多いと、
    /// 出会うたびに末尾まで探して文書の長さの 2 乗になるので、使い回す（[`Scanner::close_of`]）。
    closes: std::collections::HashMap<String, (usize, Option<(usize, usize)>)>,
}

impl<'a> Scanner<'a> {
    fn new(src: &'a str) -> Self {
        Scanner {
            src,
            lower: src.to_ascii_lowercase(),
            out: String::new(),
            copied: 0,
            pos: 0,
            in_link: false,
            svg_depth: 0,
            closes: std::collections::HashMap::new(),
        }
    }

    fn run(mut self) -> Option<String> {
        while let Some(rel) = self.src[self.pos..].find('<') {
            let p = self.pos + rel;
            let rest = &self.lower[p..];
            if rest.starts_with("<!--") {
                self.pos = rest[4..].find("-->").map_or(self.src.len(), |e| p + 4 + e + 3);
                continue;
            }
            if let Some(name) = rest.strip_prefix("</") {
                let name: String = name.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
                match name.as_str() {
                    "a" => self.in_link = false,
                    "svg" => self.svg_depth = self.svg_depth.saturating_sub(1),
                    _ => {}
                }
                self.pos = p + 2;
                continue;
            }
            let Some(tag) = parse_tag(self.src, p) else {
                self.pos = p + 1;
                continue;
            };
            self.pos = tag.end;
            match tag.name.as_str() {
                n if RAW_TEXT.contains(&n) => {
                    if let Some((_, end)) = self.close_of(tag.end, n) {
                        self.pos = end;
                    }
                }
                "a" => self.in_link = !tag.self_closing && tag.attrs.iter().any(|a| a.name == "href"),
                "svg" if !tag.self_closing => self.svg_depth += 1,
                // `<image>` は HTML の中では `<img>` として読まれる。
                "img" => self.image(&tag, false),
                "image" => self.image(&tag, self.svg_depth > 0),
                "source" => self.source(&tag),
                "picture" => self.picture(&tag),
                "video" => self.media(&tag, Media::Video),
                "audio" => self.media(&tag, Media::Audio),
                "iframe" => self.iframe(&tag),
                _ => {}
            }
        }
        if self.copied == 0 {
            return None;
        }
        self.out.push_str(&self.src[self.copied..]);
        Some(self.out)
    }

    /// `from` から後ろで最初の `</name>`。[`find_close`] の結果を名前ごとに覚えておく。
    ///
    /// 前に `from0 <= from` から探して `p` が見つかったなら、`p >= from` である限り `from` からでも
    /// 最初に見つかるのは `p`。見つからなかったなら、`from` からでも見つからない。
    fn close_of(&mut self, from: usize, name: &str) -> Option<(usize, usize)> {
        if let Some(&(from0, found)) = self.closes.get(name) {
            if from0 <= from && found.is_none_or(|(p, _)| p >= from) {
                return found;
            }
        }
        let found = find_close(&self.lower, from, name);
        self.closes.insert(name.to_string(), (from, found));
        found
    }

    /// `[start, end)` を `with` に差し替える。
    fn splice(&mut self, start: usize, end: usize, with: &str) {
        self.out.push_str(&self.src[self.copied..start]);
        self.out.push_str(with);
        self.copied = end;
        // 差し替えた範囲の中は見ない（置き換えた要素の中身）。
        self.pos = self.pos.max(end);
    }

    /// タグから属性を外したり差し替えたりして書き直す。`set` は (名前, 新しい値か None で外す)。
    fn rewrite_attrs(&mut self, tag: &Tag, set: &[(&str, Option<String>)]) {
        let mut text = String::new();
        let mut at = tag.start;
        for a in &tag.attrs {
            let Some((_, new)) = set.iter().find(|(n, _)| *n == a.name) else { continue };
            text.push_str(&self.src[at..a.span.0]);
            if let Some(v) = new {
                text.push_str(&format!(" {}=\"{}\"", a.name, attr_escape(v)));
            }
            at = a.span.1;
        }
        text.push_str(&self.src[at..tag.end]);
        self.splice(tag.start, tag.end, &text);
    }

    /// 置き換える文字。`<a>` の中なら外側のリンクを活かして、押せない小さなチップにする。
    fn replacement(&self, kind: Media, url: &str, host: &str, caption: Option<String>, svg: Option<&Tag>) -> String {
        if let Some(img) = svg {
            // SVG の中に HTML のリンクを置いても描かれない。同じ位置に SVG の文字で出す。
            let x = img.attr("x").unwrap_or("0");
            let y = img.attr("y").unwrap_or("0");
            let text = format!(
                r#"<text class="md-ext-media" x="{}" y="{}" dominant-baseline="hanging">{}</text>"#,
                attr_escape(x),
                attr_escape(y),
                html_escape(&link_text(kind, host, caption)),
            );
            return if self.in_link {
                text
            } else {
                format!(r#"<a href="{}">{}</a>"#, attr_escape(url), text)
            };
        }
        if self.in_link {
            return format!(
                r#"<span class="md-ext-chip" title="{}">{}</span>"#,
                attr_escape(host),
                html_escape(&chip_text(url, host, caption)),
            );
        }
        format!(
            r#"<a class="md-ext-media" href="{}" title="{}">{}</a>"#,
            attr_escape(url),
            attr_escape(url),
            html_escape(&link_text(kind, host, caption)),
        )
    }

    fn image(&mut self, tag: &Tag, in_svg: bool) {
        let src = if in_svg {
            tag.attr("href").or_else(|| tag.attr("xlink:href"))
        } else {
            tag.attr("src")
        };
        let has_src = src.is_some();
        match classify(src.unwrap_or("")) {
            Src::External { url, host } => {
                let with = self.replacement(Media::Image, &url, &host, tag.caption(), in_svg.then_some(tag));
                self.splice(tag.start, tag.end, &with);
            }
            Src::Blocked => self.splice(tag.start, tag.end, ""),
            Src::Local => {
                let Some(srcset) = tag.attr("srcset").map(str::to_string) else { return };
                let (kept, external, dropped) = filter_srcset(&srcset);
                if !has_src && kept.is_empty() {
                    match external {
                        Some((url, host)) => {
                            let with = self.replacement(Media::Image, &url, &host, tag.caption(), None);
                            self.splice(tag.start, tag.end, &with);
                        }
                        None => self.splice(tag.start, tag.end, ""),
                    }
                } else if dropped {
                    let new = (!kept.is_empty()).then_some(kept);
                    self.rewrite_attrs(tag, &[("srcset", new)]);
                }
            }
        }
    }

    /// `<picture>` と `<video>` / `<audio>` の中の `<source>`。外の候補を外し、残らなければ消す。
    /// 外部の候補しか無い要素は、親（picture・video）の側で丸ごと 1 個に置き換え済み。
    fn source(&mut self, tag: &Tag) {
        if let Some(src) = tag.attr("src") {
            if classify(src) != Src::Local {
                self.splice(tag.start, tag.end, "");
                return;
            }
        }
        let Some(srcset) = tag.attr("srcset").map(str::to_string) else { return };
        let (kept, _, dropped) = filter_srcset(&srcset);
        if kept.is_empty() {
            self.splice(tag.start, tag.end, "");
        } else if dropped {
            self.rewrite_attrs(tag, &[("srcset", Some(kept))]);
        }
    }

    /// 中の `<source>` が いくつあっても 1 個に置き換える。手元の `<img>` が控えにあれば、
    /// 置き換えずに外部の `<source>` だけを外す（手元の画像が出る）。
    fn picture(&mut self, tag: &Tag) {
        let Some((close, end)) = self.close_of(tag.end, "picture") else { return };
        let inner = self.inner_tags(tag.end, close);
        let img = inner.iter().find(|t| t.name == "img");
        if let Some(img) = img {
            if img.attr("src").is_some_and(|s| classify(s) == Src::Local) {
                return;
            }
        }
        let caption = img.and_then(Tag::caption);
        if let Some((url, host)) = first_external(&inner) {
            let with = self.replacement(Media::Image, &url, &host, caption, None);
            self.splice(tag.start, end, &with);
        }
    }

    fn media(&mut self, tag: &Tag, kind: Media) {
        let close = self.close_of(tag.end, &tag.name);
        let elem_end = close.map_or(tag.end, |(_, e)| e);
        let caption = tag.caption();
        match tag.attr("src").map(classify) {
            Some(Src::External { url, host }) => {
                let with = self.replacement(kind, &url, &host, caption, None);
                self.splice(tag.start, elem_end, &with);
                return;
            }
            Some(Src::Blocked) => {
                self.splice(tag.start, elem_end, "");
                return;
            }
            Some(Src::Local) => {}
            None => {
                // src が無ければ中の `<source>` で決まる。手元のものが 1 つも無ければ丸ごと置き換える。
                let inner = close.map(|(c, _)| self.inner_tags(tag.end, c)).unwrap_or_default();
                let sources: Vec<&Tag> = inner.iter().filter(|t| t.name == "source").collect();
                let any_local = sources.iter().any(|t| t.attr("src").is_some_and(|s| classify(s) == Src::Local));
                if !sources.is_empty() && !any_local {
                    match first_external(&inner) {
                        Some((url, host)) => {
                            let with = self.replacement(kind, &url, &host, caption, None);
                            self.splice(tag.start, elem_end, &with);
                        }
                        None => self.splice(tag.start, elem_end, ""),
                    }
                    return;
                }
            }
        }
        // 本体は手元のもの。外の poster だけ外す（読まれず、灰色の枠が残るだけなので）。
        if tag.attr("poster").is_some_and(|p| classify(p) != Src::Local) {
            self.rewrite_attrs(tag, &[("poster", None)]);
        }
    }

    fn iframe(&mut self, tag: &Tag) {
        // iframe の中身は文字として読まれるので、閉じタグまで丸ごと 1 個として扱う。
        let elem_end = self.close_of(tag.end, "iframe").map_or(tag.end, |(_, e)| e);
        match tag.attr("src").map(classify) {
            Some(Src::External { url, host }) => {
                let with = self.replacement(Media::Frame, &url, &host, tag.caption(), None);
                self.splice(tag.start, elem_end, &with);
            }
            Some(Src::Blocked) => self.splice(tag.start, elem_end, ""),
            _ => self.pos = elem_end,
        }
    }

    /// `[from, to)` にある開きタグを全部読む（`<picture>` などの中身を先に覗くため）。
    fn inner_tags(&self, from: usize, to: usize) -> Vec<Tag> {
        let mut out = Vec::new();
        let mut at = from;
        while let Some(rel) = self.src[at..to].find('<') {
            let p = at + rel;
            match parse_tag(self.src, p) {
                Some(t) if t.end <= to => {
                    at = t.end;
                    out.push(t);
                }
                _ => at = p + 1,
            }
        }
        out
    }
}

/// `srcset` から手元の候補だけを残す。(残した値, 最初の外部の URL とホスト, 何か外したか)。
fn filter_srcset(value: &str) -> (String, Option<(String, String)>, bool) {
    let mut kept = Vec::new();
    let mut external = None;
    let mut dropped = false;
    for (url, candidate) in srcset_candidates(value) {
        match classify(url) {
            Src::Local => kept.push(candidate),
            Src::External { url, host } => {
                dropped = true;
                external.get_or_insert((url, host));
            }
            Src::Blocked => dropped = true,
        }
    }
    (kept.join(", "), external, dropped)
}

/// 中のタグ（`<source>` の src / srcset、`<img>` の src / srcset）で最初の外部の URL。
fn first_external(tags: &[Tag]) -> Option<(String, String)> {
    for t in tags.iter().filter(|t| t.name == "source" || t.name == "img") {
        if let Some(Src::External { url, host }) = t.attr("src").map(classify) {
            return Some((url, host));
        }
        if let Some((_, Some(ext), _)) = t.attr("srcset").map(filter_srcset) {
            return Some(ext);
        }
    }
    None
}

/// 「外部の画像: alt（ホスト名）」。名前が無ければ「外部の画像: ホスト名」。
fn link_text(kind: Media, host: &str, caption: Option<String>) -> String {
    match caption {
        Some(c) => format!("{}: {c}（{host}）", kind.label()),
        None => format!("{}: {host}", kind.label()),
    }
}

/// `<a>` の中のチップの文字。alt → title → URL の最後のパス（拡張子を落とす）→ ホスト名。
/// README のバッジは alt が空のことが多く、ホスト名だけだと全部「img.shields.io」になる。
fn chip_text(url: &str, host: &str, caption: Option<String>) -> String {
    if let Some(c) = caption {
        return c;
    }
    let path = url.split(['?', '#']).next().unwrap_or("");
    let after_host = path.splitn(4, '/').nth(3).unwrap_or("");
    let last = after_host.rsplit('/').find(|s| !s.is_empty()).unwrap_or("");
    let last = crate::urlpath::percent_decode(last);
    let stem = match last.rfind('.') {
        Some(i) if i > 0 => &last[..i],
        _ => last.as_str(),
    };
    if stem.is_empty() {
        host.to_string()
    } else {
        stem.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rep(html: &str) -> String {
        replace_external(html).unwrap_or_else(|| html.to_string())
    }

    #[test]
    fn external_image_becomes_an_open_link_with_alt_and_host() {
        let out = rep(r#"<p><img src="https://example.com/a/fig.png" alt="図 1" /></p>"#);
        assert_eq!(
            out,
            r#"<p><a class="md-ext-media" href="https://example.com/a/fig.png" title="https://example.com/a/fig.png">外部の画像: 図 1（example.com）</a></p>"#
        );
    }

    #[test]
    fn without_alt_the_title_then_the_host_is_used() {
        assert!(rep(r#"<img src="https://h.example/x.png" title="題">"#).contains("外部の画像: 題（h.example）"));
        assert!(rep(r#"<img src="https://h.example/x.png" alt="">"#).contains(">外部の画像: h.example</a>"));
    }

    #[test]
    fn local_data_and_blob_images_are_left_alone() {
        for html in [
            r#"<img src="/docs/fig.png">"#,
            r#"<img src="fig.png">"#,
            r#"<img src="/__abs/Users/me/fig.png">"#,
            r#"<img src="data:image/png;base64,AA==">"#,
            r#"<img src="blob:mdpreview://localhost/1">"#,
            r#"<img alt="no src">"#,
        ] {
            assert!(replace_external(html).is_none(), "{html} を書き換えてはいけない");
        }
    }

    #[test]
    fn other_schemes_are_removed_without_a_link() {
        for src in ["javascript:alert(1)", "file:///etc/passwd", "vscode://file/x", "JaVaScRiPt:x"] {
            let out = rep(&format!(r#"<p>a<img src="{src}">b</p>"#));
            assert_eq!(out, "<p>ab</p>", "{src}");
        }
    }

    #[test]
    fn obfuscated_schemes_are_still_seen_as_external() {
        for src in ["HTTPS://example.com/a.png", "https&#58;//example.com/a.png", "ht\ttps://example.com/a.png", " https://example.com/a.png"] {
            let out = rep(&format!(r#"<img src="{src}" alt="x">"#));
            assert!(out.contains("外部の画像: x（example.com）"), "{src}: {out}");
            assert!(!out.contains("<img"), "{src}: {out}");
        }
    }

    #[test]
    fn alt_with_html_is_shown_as_text() {
        let out = rep(r#"<img src="https://e.example/a.png" alt="&lt;b onclick=x&gt;太字&lt;/b&gt;">"#);
        assert!(out.contains("外部の画像: &lt;b onclick=x&gt;太字&lt;/b&gt;（e.example）"), "{out}");
        assert!(!out.contains("<b"), "{out}");
        // 生 HTML の属性に直接書いた `<` も同じ。
        let out = rep(r#"<img src="https://e.example/a.png" alt="<i>x</i>">"#);
        assert!(out.contains("&lt;i&gt;x&lt;/i&gt;"), "{out}");
    }

    #[test]
    fn badge_inside_a_link_becomes_a_chip_and_keeps_the_outer_link() {
        let out = rep(r#"<a href="https://ci.example/run"><img src="https://img.shields.io/badge/build-passing-green.svg" alt=""></a>"#);
        assert_eq!(
            out,
            r#"<a href="https://ci.example/run"><span class="md-ext-chip" title="img.shields.io">build-passing-green</span></a>"#
        );
        // 入れ子のリンクにしない。
        assert_eq!(out.matches("<a ").count(), 1);
        // alt があれば alt。
        let out = rep(r#"<a href="/x"><img src="https://img.shields.io/b.svg" alt="CI"></a>"#);
        assert!(out.contains(r#"title="img.shields.io">CI</span>"#), "{out}");
        // パスが無ければホスト名。
        let out = rep(r#"<a href="/x"><img src="https://badge.example/" alt=""></a>"#);
        assert!(out.contains(">badge.example</span>"), "{out}");
        // `<a>` を出た後は普通のリンクに戻る。
        let out = rep(r#"<a href="/x">t</a><img src="https://e.example/a.png">"#);
        assert!(out.contains(r#"<a class="md-ext-media""#), "{out}");
    }

    #[test]
    fn picture_with_several_external_sources_becomes_one_link() {
        let html = "<picture>\n<source media=\"(prefers-color-scheme: dark)\" srcset=\"https://e.example/dark.png\">\n<source srcset=\"https://e.example/light.png 1x, https://e.example/light@2x.png 2x\">\n<img src=\"https://e.example/light.png\" alt=\"ロゴ\">\n</picture>";
        let out = rep(html);
        assert_eq!(out.matches("md-ext-media").count(), 1, "{out}");
        assert!(out.contains("外部の画像: ロゴ（e.example）"), "{out}");
        assert!(!out.contains("<source") && !out.contains("<img") && !out.contains("picture"), "{out}");
    }

    #[test]
    fn picture_with_a_local_fallback_keeps_it_and_drops_the_external_sources() {
        let html = r#"<picture><source srcset="https://e.example/dark.png"><source srcset="dark.png"><img src="/light.png" alt="ロゴ"></picture>"#;
        assert_eq!(rep(html), r#"<picture><source srcset="dark.png"><img src="/light.png" alt="ロゴ"></picture>"#);
    }

    #[test]
    fn srcset_keeps_only_local_candidates() {
        let out = rep(r#"<img src="/a.png" srcset="/a@2x.png 2x, https://e.example/a@3x.png 3x" alt="a">"#);
        assert_eq!(out, r#"<img src="/a.png" srcset="/a@2x.png 2x" alt="a">"#);
        let out = rep(r#"<img src="/a.png" srcset="https://e.example/a@3x.png 3x">"#);
        assert_eq!(out, r#"<img src="/a.png">"#);
        // src が無く、外部の候補しか無いものはリンクに。
        let out = rep(r#"<img srcset="https://e.example/a.png 1x" alt="a">"#);
        assert!(out.contains("外部の画像: a（e.example）"), "{out}");
        // data: の中の `,` は区切りにしない。
        let out = rep(r#"<img src="/a.png" srcset="data:image/png;base64,AA== 2x, https://e.example/b.png 3x">"#);
        assert_eq!(out, r#"<img src="/a.png" srcset="data:image/png;base64,AA== 2x">"#);
    }

    #[test]
    fn external_iframe_becomes_a_link_and_local_ones_stay() {
        let out = rep(r#"<iframe width="560" src="https://www.youtube.com/embed/x" title="YouTube video player"></iframe>"#);
        assert_eq!(
            out,
            r#"<a class="md-ext-media" href="https://www.youtube.com/embed/x" title="https://www.youtube.com/embed/x">外部の埋め込み: YouTube video player（www.youtube.com）</a>"#
        );
        assert!(replace_external(r#"<iframe src="/page.html"></iframe>"#).is_none());
        assert_eq!(rep(r#"<iframe src="file:///x"></iframe>"#), "");
    }

    #[test]
    fn video_and_audio() {
        let out = rep(r#"<video src="https://e.example/demo.mp4" controls></video>"#);
        assert!(out.starts_with(r#"<a class="md-ext-media" href="https://e.example/demo.mp4""#), "{out}");
        assert!(out.contains("外部の動画: e.example") && !out.contains("video"), "{out}");
        let out = rep("<audio controls>\n<source src=\"https://e.example/a.mp3\">\n<source src=\"https://e.example/a.ogg\">\n</audio>");
        assert_eq!(out.matches("md-ext-media").count(), 1, "{out}");
        assert!(out.contains("外部の音声: e.example") && !out.contains("audio"), "{out}");
        // 手元の動画は残し、外の poster だけ外す。
        assert_eq!(rep(r#"<video src="/clip.mp4" poster="https://e.example/p.png"></video>"#), r#"<video src="/clip.mp4"></video>"#);
        // 手元の source があれば、外部の source だけ外す。
        assert_eq!(
            rep(r#"<video><source src="https://e.example/a.webm"><source src="/a.mp4"></video>"#),
            r#"<video><source src="/a.mp4"></video>"#
        );
    }

    #[test]
    fn svg_image_becomes_svg_text() {
        let out = rep(r#"<svg width="100" height="40"><image href="https://e.example/a.png" x="4" y="8" width="90" height="30"/></svg>"#);
        assert_eq!(
            out,
            r#"<svg width="100" height="40"><a href="https://e.example/a.png"><text class="md-ext-media" x="4" y="8" dominant-baseline="hanging">外部の画像: e.example</text></a></svg>"#
        );
        let out = rep(r#"<svg><image xlink:href="https://e.example/a.png"/></svg>"#);
        assert!(out.contains("<text"), "{out}");
    }

    #[test]
    fn text_contexts_are_not_rewritten() {
        for html in [
            r#"<!-- <img src="https://e.example/a.png"> -->"#,
            r#"<style>/* <img src="https://e.example/a.png"> */</style>"#,
            r#"<textarea><img src="https://e.example/a.png"></textarea>"#,
            r#"<pre><code>&lt;img src="https://e.example/a.png"&gt;</code></pre>"#,
        ] {
            assert!(replace_external(html).is_none(), "{html}");
        }
    }

    #[test]
    fn unquoted_and_uppercase_attributes() {
        let out = rep(r#"<IMG SRC=https://e.example/a.png ALT=x>"#);
        assert!(out.contains("外部の画像: x（e.example）"), "{out}");
    }

    #[test]
    fn host_drops_userinfo_and_keeps_the_port() {
        assert_eq!(
            classify("https://user:pw@Example.COM:8443/a"),
            Src::External { url: "https://user:pw@example.com:8443/a".into(), host: "example.com:8443".into() }
        );
        assert_eq!(classify("https://"), Src::Blocked);
    }

    #[test]
    fn the_link_gets_a_normalised_url() {
        for (src, url, host) in [
            ("HTTPS://Example.COM/A.png", "https://example.com/A.png", "example.com"),
            ("https://example.com", "https://example.com/", "example.com"),
            ("http://example.com:80/a", "http://example.com/a", "example.com"),
            ("https://example.com:443/a?q=1#f", "https://example.com/a?q=1#f", "example.com"),
            ("https:\\\\example.com\\a\\b.png?x=\\", "https://example.com/a/b.png?x=\\", "example.com"),
        ] {
            assert_eq!(classify(src), Src::External { url: url.into(), host: host.into() }, "{src}");
        }
        let out = rep(r#"<img src="HTTPS://Example.COM/a.png">"#);
        assert!(out.contains(r#"href="https://example.com/a.png""#), "{out}");
    }

    #[test]
    fn protocol_relative_urls_are_external() {
        for src in ["//cdn.example/x.png", "\\\\cdn.example/x.png", "/\\cdn.example/x.png", " //cdn.example/x.png"] {
            assert_eq!(
                classify(src),
                Src::External { url: "https://cdn.example/x.png".into(), host: "cdn.example".into() },
                "{src}"
            );
        }
        let out = rep(r#"<img src="//cdn.example/x.png" alt="図">"#);
        assert!(out.contains(r#"href="https://cdn.example/x.png""#) && out.contains("外部の画像: 図（cdn.example）"), "{out}");
        // `/` 1 つは手元。
        assert_eq!(classify("/cdn.example/x.png"), Src::Local);
    }

    #[test]
    fn only_the_apps_own_mdpreview_host_is_local() {
        assert_eq!(classify("mdpreview://localhost/a.png"), Src::Local);
        assert_eq!(classify("MDPREVIEW://LocalHost/a.png"), Src::Local);
        assert_eq!(classify("mdpreview://other-host/a.png"), Src::Blocked);
    }

    #[test]
    fn an_anchor_without_href_is_not_a_link() {
        // 閉じていない `<a name>` の後の外部の画像は、チップではなく開くリンクになる。
        let out = rep(r#"<p><a name="top">見出し<img src="https://e.example/a.png" alt="図"></p>"#);
        assert!(out.contains(r#"<a class="md-ext-media" href="https://e.example/a.png""#), "{out}");
        assert!(!out.contains("md-ext-chip"), "{out}");
        // 新しい `<a href>` は前の `<a name>` を閉じて、その中はリンクの中。
        let out = rep(r#"<a name="x"><a href="/y"><img src="https://e.example/a.png" alt="図"></a>"#);
        assert!(out.contains(r#"<span class="md-ext-chip""#), "{out}");
    }

    #[test]
    fn many_unclosed_tags_do_not_rescan_to_the_end() {
        // 閉じタグを探して毎回末尾まで読むと 2 乗になり、この大きさで数十秒かかる。
        let mut html = String::new();
        for i in 0..20_000 {
            html.push_str(&format!(
                "<video src=\"/v{i}.mp4\"><audio src=\"/a.mp3\"><iframe src=\"/f.html\"><picture><textarea><p>{i}</p>\n"
            ));
        }
        html.push_str(r#"<img src="https://e.example/last.png">"#);
        let start = std::time::Instant::now();
        let out = rep(&html);
        assert!(start.elapsed() < std::time::Duration::from_secs(2), "{:?}", start.elapsed());
        assert_eq!(out.matches("<video").count(), 20_000);
        assert!(out.contains("外部の画像: e.example"));
    }
}
