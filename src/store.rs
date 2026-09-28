//! 設定ディレクトリに置く「レコードの並び」を読み書きする共通ヘルパ。
//!
//! 1 行 ＝ 1 レコード、フィールドはタブ区切り。レコードの並びがそのまま意味を持つ
//! （Quick Access なら表示順、通知なら新しい順）ので、読んだ順序は保って返す。
//!
//! 最初の客は Quick Access（#35）だが、通知の履歴（#32）と設定（#38）も同じ書式で
//! ここへ乗る。**書式とエスケープと原子書き込みを持つ場所はここ 1 つ**にする——
//! 機能ごとに手書きすると、パスにタブが入ったときに壊れる場所が 3 つになる。
//!
//! Why not serde / JSON: 依存を 1 つも増やしていないプロジェクトで、扱うのは
//! 「文字列の表」だけである。手書きのパーサが 30 行で済むうちは、行指向の方が
//! 壊れたファイルを人が読んで直せる。
//!
//! Why not `theme.rs` の `active-theme` もここへ移す: あれは 1 行の素文字列で、
//! 読み手が `trim()` するだけの形が外（ユーザーが手で書き換える設定）として成立して
//! いる。エスケープを被せると人が書いた `\` の意味が変わる。設定（#38）を作るときに
//! まとめて決め直す。
//!
//! 置き場所（`dir`）を引数で受けるのは、`uninstall::plan` と同じ理由——テストが
//! 実物の HOME を触らずに往復を確かめられるようにするため。ここは「壊れたら全部の
//! 台帳が壊れる」層なので、テストできない形にしてはいけない。

use std::io;
use std::path::Path;

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

/// `<dir>/<name>` へ書く。temp へ書いてから rename するので、途中で落ちても
/// 中途半端な内容が残らない（次の起動が読むのは前の完全な内容）。
pub fn write(dir: &Path, name: &str, records: &[Vec<String>]) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;

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

    // temp は同じディレクトリに置く。別のファイルシステム（/tmp）へ置くと
    // rename が EXDEV で落ちる。
    let tmp = dir.join(format!("{name}.tmp"));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, dir.join(name))
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

    #[test]
    fn writing_leaves_no_temp_file_behind() {
        let dir = temp("tmp");
        write(&dir, "list", &records(&[&["/a", "dir"]])).unwrap();
        assert!(!dir.join("list.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_escape_keeps_the_character() {
        assert_eq!(unescape("/a\\qb"), "/aqb");
        assert_eq!(unescape("trailing\\"), "trailing\\");
    }
}
