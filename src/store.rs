//! 設定ディレクトリに置く「レコードの並び」を読み書きする共通ヘルパ。
//!
//! 1 行 ＝ 1 レコード、フィールドはタブ区切り。レコードの並びがそのまま意味を持つ
//! （Quick Access なら表示順、通知なら新しい順）ので、読んだ順序は保って返す。
//!
//! 客は Quick Access（#35）と通知の履歴（#32）。**書式とエスケープを持つ場所はここ
//! 1 つ**にする——機能ごとに手書きすると、パスにタブが入ったときに壊れる場所が
//! 2 つになる。原子書き込み（[`write_text`]）はレコードの形を取らない設定（#38）も
//! 使うので、書式を組む側から切り離して**単体で呼べるようにしてある**。
//!
//! ⚠️ ただし**層になってはいない。**[`write_text`] は [`escape`] と同じ公開面に
//! 並んでいるだけで、新しい客が書式を手書きするのを止めるものは無い。守っているのは
//! このコメントだけである。
//!
//! Why not serde / JSON: 依存を 1 つも増やしていないプロジェクトで、扱うのは
//! 「文字列の表」だけである。手書きのパーサが 30 行で済むうちは、行指向の方が
//! 壊れたファイルを人が読んで直せる。
//!
//! Why not 設定（`active-theme` の行き先）もこの書式に乗せる: #38 で決め直して、
//! 乗せないことにした。エスケープを被せると人が手で書いた `\` の意味が変わる。
//! 設定は [`crate::settings`] が `key=value` で持ち、ここからは [`write_text`] だけを
//! 借りる。
//!
//! 置き場所（`dir`）を引数で受けるのは、`uninstall::plan` と同じ理由——テストが
//! 実物の HOME を触らずに往復を確かめられるようにするため。ここは「壊れたら全部の
//! 台帳が壊れる」層なので、テストできない形にしてはいけない。

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// 同じプロセスの中でも temp の名前が衝突しないようにする連番。
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// フィールドの区切り。パスに含まれうるので、値の側でエスケープする
/// （[`escape`] の対象から外すと、区切りが値に混ざって読めなくなる）。
const SEP: char = '\t';

/// `<dir>/<name>` を読む。無い・読めない・壊れているときは空を返す。
///
/// Why not 壊れたファイルを検知して知らせる: 起動経路で呼ばれるので、ここで止まると
/// 窓が出ない。**読めなければ空として起動する**方を選ぶ（issue #32 の決め）。
pub fn read(dir: &Path, name: &str) -> Vec<Vec<String>> {
    let Ok(text) = std::fs::read_to_string(dir.join(name)) else { return Vec::new() };
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| line.split(SEP).map(unescape).collect())
        .collect()
}

/// `<dir>/<name>` へ書く。置き換えが原子になる仕掛けは [`write_text`] が持つ。
pub fn write(dir: &Path, name: &str, records: &[Vec<String>]) -> io::Result<()> {
    let mut text = String::new();
    for record in records {
        for (i, field) in record.iter().enumerate() {
            if i > 0 {
                text.push(SEP);
            }
            text.push_str(&escape(field));
        }
        text.push('\n');
    }
    write_text(dir, name, &text)
}

/// `<dir>/<name>` をまるごと置き換える。temp へ書いてから rename するので、途中で
/// 落ちても中途半端な内容が残らない（次の起動が読むのは前の完全な内容）。
///
/// レコードの形を持たない設定（[`crate::settings`]）もここを通る。**書式は客ごとに
/// 違ってよいが、置き換え方は 1 つ**——temp の置き場所を間違える場所を増やさない。
///
/// ⚠️ **temp の名前は書き手ごとに変える。** 固定名にすると、A が temp へ書いている
/// 最中に B が同じ temp を rename してしまい、A の続きが live のファイルを直接
/// 書き換える。rename 自体が原子でも、そこで壊れる（8 スレッドで測って、読んだ
/// 2889 回のうち 2466 回が途中の状態だった）。#49 で窓が長生きするので、窓と CLI が
/// 同時に書く経路は現実に在る。
pub fn write_text(dir: &Path, name: &str, text: &str) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    // temp は同じディレクトリに置く。別のファイルシステム（/tmp）へ置くと
    // rename が EXDEV で落ちる。
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!("{name}.tmp.{}.{seq}", std::process::id()));
    // 名前を一意にしたぶん、失敗した残骸は上書きされずに溜まる。どちらの失敗でも畳む。
    // Why not 掃除を別に持つ: 残りうるのは SIGKILL と電源断で死んだときだけになり、
    // そのために設定ディレクトリを走査する口を増やす方が高くつく。
    if let Err(e) = std::fs::write(&tmp, text) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, dir.join(name)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// 区切りと行末を値の中から追い出す。`\` 自身も対象（でないと `a\` ＋ 区切り が
