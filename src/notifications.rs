//! `md --notify` で届いたファイルの台帳（#36）。ベルの見た目と既読の操作は #32 だが、
//! **持ち主は Rust** と決めてある（#32）ので、受け取った時点でここへ積んで永続化する。
//!
//! ページが描ける前に届くぶんがあるのが理由。受信は accept ループ（別スレッド）で、
//! 窓は `AppEvent::Ready` より前は中身を持たない。JS に持たせると、その間に来たものが
//! `evaluate_script` ごと消える。Rust が持っていれば、後から何度でも流し込める。
//!
//! 並びは**新しい順**。1 レコードは `<識別子>\t<届いた時刻>\t<read|unread>` の
//! 3 フィールドで、書式と原子書き込みは [`crate::store`] が持つ。

use std::path::Path;

/// 台帳のファイル名。`~/.config/md-preview/` 直下。
const FILE: &str = "notifications";

/// 持つ件数の上限。溢れたら古いものから落ちる（#32）。
const MAX: usize = 100;

/// 1 行ぶん。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 届いたファイルの識別子（絶対パス）。
    pub path: String,
    /// 届いた時刻（UNIX 秒）。**表示は相対時刻**（#32）なので、絶対時刻で持って
    /// 描くときに引く。読めなかった行は 0 になるが、行ごと落とすよりはまし——
    /// 台帳が壊れていても「届いた事実」は残す。
    pub at: u64,
    /// 既読か。書き換えるのは #32。
    pub read: bool,
}

/// 保存されている並びを読む。読めなければ空。
pub fn load() -> Vec<Entry> {
    let Some(dir) = crate::config_dir() else { return Vec::new() };
    parse(crate::store::read(&dir, FILE))
}

/// 届いたぶんを積む。`ids` の並びはそのまま先頭の並びになる。
///
/// 絶対パスでないものは捨てる。ここはワイヤから来た値の入口なので、**受け取った側でも
/// 形を見る**（送り側が `canonicalize` を通している前提に乗らない）。
///
/// Why not ここで `exists()` を聞いて、消えたファイルを積まない: **消えたものを黙って
/// 落とさない**のが #32 と #35 で揃えた決め。積んだ行は履歴で、押して初めて
/// 「開けません」と言う。ここで聞くと、`--notify` の直後に書き換えられただけの
/// ファイル（AI が書いて、すぐ整形し直す）が通知ごと消える。
///
/// ⚠️ read-modify-write をロック無しでやっている。いま `add` を呼ぶのは**座を持つ
/// 1 プロセスのイベントループスレッドだけ**なので直列だが、#32 が別プロセスから
/// 書けるようにすると last-writer-wins で行が消える。
pub fn add(ids: &[String]) {
    let Some(dir) = crate::config_dir() else { return };
    let mut list = parse(crate::store::read(&dir, FILE));
    if !push_front(&mut list, ids, now()) {
        return;
    }
    save(&dir, &list);
}

/// 先頭へ積む。積むものが 1 つも無ければ false（書き込みごと省く）。
///
/// **同じパスは 1 行にまとめる**（#32）。古い行を消してから先頭へ置き直すので、
/// 時刻が新しくなり、既読だったものは未読へ戻る。「さっき見たあれがまた更新された」を
/// 未読として拾えるようにするため。
fn push_front(list: &mut Vec<Entry>, ids: &[String], at: u64) -> bool {
    let mut added = false;
    // 逆から入れて先頭へ差すと、`ids` の並びがそのまま先頭の並びになる。
    // 1 回の `--notify` で届いた複数ファイルは同時刻なので、時刻では順が決まらない。
    for id in ids.iter().rev() {
        if !id.starts_with('/') {
            continue;
        }
        list.retain(|e| &e.path != id);
        list.insert(0, Entry { path: id.clone(), at, read: false });
        added = true;
    }
    list.truncate(MAX);
    added
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
            at: record.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
            read: record.get(2).map(|k| k == "read").unwrap_or(false),
        });
    }
    // 手で書き足されたファイルも上限に従わせる。読んだ側が長さを見ないで済む。
    out.truncate(MAX);
    out
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn save(dir: &Path, list: &[Entry]) {
    let records: Vec<Vec<String>> = list.iter().map(record).collect();
    // 書けなくても窓は動き続ける（次の起動で前の内容に戻るだけ）。知らせ先を持たない
    // ので握りつぶす——#35 の Quick Access と同じ扱い。
    let _ = crate::store::write(dir, FILE, &records);
}

