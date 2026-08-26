//! `src/assets/sample.md`（機能ショーケース）が実際に機能しているかを守るテスト。
//!
//! sample.md はドキュメント群の中で唯一「動く実例」で、ファイルリンクのコード埋め込みは
//! 参照先の実パスに依存する。README / SKILL.md の同じ例はコードブロックやインライン
//! コードの中なので壊れないが、sample.md だけは sample.md 自身か参照先が動いた瞬間に
//! 静かに壊れる。埋め込みが展開されず普通のリンクに落ちるだけで、エラーは出ないためだ。
//! 実際に `src/sample.md` → `src/assets/sample.md` の移動で参照先が全滅した。
//!
//! 期待値は sample.md 自身から導出するので、埋め込みの例を増やしても追従する。
//! 「引用した範囲がラベルの説明と合っているか」は機械では判定できないので見ていない
//! （`[MD_OPTIONS の宣言](../html.rs#L135)` の L135 が本当に MD_OPTIONS かは人が見る）。

use std::path::{Path, PathBuf};

use md_preview::request::{is_renderable, render_file, ViewMode};

fn sample_md() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/assets/sample.md")
}

/// sample.md 内のリンク 1 本。
struct Link {
    /// sample.md 内の行番号（1 始まり）。落ちたときに直す場所がすぐ判るように持つ。
    line: usize,
    /// `](...)` の中身そのまま。
    dest: String,
    /// 行全体がこのリンクだけで出来ているか（＝段落に単独で置かれている）。
    standalone: bool,
}

impl Link {
    /// `#` より前のパス部分。
    fn path(&self) -> &str {
        self.dest.split('#').next().unwrap_or("")
    }

    /// `#L10-L20` の開始行・終端行。行指定として読めなければ None。
    fn range(&self) -> Option<(usize, usize)> {
        let frag = self.dest.split_once('#')?.1;
        let rest = frag.strip_prefix('L').or_else(|| frag.strip_prefix('l'))?;
        let (a, b) = match rest.split_once('-') {
            Some((a, b)) => (a, Some(b)),
            None => (rest, None),
        };
        let start: usize = a.parse().ok()?;
        let end = match b {
            None => start,
            Some(b) => {
                let b = b.strip_prefix('L').or_else(|| b.strip_prefix('l')).unwrap_or(b);
                b.parse().ok()?
            }
        };
        Some((start, end))
    }

    /// コード埋め込みに展開されるはずのリンクか（`embed.rs` の受け入れ条件と揃える）。
    fn should_embed(&self) -> bool {
        // 単独行でなければ普通のリンクのまま。
        if !self.standalone {
            return false;
        }
        // 範囲なしの md / html はペイン内遷移のナビゲーションなので埋め込まない。
        self.range().is_some() || !is_renderable(Path::new(self.path()))
    }
}

/// sample.md からローカルファイルへのリンクだけを拾う。外部 URL・見出しアンカー・
/// コードブロックの中の例は対象外（コードブロックはそもそもリンクにならない）。
fn local_links(md: &str) -> Vec<Link> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for (i, raw) in md.lines().enumerate() {
        let line = raw.trim();
        // フェンスの内側は描画されないので見ない（``` も ```` も開始と同じ印で閉じる）。
        if line.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        for (dest, span) in link_dests(raw) {
            let d = dest.trim();
            let is_local = !d.is_empty()
                && !d.starts_with('#')
                && !d.starts_with("//")
                && !d.contains("://")
                && !d.starts_with("mailto:");
            if !is_local {
                continue;
            }
            out.push(Link {
                line: i + 1,
                dest: d.to_string(),
                // 行全体がこのリンク 1 本だけ（前後に本文が無い）なら単独段落。
                standalone: span.0 == 0 && span.1 == raw.trim_end().len() && raw.starts_with('['),
            });
        }
    }
    out
}

