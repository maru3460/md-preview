//! 段落に単独で置かれたローカルファイルへのリンクを、その中身のコード埋め込みに
//! 展開する（GitHub のパーマリンク埋め込みのローカル版）。
//!
//! `[main](./src/main.rs#L10-L20)` のようにリンクだけの段落が対象で、文中のリンクは
//! これまで通り普通のリンクとして描画する。参照先はローカルファイルのみを読み、
//! ネットワークアクセスは一切行わない。

use std::path::Path;

use pulldown_cmark::{Event, Tag, TagEnd};

use crate::html::html_escape;
use crate::request::{extension_to_hljs_lang, percent_decode};

/// これを超える行数の埋め込みは畳んでおき、展開ボタンを付ける。
/// 100 行を超える引用が本文に居座ると読み物として重いので、そこから先は畳む。
/// 畳んだときに見せる高さ自体は base.css の `.code-embed.is-foldable > pre`（40 行ぶん）。
const COLLAPSE_LINES: usize = 100;

/// 1 つの埋め込みに展開する最大行数。折りたたみを開いてもここで打ち切る。
/// これを超える長さは、本文に引用するのではなくリンクで飛ばすべきという線引き。
const MAX_EMBED_LINES: usize = 1000;

/// 埋め込むコード本文の上限バイト数。行数上限とは別に、1 行が極端に長いケースを抑える。
const MAX_EMBED_BYTES: usize = 256 * 1024;

/// これを超えるファイルはそもそも読まない。巨大ファイルを丸ごとメモリに載せない
/// ための門番で、超えたら埋め込みを諦めて普通のリンクに戻す。
const MAX_READ_BYTES: u64 = 8 * 1024 * 1024;

/// リンク先フラグメントで指定された行範囲（1 始まり・両端含む）。
struct LineSpec {
    start: usize,
    /// `#L10` のように終端を書かなかった場合は None（開始行のみ）。
    end: Option<usize>,
}

/// 段落が「単独のリンク」だけで構成されていれば、その参照先のコード埋め込み HTML を返す。
/// 埋め込めない場合は None を返し、呼び出し側で普通の段落として描画させる。
///
/// `attrs` は外枠 div に差し込む属性文字列（コメント機能の `data-src-line`）。
/// コードブロックや表と同じく、埋め込み全体を 1 ユニットとしてコメント対象にする。
/// `uid` は折りたたみ用チェックボックスの id に使う、ページ内で一意な番号（段落の開始行）。
pub fn try_embed(base_dir: &Path, para: &[Event], attrs: &str, uid: usize) -> Option<String> {
    if para.len() < 2 {
        return None;
    }
    let Event::Start(Tag::Link { dest_url, .. }) = &para[0] else {
        return None;
    };
    if !matches!(para[para.len() - 1], Event::End(TagEnd::Link)) {
        return None;
    }
    // 途中にリンク終端があれば「リンクが2つ以上並んだ段落」なので対象外。
    if para[1..para.len() - 1]
        .iter()
        .any(|e| matches!(e, Event::End(TagEnd::Link)))
    {
        return None;
    }
    render_embed(base_dir, dest_url, attrs, uid)
}