fn record(e: &Entry) -> Vec<String> {
    vec![
        e.path.clone(),
        e.at.to_string(),
        if e.read { "read" } else { "unread" }.to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(path: &str, at: u64, read: bool) -> Entry {
        Entry { path: path.to_string(), at, read }
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn records(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect()
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("md-notify-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// 書いた側（`record`）と読む側（`parse`）が同じ綴りを使っていること。
    /// ずれると既読が毎回未読に戻り、ベルが落ちない。
    #[test]
    fn a_read_entry_is_still_read_after_a_save_and_a_load() {
        let dir = temp("roundtrip");
        let want = vec![entry("/a/new.md", 1_700_000_000, false), entry("/a/old.md", 1, true)];
        save(&dir, &want);
        assert_eq!(parse(crate::store::read(&dir, FILE)), want);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_arrives_goes_to_the_front_in_the_order_it_was_sent() {
        let mut list = vec![entry("/old.md", 1, true)];
        assert!(push_front(&mut list, &ids(&["/a.md", "/b.md"]), 9));
        assert_eq!(
            list,
            vec![entry("/a.md", 9, false), entry("/b.md", 9, false), entry("/old.md", 1, true)]
        );
    }

    #[test]
    fn the_same_file_again_is_one_row_that_moves_up_and_goes_unread() {
        let mut list = vec![entry("/x.md", 1, true), entry("/y.md", 2, false)];
        push_front(&mut list, &ids(&["/x.md"]), 9);
        assert_eq!(list, vec![entry("/x.md", 9, false), entry("/y.md", 2, false)]);
    }

    #[test]
    fn the_oldest_rows_fall_off_at_the_cap() {
        let mut list: Vec<Entry> = (0..MAX).map(|i| entry(&format!("/{i}.md"), 1, true)).collect();
        push_front(&mut list, &ids(&["/new.md"]), 9);
        assert_eq!(list.len(), MAX);
        assert_eq!(list[0], entry("/new.md", 9, false));
        assert!(!list.iter().any(|e| e.path == format!("/{}.md", MAX - 1)));
    }

    #[test]
    fn a_relative_path_is_never_stored() {
        let mut list = Vec::new();
        assert!(!push_front(&mut list, &ids(&["docs/a.md", ""]), 9));
        assert!(list.is_empty());
        assert_eq!(parse(records(&[&["docs/a.md", "1", "unread"]])), Vec::new());
    }

    #[test]
    fn a_record_without_a_time_or_a_flag_still_reads_as_an_unread_entry() {
        assert_eq!(parse(records(&[&["/a.md"]])), vec![entry("/a.md", 0, false)]);
    }

    #[test]
    fn the_same_path_twice_on_disk_reads_as_one_entry_and_keeps_the_newer_one() {
        // 手で書き足された台帳。先頭を残すのは、並びが**新しい順**だから
        // （`quick_access` は登録順なので同じ「先頭を残す」でも意味が違う）。
        let got = parse(records(&[&["/a.md", "9", "unread"], &["/a.md", "1", "read"]]));
        assert_eq!(got, vec![entry("/a.md", 9, false)]);
    }

    #[test]
    fn a_ledger_longer_than_the_cap_is_cut_on_the_way_in() {
        // 手で書き足されたぶんも上限に従わせる。読んだ側が長さを見ないで済む。
        let rows: Vec<Vec<String>> = (0..MAX + 10)
            .map(|i| vec![format!("/{i}.md"), "1".into(), "read".into()])
            .collect();
        assert_eq!(parse(rows).len(), MAX);
    }
}
