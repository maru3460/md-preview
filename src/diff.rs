use std::path::Path;
use std::process::Command;

use crate::html::html_escape;

/// ファイルの `git diff HEAD`（最後のコミットからの変更）を、VSCode 風のインライン
/// ソース差分（1カラムの +/-、変わった行＋その行内の変わった文字を強調）として
/// HTML フラグメントに変換して返す。返すのは中身だけ（`.markdown-body` ラッパは
/// 呼び出し側で付ける）。
///
/// レンダリング結果ではなく「ソース」を見せるので、.md 以外のあらゆるテキストファイル
/// に対応する。変更が無ければハイライト無しでソースをそのまま出す（余計な通知は出さない）。
/// 外部コマンド（git）実行のみで依存追加はしていない。
pub fn render_diff_inner(file_path: &Path) -> String {
    let new_src = match std::fs::read(file_path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => return message_html("バイナリファイルは差分表示できません"),
        },
        Err(_) => return message_html("ファイルを読み込めませんでした"),
    };
    let dir = file_path.parent().unwrap_or(Path::new("."));

    let old_src = match git_show_head(dir, file_path) {
        Some(s) => s,
        None => {
            if inside_work_tree(dir) {
                // 未追跡 / まだコミット前: 差分の相手がいないので全行が追加。
                String::new()
            } else {
                // リポジトリ外: 差分の相手がいないので、そのままソースを出す（差分無し）。
                new_src.clone()
            }
        }
    };

    let inner = render_source_diff(&old_src, &new_src);
    if inner.is_empty() {
        // 空ファイル等。何も出さない。
        return String::new();
    }
    // ソースビュー（.md 以外の表示）と同じファイル名バーを付けて見た目を揃える。
    // 右ラベルは差分モードであることを示す "Diff"（変更行数バッジは右下トグル側）。
    let name = file_path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    format!(
        r#"<div class="source-view"><div class="source-titlebar"><span class="source-fname">{}</span><span class="source-lang">Diff</span></div>{}</div>"#,
        html_escape(name),
        inner
    )
}

/// HEAD にあるそのファイルの内容を取り出す。未追跡 / リポジトリ外 / コミット前などで
/// 取れなければ None。`HEAD:./<name>` は -C のディレクトリを prefix として解決される
/// ので、サブディレクトリのファイルでも正しく引ける。
///
/// セキュリティ: 他人のリポジトリを開く脅威モデルでも、ここで使うのは blob ダンプ
/// （`show <rev>:<path>`）と `rev-parse` のみで、いずれも .output()（パイプ）実行。
/// pager は tty 時のみ・textconv/external diff・hooks は経由しないため、悪性 .git/config
/// による既知ベクタは発火しない。将来 `git diff` 等へ広げる場合は要再評価。
fn git_show_head(dir: &Path, file_path: &Path) -> Option<String> {
    let name = file_path.file_name()?.to_str()?;
    let spec = format!("HEAD:./{}", name);
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .arg("show")
        .arg(&spec)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        None
    }
}

fn inside_work_tree(dir: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true")
        .unwrap_or(false)
}

fn message_html(msg: &str) -> String {
    crate::request::notice_html(msg)
}

/// トグルボタンのバッジ用に、追加行数・削除行数だけを軽量に数える。差分本文は組まず
/// `git diff --numstat` に任せる（未追跡ファイルは numstat に出ないので全行を追加として数える）。
pub fn diff_stat(file_path: &Path) -> (usize, usize) {
    let dir = file_path.parent().unwrap_or(Path::new("."));
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        // --no-ext-diff / --no-textconv の理由は changed_files のコメント参照。
        .args(["diff", "--numstat", "--no-ext-diff", "--no-textconv", "HEAD", "--"])
        .arg(file_path)
        .output();

    if let Ok(o) = output {
        if o.status.success() {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if let Some(line) = stdout.lines().next() {
                // 形式: "<added>\t<deleted>\t<path>"。バイナリは "-\t-\t..." で parse 失敗 → 0。
                let mut fields = line.split('\t');
                let add = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let del = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                return (add, del);
            }
            // 空 = 変更なし。ただし未追跡なら diff 表示と同様に全行を追加として数える。
            if inside_work_tree(dir) && !is_tracked(dir, file_path) {
                return (count_lines(file_path), 0);
            }
        }
    }
    (0, 0)
}

/// 変更のあるファイルを列挙する上限。⌘P の並べ替えに使うだけなので、巨大な変更セット
/// （生成物を大量にコミットし直した時など）で git 出力の走査に時間を使わないよう打ち切る。
const CHANGED_MAX: usize = 500;