/// `a` ＋ タブ と区別できなくなる）。
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

/// 知らないエスケープ（`\x`）は `x` に畳む。
/// Why not エラーにする: ここで弾くと、人が手で直したファイルの 1 文字の書き損じが
/// 台帳ごと消える。読み手は壊れた 1 レコードを捨てずに受け取って、使う側
/// （Quick Access なら `id_to_path`）の関門で落とす方が被害が小さい。
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("md-store-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn records(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|r| r.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    #[test]
    fn what_was_written_is_what_comes_back() {
        let dir = temp("roundtrip");
        let want = records(&[&["/a/b.md", "file"], &["/c", "dir"]]);
        write(&dir, "list", &want).unwrap();
        assert_eq!(read(&dir, "list"), want);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tab_or_a_newline_in_a_value_survives_the_round_trip() {
        let dir = temp("control-chars");
        // 区切りにも行末にも化けてはいけない値。
        let want = records(&[&["/a\tb\nc\r\\d", "file"]]);
        write(&dir, "list", &want).unwrap();
        assert_eq!(read(&dir, "list"), want);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reading_what_was_never_written_is_empty_not_an_error() {
        let dir = temp("missing");
        assert!(read(&dir, "list").is_empty());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("list"), b"\xff\xfe not utf-8").unwrap();
        assert!(read(&dir, "list").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 書き終わったディレクトリに残るのは本物 1 つだけ。temp の名前を変えたので、
    /// 決め打ちの `list.tmp` を見るだけでは番人にならない。
    #[test]
    fn writing_leaves_no_temp_file_behind() {
        let dir = temp("tmp");
        write(&dir, "list", &records(&[&["/a", "dir"]])).unwrap();
        write_text(&dir, "plain", "x\n").unwrap();
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["list".to_string(), "plain".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 置き換えに失敗したぶんの一時ファイルを残さないこと。名前を一意にしたので、
    /// 畳まないと失敗のたびに溜まる（固定名のころは次の書き込みが上書きしていた）。
    #[test]
    fn a_failed_replace_leaves_no_temp_file_behind() {
        let dir = temp("rename-fails");
        // 行き先をディレクトリにして rename を失敗させる。
        std::fs::create_dir_all(dir.join("list").join("blocker")).unwrap();
        assert!(write_text(&dir, "list", "x\n").is_err(), "失敗するはずが通っている");

        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "一時ファイルが残っている: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 同じファイルを同時に書いても、読んだ側が途中の状態を見ないこと。
    #[test]
    fn writers_racing_on_one_file_never_leave_a_half_written_state() {
        let dir = temp("concurrent");
        let long: Vec<Vec<String>> = (0..2000).map(|i| vec![format!("/pad/{i}")]).collect();
        write(&dir, "list", &long).unwrap();

        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    for _ in 0..20 {
                        write(&dir, "list", &long).unwrap();
                    }
                });
            }
            s.spawn(|| {
                for _ in 0..200 {
                    let got = read(&dir, "list");
                    assert_eq!(got.len(), long.len(), "途中まで書かれたファイルを読んだ");
                }
            });
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_escape_keeps_the_character() {
        assert_eq!(unescape("/a\\qb"), "/aqb");
        assert_eq!(unescape("trailing\\"), "trailing\\");
    }
}