fn render_embed(base_dir: &Path, dest: &str, attrs: &str, uid: usize) -> Option<String> {
    let (rel_path, spec) = parse_dest(dest)?;

    // 範囲指定なしの md / html リンクは「プレビューペイン内で遷移するナビゲーション」
    // として既に意味を持っている（`[次へ](./other.md)`）。単独行に置かれることが多い
    // 用法なので、埋め込みに奪わずリンクのまま残す。`#L10` を明示した場合だけ展開する。
    if spec.is_none() && is_navigable_page(Path::new(&rel_path)) {
        return None;
    }

    // `..` やシンボリックリンクを畳んで実体を得る。ローカル閲覧専用で外部への
    // 送信経路が無いため、base_dir の外へ出る参照（`../src/x.rs`）も許可する。
    let file = base_dir.join(&rel_path).canonicalize().ok()?;
    let meta = std::fs::metadata(&file).ok()?;
    if !meta.is_file() || meta.len() > MAX_READ_BYTES {
        return None;
    }
    // 非 UTF-8（バイナリ）は埋め込まず普通のリンクに戻す。
    let text = std::fs::read_to_string(&file).ok()?;

    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    if total == 0 {
        return None;
    }

    let start = match &spec {
        // 実ファイルの行数を超える開始行は指定ミスなので埋め込まない。
        Some(s) if s.start > total => return None,
        Some(s) => s.start,
        None => 1,
    };
    // 書き手が読ませたかった終端（intent）と、上限を適用した終端を分けて持つ。
    // 省略の告知は「intent より短くなったとき」だけ出したいため。
    let intent_end = match &spec {
        Some(s) => s.end.unwrap_or(s.start).max(s.start).min(total),
        None => total,
    };
    // 行数のハード上限。範囲を明示していても、ここより先は読ませない。
    let requested_end = intent_end.min(start + MAX_EMBED_LINES - 1);

    // 上限バイトに収まるところまで行を詰める。1 行目は必ず含める。
    let mut bytes = 0usize;
    let mut end = start;
    for i in start..=requested_end {
        bytes += lines[i - 1].len() + 1;
        if bytes > MAX_EMBED_BYTES && i > start {
            break;
        }
        end = i;
    }

    let lines_label = if end < intent_end {
        // 上限で頭を切ったので、総行数を添えて省略を明示する。
        format!("L{}-L{} / 全 {} 行", start, end, total)
    } else if spec.is_none() {
        // 範囲指定なし＝ファイル全体。行範囲より「全 N 行」の方が情報量がある。
        format!("全 {} 行", total)
    } else if start == end {
        format!("L{}", start)
    } else {
        format!("L{}-L{}", start, end)
    };

    // 長い埋め込みは畳んでおく。展開はチェックボックス + label の CSS だけで動くので
    // JS を介さず、`md --html` のダンプでもそのまま開閉できる。id はページ内で一意。
    // 兄弟セレクタで pre と label の両方を切り替えるため、input は pre より前に置く。
    let shown_lines = end - start + 1;
    let (fold_class, fold_input, fold_label) = if shown_lines > COLLAPSE_LINES {
        (
            " is-foldable",
            format!(r#"<input class="code-embed-fold" type="checkbox" id="code-embed-{uid}">"#),
            format!(
                r#"<label class="code-embed-expand" for="code-embed-{uid}"><span class="on-collapsed">すべて表示（{shown_lines} 行）</span><span class="on-expanded">折りたたむ</span></label>"#
            ),
        )
    } else {
        ("", String::new(), String::new())
    };

    let shown = rel_path.strip_prefix("./").unwrap_or(&rel_path);
    Some(format!(
        r#"<div class="code-wrapper has-filename code-embed{fold_class}"{attrs}><div class="code-filename"><span class="code-embed-path">{path}</span><span class="code-embed-lines">{lines}</span></div>{fold_input}<pre><code class="language-{lang}">{code}</code></pre>{fold_label}</div>"#,
        fold_class = fold_class,
        attrs = attrs,
        fold_input = fold_input,
        fold_label = fold_label,
        path = html_escape(shown),
        lines = html_escape(&lines_label),
        lang = extension_to_hljs_lang(&file),
        code = html_escape(&lines[start - 1..end].join("\n")),
    ))
}

/// リンク先を「ローカルパス」と「行範囲」に分解する。
/// URL・mailto・アンカーのみ・行指定として読めないフラグメントは None（＝埋め込まない）。
fn parse_dest(dest: &str) -> Option<(String, Option<LineSpec>)> {
    let dest = dest.trim();
    if dest.is_empty() || dest.starts_with('#') || dest.starts_with("//") || has_scheme(dest) {
        return None;
    }
    let (path_part, frag) = match dest.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (dest, None),
    };
    if path_part.is_empty() {
        return None;
    }
    let spec = match frag {
        None => None,
        // 見出しアンカー（`#section`）は「リンク」であって埋め込み指定ではないので、
        // 行指定として読めないフラグメントが付いていたら埋め込まない。
        Some(f) => Some(parse_line_spec(f)?),
    };
    Some((percent_decode(path_part), spec))
}