/// 1 行から `[label](dest)` の dest と、リンク全体が占める範囲を取り出す。
/// インラインコード（`` `[a](./b)` ``）の中は描画されないので飛ばす。
fn link_dests(line: &str) -> Vec<(String, (usize, usize))> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut in_code = false;
    while i < b.len() {
        match b[i] {
            b'`' => in_code = !in_code,
            b'[' if !in_code => {
                let start = i;
                // `](` を探す。ラベル内の `]` は sample.md では使っていない。
                if let Some(close) = line[i..].find("](") {
                    let open = i + close + 2;
                    if let Some(end) = line[open..].find(')') {
                        out.push((line[open..open + end].to_string(), (start, open + end + 1)));
                        i = open + end + 1;
                        continue;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// sample.md が参照しているローカルファイルが実在し、行範囲もファイルに収まっていること。
///
/// これが落ちたら sample.md か参照先が移動している。sample.md からの相対パスを直す。
#[test]
fn sample_md_local_links_all_resolve() {
    let md = std::fs::read_to_string(sample_md()).expect("sample.md が読めない");
    let base = sample_md().parent().unwrap().to_path_buf();
    let links = local_links(&md);
    assert!(!links.is_empty(), "ローカルリンクを 1 本も拾えていない（パーサ側の壊れ）");

    for l in &links {
        let target = base.join(l.path());
        let target = target
            .canonicalize()
            .unwrap_or_else(|_| panic!("sample.md:{} の参照先が無い: {}", l.line, l.dest));
        assert!(target.is_file(), "sample.md:{} がファイル以外を指している: {}", l.line, l.dest);

        // 行範囲を書いたなら、その行が実在すること（範囲外は静かに埋め込まれなくなる）。
        if let Some((start, _)) = l.range() {
            let total = std::fs::read_to_string(&target).map(|s| s.lines().count()).unwrap_or(0);
            assert!(
                start <= total,
                "sample.md:{} の開始行がファイル外: {} （{} は全 {} 行）",
                l.line,
                l.dest,
                l.path(),
                total
            );
        }
    }
}

/// 単独行に置いた埋め込み対象のリンクが、実際にコード埋め込みとして描画されること。
///
/// パス解決だけでなく、描画経路（`render_file` → `html` → `embed`）まで通して確認する。
#[test]
fn sample_md_standalone_links_render_as_embeds() {
    let path = sample_md();
    let md = std::fs::read_to_string(&path).expect("sample.md が読めない");
    let rendered = render_file(&path, "sample.md", ViewMode::Normal).expect("描画できない");

    let links = local_links(&md);
    let expected = links.iter().filter(|l| l.should_embed()).count();
    assert!(expected > 0, "埋め込まれるはずのリンクが sample.md から拾えていない");

    let actual = rendered.html.matches("code-wrapper has-filename code-embed").count();
    assert_eq!(
        actual, expected,
        "コード埋め込みの数が合わない（展開されずリンクに落ちたものがある）"
    );

    // 埋め込まれたリンクは href として消え、埋め込まない約束のリンク（文中のリンク・
    // 範囲なしの md）は href のまま残る。同じリンク先が単独行と文中の両方に出ることが
    // あるので（`../lib.rs`）、リンク先ごとに「残るはずの本数」と数を突き合わせる。
    let mut dests: Vec<&str> = links.iter().map(|l| l.dest.as_str()).collect();
    dests.sort_unstable();
    dests.dedup();
    for dest in dests {
        let want = links.iter().filter(|l| l.dest == dest && !l.should_embed()).count();
        let got = rendered.html.matches(&format!("href=\"{}\"", dest)).count();
        let lines: Vec<String> = links
            .iter()
            .filter(|l| l.dest == dest)
            .map(|l| l.line.to_string())
            .collect();
        assert_eq!(
            got,
            want,
            "{} のリンクとして残った数が合わない（sample.md:{} 付近）",
            dest,
            lines.join(",")
        );
    }
}