/// `root` 以下の「変更のあるファイル」を、追加/削除行数つきで root 相対パスで列挙する。
/// ファイル検索パレット（⌘P）が変更のあるファイルを先に出すために使う。
/// リポジトリ外・git が無い場合は空。削除されたファイルは開けないので含めない。
///
/// セキュリティ: 他人のリポジトリを開く脅威モデルなので、`--no-ext-diff` / `--no-textconv`
/// で .git/config・.gitattributes 由来の外部コマンド実行経路を封じる（diff_stat も同様）。
pub fn changed_files(root: &Path) -> Vec<(String, usize, usize)> {
    let mut out: Vec<(String, usize, usize)> = Vec::new();

    // 追跡済みの変更。`--relative` で root 相対のパスにする（root がリポジトリの
    // サブディレクトリでも正しい）。リネームは分解して出す（`old => new` を扱わない）。
    if let Ok(o) = git_output(
        root,
        &[
            "diff", "--numstat", "--relative", "--no-renames",
            "--no-ext-diff", "--no-textconv", "HEAD", "--", ".",
        ],
    ) {
        for line in o.lines() {
            if out.len() >= CHANGED_MAX {
                return out;
            }
            // 形式: "<added>\t<deleted>\t<path>"。バイナリは "-\t-\t..." で parse 失敗 → 0。
            let mut fields = line.split('\t');
            let add = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let del = fields.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            if let Some(path) = fields.next() {
                push_changed(&mut out, root, path, add, del);
            }
        }
    }

    // 未追跡ファイル（.gitignore は尊重する）。numstat には出ないので全行を追加として数える。
    if let Ok(o) = git_output(root, &["ls-files", "--others", "--exclude-standard", "--", "."]) {
        for path in o.lines() {
            if out.len() >= CHANGED_MAX {
                return out;
            }
            let add = untracked_added_lines(&root.join(path));
            push_changed(&mut out, root, path, add, 0);
        }
    }

    out
}

/// core.quotepath=false で git を実行し、成功時のみ stdout を返す。マルチバイトの
/// ファイル名を `\346\227\245` 形式にされると root 相対パスとして使えないため。
fn git_output(dir: &Path, args: &[&str]) -> Result<String, ()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.quotepath=false"])
        .args(args)
        .output()
        .map_err(|_| ())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(())
    }
}

/// 一覧に 1 件足す。開けないもの（削除済み・ディレクトリ）と、C 形式でクオートされた
/// パスは捨てる。クオートは制御文字を含む名前だけで起きる（quotepath=false 済み）ので、
/// アンクオートを実装するより落とす方が安全。
fn push_changed(out: &mut Vec<(String, usize, usize)>, root: &Path, path: &str, add: usize, del: usize) {
    if path.is_empty() || path.starts_with('"') {
        return;
    }
    if !root.join(path).is_file() {
        return;
    }
    out.push((path.to_string(), add, del));
}

/// 未追跡ファイルの追加行数。一覧では数百ファイルを数えうるので、巨大ファイルは
/// 行数を諦めて 0 にする（バッジが空になるだけで、「変更あり」の扱いは変わらない）。
fn untracked_added_lines(path: &Path) -> usize {
    match std::fs::metadata(path) {
        Ok(m) if m.len() > 2 * 1024 * 1024 => 0,
        _ => count_lines(path),
    }
}