/// `L10` / `L10-L20` / `L10-20` を解釈する（`L` の大文字小文字は問わない）。
fn parse_line_spec(frag: &str) -> Option<LineSpec> {
    let rest = strip_l(frag)?;
    let (start_s, end_s) = match rest.split_once('-') {
        Some((a, b)) => (a, Some(b)),
        None => (rest, None),
    };
    let start: usize = start_s.parse().ok()?;
    if start == 0 {
        return None;
    }
    let end = match end_s {
        None => None,
        Some(b) => {
            let n: usize = strip_l(b).unwrap_or(b).parse().ok()?;
            if n == 0 {
                return None;
            }
            Some(n)
        }
    };
    Some(LineSpec { start, end })
}

fn strip_l(s: &str) -> Option<&str> {
    s.strip_prefix('L').or_else(|| s.strip_prefix('l'))
}

/// md-preview が「ページとして描画する」拡張子か（folder.js の isRenderablePath と同じ線引き）。
/// これらへのリンクはプレビューペイン内遷移に使われるため、埋め込み判定で特別扱いする。
fn is_navigable_page(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("md" | "markdown" | "html" | "htm")
    )
}

/// `https:` や `mailto:` のようなスキーム付き URL か。ローカルパスと区別するために使う。
/// `./a:b.rs` のような「コロン入りのローカルパス」を誤判定しないよう、コロンより前が
/// スキームとして妥当な文字並び（英字始まり + 英数と `+-.`）かどうかで見る。
fn has_scheme(s: &str) -> bool {
    let Some(colon) = s.find(':') else {
        return false;
    };
    let head = &s[..colon];
    head.starts_with(|c: char| c.is_ascii_alphabetic())
        && head.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `parse_dest` の結果を比較しやすい `パス|範囲` 形式にする（範囲なしは `all`）。
    fn spec(dest: &str) -> Option<String> {
        let (path, s) = parse_dest(dest)?;
        let range = match s {
            None => "all".to_string(),
            Some(LineSpec { start, end: None }) => start.to_string(),
            Some(LineSpec { start, end: Some(e) }) => format!("{}-{}", start, e),
        };
        Some(format!("{}|{}", path, range))
    }

    #[test]
    fn parses_local_paths_and_ranges() {
        assert_eq!(spec("./src/main.rs").unwrap(), "./src/main.rs|all");
        assert_eq!(spec("./src/main.rs#L10").unwrap(), "./src/main.rs|10");
        assert_eq!(spec("./a.rs#L10-L20").unwrap(), "./a.rs|10-20");
        assert_eq!(spec("./a.rs#l10-20").unwrap(), "./a.rs|10-20");
        // 空白入りのパスは percent-encode されて届くのでデコードする
        assert_eq!(spec("my%20file.rs#L1").unwrap(), "my file.rs|1");
    }

    #[test]
    fn rejects_non_embeddable_destinations() {
        assert_eq!(spec("https://github.com/"), None);
        assert_eq!(spec("mailto:a@b.c"), None);
        assert_eq!(spec("//example.com/x.rs"), None);
        assert_eq!(spec("#section"), None);
        assert_eq!(spec(""), None);
        // 行指定として読めないフラグメントは「見出しへのリンク」なので埋め込まない
        assert_eq!(spec("./other.md#heading"), None);
        assert_eq!(spec("./a.rs#L0"), None);
        assert_eq!(spec("./a.rs#"), None);
    }

    /// 一時ディレクトリに行数だけ決めたファイルを置き、埋め込み結果を得る。
    fn embed_with(lines: usize, dest_frag: &str) -> Option<String> {
        let dir = std::env::temp_dir().join(format!("md-embed-test-{}-{}", lines, dest_frag.len()));
        std::fs::create_dir_all(&dir).unwrap();
        let body: String = (1..=lines).map(|i| format!("line {}\n", i)).collect();
        std::fs::write(dir.join("f.txt"), body).unwrap();
        render_embed(&dir, &format!("./f.txt{}", dest_frag), "", 1)
    }

    #[test]
    fn line_label_marks_truncation_only_when_cut() {
        // 単一行指定は「省略」ではないので総行数を添えない
        assert!(embed_with(14, "#L5").unwrap().contains(">L5<"));
        // 範囲指定はそのまま範囲を出す
        assert!(embed_with(14, "#L8-L12").unwrap().contains(">L8-L12<"));
        // 範囲指定なし＝全体は「全 N 行」
        assert!(embed_with(3, "").unwrap().contains("全 3 行"));
        // 行数のハード上限で切ったときだけ省略告知つき
        let over = MAX_EMBED_LINES + 100;
        let big = embed_with(over, "").unwrap();
        assert!(
            big.contains(&format!("L1-L{} / 全 {} 行", MAX_EMBED_LINES, over)),
            "{big:.300}"
        );
    }

    #[test]
    fn long_embeds_are_foldable_and_short_ones_are_not() {
        // 閾値ちょうどは畳まない
        let short = embed_with(COLLAPSE_LINES, "").unwrap();
        assert!(!short.contains("is-foldable"), "{short:.300}");
        assert!(!short.contains("code-embed-fold"), "{short:.300}");

        // 超えたら畳んで開閉バーを付ける
        let long = embed_with(COLLAPSE_LINES + 1, "").unwrap();
        assert!(long.contains("is-foldable"), "{long:.300}");
        assert!(long.contains(r#"type="checkbox" id="code-embed-1""#), "{long:.300}");
        assert!(long.contains(r#"for="code-embed-1""#), "{long:.300}");
        assert!(
            long.contains(&format!("すべて表示（{} 行）", COLLAPSE_LINES + 1)),
            "{long:.300}"
        );
        // 兄弟セレクタが効くよう input は pre より前、label は後ろに置く
        let input_at = long.find("code-embed-fold").unwrap();
        let pre_at = long.find("<pre>").unwrap();
        let label_at = long.find("code-embed-expand").unwrap();
        assert!(input_at < pre_at && pre_at < label_at, "{long:.300}");
    }

    #[test]
    fn hard_cap_applies_to_explicit_ranges_too() {
        // 範囲を明示しても MAX_EMBED_LINES で打ち切る
        let html = embed_with(3000, &format!("#L1-L{}", 2500)).unwrap();
        let shown = html.matches('\n').count() + 1;
        assert!(shown <= MAX_EMBED_LINES, "emitted {shown} lines");
        assert!(html.contains(&format!("L1-L{} / 全 3000 行", MAX_EMBED_LINES)), "{html:.300}");
    }

    #[test]
    fn embeds_requested_lines_only() {
        let html = embed_with(20, "#L3-L5").unwrap();
        assert!(html.contains("line 3\nline 4\nline 5</code>"), "{html}");
        assert!(!html.contains("line 2"), "{html}");
    }

    #[test]
    fn start_line_past_eof_is_not_embedded() {
        assert!(embed_with(5, "#L99").is_none());
    }

    /// `[次へ](./other.md)` のようなペイン内遷移リンクを埋め込みに奪わないこと。
    #[test]
    fn navigable_page_links_stay_links_without_a_range() {
        let dir = std::env::temp_dir().join("md-embed-nav-test");
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["other.md", "page.html", "note.txt"] {
            std::fs::write(dir.join(name), "one\ntwo\nthree\n").unwrap();
        }
        // 範囲なしの md / html は遷移リンクのまま
        assert!(render_embed(&dir, "./other.md", "", 1).is_none());
        assert!(render_embed(&dir, "./page.html", "", 1).is_none());
        // 行範囲を明示すれば md / html も展開する（`#L2` は見出しアンカーではない）
        assert!(render_embed(&dir, "./other.md#L2", "", 1).is_some());
        assert!(render_embed(&dir, "./page.html#L1-L2", "", 1).is_some());
        // ページとして描画されない拡張子は範囲なしでも展開する
        assert!(render_embed(&dir, "./note.txt", "", 1).is_some());
    }

    #[test]
    fn colon_in_filename_is_not_a_scheme() {
        assert!(!has_scheme("./a:b.rs"));
        assert!(!has_scheme("src/main.rs"));
        assert!(has_scheme("https://x"));
        assert!(has_scheme("mailto:x"));
    }
}
