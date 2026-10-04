//! Quick Access（#35）の台帳。よく開くフォルダ・ファイルへの固定ジャンプを
//! `~/.config/md-preview/quick-access` に持つ。
//!
//! **並びがそのまま表示順**で、登録順に後ろへ積む（並べ替えは持たない）。
//! 1 レコードは `<識別子>\t<dir|file>` の 2 フィールドで、書式と原子書き込みは
//! [`crate::store`] が持つ。
//!
//! 種別を保存するのは、消えたパスでも行を描けるようにするため。**消えたものを
//! 黙って落とさない**（クリックして初めて気づかせる、が #32 と揃えた決め）ので、
//! 描画の時点で `is_dir()` を聞けるとは限らない。登録できた時点の答えを持っておく。

use std::path::Path;

/// 台帳のファイル名。`~/.config/md-preview/` 直下。
const FILE: &str = "quick-access";

/// 1 行ぶん。`is_dir` は**登録した時点**のファイルシステムの答え。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub is_dir: bool,
}

/// 保存されている並びを読む。読めなければ空。
pub fn load() -> Vec<Entry> {
    let Some(dir) = crate::config_dir() else { return Vec::new() };
    parse(crate::store::read(&dir, FILE))
}

/// 登録する。既にいるパスは**動かさない**——★ はトグルなので、登録済みのものが
/// もう一度来るのは転送や手編集のときだけで、そこで並びが変わる理由が無い。
///
/// `id` は**ページが送ってきた識別子をそのまま**積む。`is_dir` を決めるために
/// 関門（`id_to_path`）を通した実体は呼び出し側が持っているが、そちらの
/// 正規化済みパスを保存してはいけない——ツリーの識別子は symlink を辿らない
/// （#33 の決め）ので、正規化したものを積むとページの持つ並びと食い違い、
/// 同じ行を外せなくなる。
pub fn add(id: &str, is_dir: bool) {
    let Some(dir) = crate::config_dir() else { return };
    let mut list = parse(crate::store::read(&dir, FILE));
    if list.iter().any(|e| e.path == id) {
        return;
    }
    list.push(Entry { path: id.to_string(), is_dir });
    save(&dir, &list);
}

/// 外す。消えたパスも外せるよう、ここは関門を通らない素の識別子で引く。
pub fn remove(id: &str) {
    let Some(dir) = crate::config_dir() else { return };
    let mut list = parse(crate::store::read(&dir, FILE));
    let before = list.len();
    list.retain(|e| e.path != id);
    if list.len() != before {
        save(&dir, &list);
    }
}

/// 壊れた行（絶対パスでないもの）はここで落とす——この先は全部
/// 「識別子＝絶対パス」の前提で動く（#33）。
fn parse(records: Vec<Vec<String>>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for record in records {
        let Some(path) = record.first() else { continue };
        if !path.starts_with('/') || out.iter().any(|e| &e.path == path) {
            continue;
        }
        out.push(Entry {
            path: path.clone(),
            is_dir: record.get(1).map(|k| k == "dir").unwrap_or(false),
        });
    }
    out
}

fn save(dir: &Path, list: &[Entry]) {
    let records: Vec<Vec<String>> = list.iter().map(record).collect();
    // 書けなくても窓は動き続ける（次の起動で前の内容に戻るだけ）。窓の中に出す
    // 手段を持たないので、ここで握りつぶす。
    let _ = crate::store::write(dir, FILE, &records);
}

fn record(e: &Entry) -> Vec<String> {
    vec![e.path.clone(), if e.is_dir { "dir" } else { "file" }.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entries(paths: &[(&str, bool)]) -> Vec<Entry> {
        paths.iter().map(|(p, d)| Entry { path: p.to_string(), is_dir: *d }).collect()
    }

    fn records(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect()
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("md-quick-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// 書いた側（`record`）と読む側（`parse`）が同じ綴りを使っていること。
    /// ここがずれると、留めたフォルダが次の起動でファイルになり、押した瞬間に
    /// ディレクトリを本文として描こうとする——しかも誰も落ちない。
    #[test]
    fn a_folder_is_still_a_folder_after_a_save_and_a_load() {
        let dir = temp("kinds");
        let want = entries(&[("/a/proj", true), ("/a/note.md", false)]);
        save(&dir, &want);
        assert_eq!(parse(crate::store::read(&dir, FILE)), want);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_relative_path_never_becomes_an_entry() {
        let got = parse(records(&[&["docs", "dir"], &["/abs/a.md", "file"]]));
        assert_eq!(got, entries(&[("/abs/a.md", false)]));
    }

    #[test]
    fn the_same_path_twice_is_one_entry_and_keeps_the_first_place() {
        let got = parse(records(&[&["/a", "dir"], &["/b.md", "file"], &["/a", "file"]]));
        assert_eq!(got, entries(&[("/a", true), ("/b.md", false)]));
    }

    #[test]
    fn a_record_without_a_kind_reads_as_a_file() {
        assert_eq!(parse(records(&[&["/a.md"]])), entries(&[("/a.md", false)]));
    }
}