fn is_tracked(dir: &Path, file_path: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(file_path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn count_lines(file_path: &Path) -> usize {
    std::fs::read_to_string(file_path)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

/// 旧ソース・新ソースを行単位で diff し、インラインの行リスト HTML を組む。
fn render_source_diff(old_src: &str, new_src: &str) -> String {
    let old_lines: Vec<&str> = old_src.lines().collect();
    let new_lines: Vec<&str> = new_src.lines().collect();

    // 資源保護: 行数・DP セルが過大な入力は差分を諦めて通知する。max で偏った次元
    // （例: 数百万行 vs 1行）のアロケーション爆発を、product で DP セル数を抑える。
    let (m, n) = (old_lines.len(), new_lines.len());
    if m.max(n) > 20_000 || m.saturating_mul(n) > 4_000_000 {
        return message_html("ファイルが大きすぎて差分表示できません");
    }

    let ops = line_diff(&old_lines, &new_lines);

    let mut rows = String::new();
    let mut i = 0;
    while i < ops.len() {
        match ops[i] {
            LineOp::Equal(text, o, n) => {
                rows.push_str(&ctx_row(o + 1, n + 1, text));
                i += 1;
            }
            // 連続する Del ラン → Add ラン をまとめ、同じ位置の行どうしを行内文字 diff で
            // ペアリングして、変わった文字だけ濃く強調する。
            LineOp::Del(..) | LineOp::Add(..) => {
                let del_start = i;
                while i < ops.len() && matches!(ops[i], LineOp::Del(..)) {
                    i += 1;
                }
                let dels = &ops[del_start..i];
                let add_start = i;
                while i < ops.len() && matches!(ops[i], LineOp::Add(..)) {
                    i += 1;
                }
                let adds = &ops[add_start..i];
                emit_replace(&mut rows, dels, adds);
            }
        }
    }

    if rows.is_empty() {
        return String::new();
    }
    // 内側トラック(.diff-body)で全行の幅を最長行に揃え、横スクロール時も短い行の
    // 地色が右端まで届くようにする（CSS 参照）。
    format!(r#"<div class="diff-source"><div class="diff-body">{}</div></div>"#, rows)
}

/// 削除行 → 追加行の順に出す（unified 形式）。k 番目の削除行と k 番目の追加行を
/// 対応づけて、行内の変わった文字だけを強調する。対応が無い余りの行は行全体を色付け。
/// ペアの行内文字 diff は 1 回だけ計算し、旧/新の HTML を両方使い回す。
fn emit_replace(out: &mut String, dels: &[LineOp], adds: &[LineOp]) {
    let paired = dels.len().min(adds.len());
    let mut del_codes: Vec<String> = Vec::with_capacity(dels.len());
    let mut paired_add: Vec<String> = Vec::with_capacity(paired);
    for (k, d) in dels.iter().enumerate() {
        let dtext = line_text(d);
        if k < paired {
            let (del_html, add_html) = char_marked(dtext, line_text(&adds[k]));
            del_codes.push(del_html);
            paired_add.push(add_html);
        } else {
            del_codes.push(html_escape(dtext));
        }
    }

    for (k, d) in dels.iter().enumerate() {
        if let LineOp::Del(_, o) = *d {
            out.push_str(&del_row(o + 1, &del_codes[k]));
        }
    }
    for (k, a) in adds.iter().enumerate() {
        if let LineOp::Add(text, n) = *a {
            let code = if k < paired {
                std::mem::take(&mut paired_add[k])
            } else {
                html_escape(text)
            };
            out.push_str(&add_row(n + 1, &code));
        }
    }
}

fn line_text<'a>(op: &LineOp<'a>) -> &'a str {
    match *op {
        LineOp::Equal(t, _, _) | LineOp::Del(t, _) | LineOp::Add(t, _) => t,
    }
}

/// 旧行・新行を文字単位で diff し、変わった文字を span で包んだ (旧HTML, 新HTML) を返す。
/// 長すぎる行は行内 diff を諦めて行全体を強調（メモリ/CPU保護）。
fn char_marked(old: &str, new: &str) -> (String, String) {
    let o: Vec<char> = old.chars().collect();
    let n: Vec<char> = new.chars().collect();
    // 極端に長い行 or 大きな DP は行内 diff を諦め、行全体を強調（偏った次元も max で抑える）。
    if o.len().max(n.len()) > 2_000 || o.len().saturating_mul(n.len()) > 40_000 {
        return (
            format!(r#"<span class="diff-char-del">{}</span>"#, html_escape(old)),
            format!(r#"<span class="diff-char-add">{}</span>"#, html_escape(new)),
        );
    }
    let (old_changed, new_changed) = char_change_flags(&o, &n);
    (
        render_flagged(&o, &old_changed, "diff-char-del"),
        render_flagged(&n, &new_changed, "diff-char-add"),
    )
}

/// 2 つの文字列の LCS を取り、各文字が「変わった（LCS に含まれない）」かどうかの
/// フラグ列を旧・新それぞれ返す。
fn char_change_flags(old: &[char], new: &[char]) -> (Vec<bool>, Vec<bool>) {
    let m = old.len();
    let n = new.len();
    let mut dp = vec![vec![0u32; n + 1]; m + 1];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            dp[i][j] = if old[i] == new[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut old_changed = vec![true; m];
    let mut new_changed = vec![true; n];
    let (mut i, mut j) = (0usize, 0usize);
    while i < m && j < n {
        if old[i] == new[j] {
            old_changed[i] = false;
            new_changed[j] = false;
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    (old_changed, new_changed)
}

/// フラグ列に従い、連続する同フラグの文字をまとめてエスケープし、変わった区間だけ
/// span で包んだ HTML を返す。
fn render_flagged(chars: &[char], changed: &[bool], class: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let flag = changed[i];
        let start = i;
        while i < chars.len() && changed[i] == flag {
            i += 1;
        }
        let seg: String = chars[start..i].iter().collect();
        let esc = html_escape(&seg);
        if flag {
            out.push_str(&format!(r#"<span class="{}">{}</span>"#, class, esc));
        } else {
            out.push_str(&esc);
        }
    }
    out
}

#[derive(Clone, Copy)]
enum LineOp<'a> {
    Equal(&'a str, usize, usize),
    Del(&'a str, usize),
    Add(&'a str, usize),
}

/// 行の LCS を取り、旧→新の編集列（Equal/Del/Add）を、元の行インデックス付きで返す。
fn line_diff<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<LineOp<'a>> {
    // 呼び出し側（render_source_diff）が行数・DP セル数の上限を保証している前提。
    let m = old.len();
    let n = new.len();
    let mut dp = vec![vec![0u32; n + 1]; m + 1];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            dp[i][j] = if old[i] == new[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < m && j < n {
        if old[i] == new[j] {
            ops.push(LineOp::Equal(new[j], i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            ops.push(LineOp::Del(old[i], i));
            i += 1;
        } else {
            ops.push(LineOp::Add(new[j], j));
            j += 1;
        }
    }
    while i < m {
        ops.push(LineOp::Del(old[i], i));
        i += 1;
    }
    while j < n {
        ops.push(LineOp::Add(new[j], j));
        j += 1;
    }
    ops
}

fn ctx_row(old_ln: usize, new_ln: usize, text: &str) -> String {
    row("ctx", &old_ln.to_string(), &new_ln.to_string(), ' ', &html_escape(text))
}

fn del_row(old_ln: usize, code_html: &str) -> String {
    row("del", &old_ln.to_string(), "", '-', code_html)
}

fn add_row(new_ln: usize, code_html: &str) -> String {
    row("add", "", &new_ln.to_string(), '+', code_html)
}

/// `code_html` はエスケープ済み（＋文字強調 span 込み）の前提。
fn row(cls: &str, old_ln: &str, new_ln: &str, sign: char, code_html: &str) -> String {
    format!(
        r#"<div class="diff-row diff-{cls}"><span class="diff-gutter">{old_ln}</span><span class="diff-gutter">{new_ln}</span><span class="diff-sign">{sign}</span><span class="diff-text">{code}</span></div>"#,
        cls = cls,
        old_ln = old_ln,
        new_ln = new_ln,
        sign = sign,
        code = code_html,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .expect("git を実行できませんでした")
            .status
            .success();
        assert!(ok, "git {:?} が失敗しました", args);
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("md-diff-test-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn changed_line_marks_del_add_and_inline_chars() {
        let dir = temp_dir("change");
        git(&dir, &["init", "-q"]);
        let file = dir.join("a.rs");
        std::fs::write(&file, "let x = 1;\nkeep me\n").unwrap();
        git(&dir, &["add", "a.rs"]);
        git(&dir, &["commit", "-q", "-m", "init"]);
        std::fs::write(&file, "let x = 2;\nkeep me\n").unwrap();

        let html = render_diff_inner(&file);
        assert!(html.contains("diff-source"), "コンテナが無い: {html}");
        assert!(html.contains("diff-del"), "削除行が無い: {html}");
        assert!(html.contains("diff-add"), "追加行が無い: {html}");
        // 変わった文字（1 と 2）だけが強調される。
        assert!(html.contains(r#"<span class="diff-char-del">1</span>"#), "旧文字強調が無い: {html}");
        assert!(html.contains(r#"<span class="diff-char-add">2</span>"#), "新文字強調が無い: {html}");
        // 変わらない行は context 表示。
        assert!(html.contains("diff-ctx"), "context 行が無い: {html}");
        assert!(html.contains("keep me"), "文脈行の内容が無い: {html}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_change_has_no_highlight_and_no_message() {
        let dir = temp_dir("nochange");
        git(&dir, &["init", "-q"]);
        let file = dir.join("a.md");
        std::fs::write(&file, "# タイトル\n\n本文なのだ。\n").unwrap();
        git(&dir, &["add", "a.md"]);
        git(&dir, &["commit", "-q", "-m", "init"]);

        let html = render_diff_inner(&file);
        assert!(!html.contains("diff-del") && !html.contains("diff-add"), "ハイライトが付いた: {html}");
        assert!(!html.contains("md-notice"), "余計な通知が出た: {html}");
        assert!(html.contains("本文なのだ"), "本文が無い: {html}");
        assert!(html.contains("diff-ctx"), "context 表示になっていない: {html}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn untracked_file_is_all_added() {
        let dir = temp_dir("untracked");
        git(&dir, &["init", "-q"]);
        std::fs::write(dir.join("seed.md"), "seed\n").unwrap();
        git(&dir, &["add", "seed.md"]);
        git(&dir, &["commit", "-q", "-m", "init"]);
        let file = dir.join("new.txt");
        std::fs::write(&file, "brand new\nsecond\n").unwrap();

        let html = render_diff_inner(&file);
        assert!(html.contains("diff-add"), "追加行が無い: {html}");
        assert!(html.contains("brand new"), "内容が無い: {html}");
        assert!(!html.contains("diff-del"), "削除行が出た: {html}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn outside_repo_shows_plain_source_no_message() {
        let dir = temp_dir("norepo");
        let file = dir.join("a.md");
        std::fs::write(&file, "# タイトル\n\n本文なのだ。\n").unwrap();

        let html = render_diff_inner(&file);
        assert!(!html.contains("md-notice"), "余計な通知が出た: {html}");
        assert!(!html.contains("diff-del") && !html.contains("diff-add"), "ハイライトが付いた: {html}");
        assert!(html.contains("本文なのだ"), "本文が無い: {html}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn diff_stat_counts_tracked_untracked_and_clean() {
        let dir = temp_dir("stat");
        git(&dir, &["init", "-q"]);
        let file = dir.join("a.md");
        std::fs::write(&file, "a\nb\nc\n").unwrap();
        git(&dir, &["add", "a.md"]);
        git(&dir, &["commit", "-q", "-m", "init"]);

        // 変更なし → (0, 0)。
        assert_eq!(diff_stat(&file), (0, 0));

        // 1 行変更 → 追加 1 / 削除 1。
        std::fs::write(&file, "a\nB\nc\n").unwrap();
        assert_eq!(diff_stat(&file), (1, 1));

        // 未追跡ファイル → 全行を追加として数える。
        let untracked = dir.join("new.txt");
        std::fs::write(&untracked, "x\ny\n").unwrap();
        assert_eq!(diff_stat(&untracked), (2, 0));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changed_files_lists_modified_and_untracked_but_not_deleted() {
        let dir = temp_dir("changed");
        git(&dir, &["init", "-q"]);
        std::fs::create_dir(dir.join("sub")).unwrap();
        std::fs::write(dir.join("kept.md"), "a\nb\nc\n").unwrap();
        std::fs::write(dir.join("sub/mod.md"), "a\nb\nc\n").unwrap();
        std::fs::write(dir.join("gone.md"), "x\n").unwrap();
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-q", "-m", "init"]);

        std::fs::write(dir.join("sub/mod.md"), "a\nB\nc\n").unwrap();
        std::fs::remove_file(dir.join("gone.md")).unwrap();
        std::fs::write(dir.join("new.txt"), "x\ny\n").unwrap();

        let changed = changed_files(&dir);
        let paths: Vec<&str> = changed.iter().map(|(p, _, _)| p.as_str()).collect();

        // サブディレクトリのパスは root 相対（--relative）で返る。
        assert!(paths.contains(&"sub/mod.md"), "変更ファイルが無い: {paths:?}");
        // 未追跡は全行が追加。
        assert!(paths.contains(&"new.txt"), "未追跡ファイルが無い: {paths:?}");
        // 開けないものは出さない。無変更のファイルも当然出ない。
        assert!(!paths.contains(&"gone.md"), "削除済みが混ざっている: {paths:?}");
        assert!(!paths.contains(&"kept.md"), "無変更が混ざっている: {paths:?}");

        let stat = |name: &str| {
            changed.iter().find(|(p, _, _)| p == name).map(|(_, a, d)| (*a, *d)).unwrap()
        };
        assert_eq!(stat("sub/mod.md"), (1, 1));
        assert_eq!(stat("new.txt"), (2, 0));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changed_files_is_empty_outside_repo() {
        let dir = temp_dir("changed-norepo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.md"), "x\n").unwrap();
        assert!(changed_files(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_input_is_skipped_with_message() {
        let big = "line\n".repeat(20_001);
        let html = render_source_diff("", &big);
        assert!(html.contains("大きすぎて"), "サイズ超過通知が無い: {html}");
        assert!(!html.contains("diff-source"), "差分を組んでしまった: {html}");
    }

    #[test]
    fn non_utf8_reports_binary() {
        let dir = temp_dir("binary");
        let file = dir.join("blob.bin");
        std::fs::write(&file, [0xff, 0xfe, 0x00, 0x01]).unwrap();

        let html = render_diff_inner(&file);
        assert!(html.contains("バイナリ"), "バイナリ通知が無い: {html}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
