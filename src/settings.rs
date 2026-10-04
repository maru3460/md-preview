//! 設定の置き場所 `~/.config/md-preview/settings`（#38）。
//!
//! 台帳（`quick-access` / `notifications`）と違って、ここに乗るのは**短い値が
//! いくつか並ぶだけ**。1 行 ＝ `key=value` で、人が直接開いて書き換えることを
//! 前提にする。
//!
//! Why not [`crate::store`] のレコード形式に乗せる: あちらはタブ区切り＋エスケープで、
//! パスのように何でも入りうる値を安全に運ぶための形である。設定の値はテーマ名や
//! 列挙で、エスケープする対象を持たない。手で開いたときに `\t` と `\\` が見える
//! ファイルにする理由が無い。**共有するのは書式ではなく原子書き込みの方**
//! （[`crate::store::write_text`]）。
//!
//! Why not エスケープを持たない代わりに何でも書けるようにする: 改行を値に入れられると
//! 1 行 1 設定が崩れて、次に読んだときファイルごと意味が変わる。エスケープを捨てた
//! 対価は [`Settings::put`] の関門で払う。
//!
//! Why not 知らない行を捨てる: 新しい md が書いたキーを古い md が起動しただけで
//! 消す、が起きる。人が書いたコメントも同じ。**読めなかった行は綴りのまま書き戻す。**

use std::io;
use std::path::Path;

/// 設定のファイル名。`~/.config/md-preview/` 直下。
const FILE: &str = "settings";

/// 移行元。`md theme <name>` が名前 1 行だけを書いていた旧ファイル。
const LEGACY_THEME_FILE: &str = "active-theme";

/// 使用中のテーマ名。読み書きの窓口は [`crate::theme`] 側にある。
///
/// Why not 機能側が `"theme"` と直に書く: 読む側と書く側で綴りがずれても誰も落ちない。
pub const THEME: &str = "theme";

/// ファイルの 1 行。
#[derive(Debug, PartialEq, Eq)]
enum Line {
    /// `key=value` と読めた行。
    Pair(String, String),
    /// それ以外（空行・コメント・書き損じ）。綴りのまま書き戻す。
    Other(String),
}

/// 読み込んだ設定 1 ファイルぶん。行の並びは読んだ順のまま持つ。
#[derive(Debug, PartialEq, Eq)]
pub struct Settings {
    lines: Vec<Line>,
}

impl Settings {
    /// 値を引く。同じキーが 2 度あれば先に書いてある方が勝つ（[`Settings::put`] が
    /// 書き換えるのも先頭なので、両者は必ず同じ行を見る）。
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().find_map(|line| match line {
            Line::Pair(k, v) if k == key => Some(v.as_str()),
            _ => None,
        })
    }

    /// 値を差し替える。無いキーは末尾へ足す（既にある行は動かさない）。
    ///
    /// **値がこのファイルへ入る唯一の入口。**関門もここに置く。
    ///
    /// Why not 関門を呼び出し側（[`set_at`]）に置く: 移行（[`migrate_legacy_theme`]）も
    /// 値を入れる経路なので、入口ごとに置くと次に足す経路が素通りする。実際に
    /// 旧 `active-theme` へ手で改行を入れると、別のキーが生えていた。
    ///
    /// Why not 断らずに `trim()` して受ける: 書いたものと読み戻すものが黙って
    /// 食い違う。呼び出し側は自分が何を書いたか知っているので、直せる形で返す。
    ///
    /// Why not エスケープして通す: それをやるなら [`crate::store`] の書式に乗る方が
    /// 早い。ここは「手で開いて読める」を取った側である。
    fn put(&mut self, key: &str, value: &str) -> io::Result<()> {
        if !round_trips(key, value) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "設定のキーか値が読み戻せない形",
            ));
        }
        for line in self.lines.iter_mut() {
            if let Line::Pair(k, v) = line {
                if k == key {
                    *v = value.to_string();
                    return Ok(());
                }
            }
        }
        self.lines.push(Line::Pair(key.to_string(), value.to_string()));
        Ok(())
    }

    /// ファイルへ書く綴り。**読んだテキストをそのまま返すわけではない**——
    /// `key=value` と読めた行は正規化され（`=` の前後の空白が落ちる）、末尾には
    /// 必ず改行が 1 本付く。読めなかった行だけが綴りのまま出る。
    fn text(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            match line {
                Line::Pair(k, v) => {
                    out.push_str(k);
                    out.push('=');
                    out.push_str(v);
                }
                Line::Other(raw) => out.push_str(raw),
            }
            out.push('\n');
        }
        out
    }
}

/// 保存されている設定を読む。読めなければ空。
///
/// Why not 読めないファイルをエラーとして返す: 起動経路で呼ばれるので、ここで
/// 止まると窓が出ない（[`crate::store::read`] と同じ決め）。**ただし書く側
/// （[`set_at`]）はこの緩さに乗ってはいけない**——読めなかった中身を空と見なして
/// 上書きすると、読めただけの行まで道連れになる。
pub fn load() -> Settings {
    let Some(dir) = crate::config_dir() else { return parse("") };
    load_at(&dir)
}

/// 値を書く。ファイルごと置き換えるので、知らないキーも書き戻される。
pub fn set(key: &str, value: &str) -> io::Result<()> {
    let dir = crate::config_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME が設定されていない"))?;
    set_at(&dir, key, value)
}

fn load_at(dir: &Path) -> Settings {
    match read_settings(dir) {
        Ok(mut settings) => {
            migrate_legacy_theme(dir, &mut settings);
            settings
        }
        // ⚠️ 読めなかったときは 1 文字も書かない。空を [`migrate_legacy_theme`] へ
        // 渡すと、あれは書く側なので、読めなかった行まで空で上書きしてしまう
        // （読むだけのつもりの `md theme` が設定を全消しする）。
        //
        // Why not 何もせず空を返す: それだと旧ファイルしか持っていない人が、
        // 読めない settings があるというだけで今回ぶんのテーマまで失う。
        // **読むのは続けて、書くのだけをやめる。**
        Err(_) => legacy_theme_only(dir),
    }
}

/// 旧ファイルの名前だけを持った、ディスクに触れていない設定。
fn legacy_theme_only(dir: &Path) -> Settings {
    let mut settings = parse("");
    if let Ok(text) = std::fs::read_to_string(dir.join(LEGACY_THEME_FILE)) {
        let name = text.trim();
        if !name.is_empty() {
            let _ = settings.put(THEME, name);
        }
    }
    settings
}

fn set_at(dir: &Path, key: &str, value: &str) -> io::Result<()> {
    let mut settings = read_settings(dir)?;
    migrate_legacy_theme(dir, &mut settings);
    settings.put(key, value)?;
    save(dir, &settings)
}

/// 読む。**無いときだけ空**で、読めないときはエラー。この区別が付いていないと、
/// read-modify-write が読めなかった中身を消す。
///
/// Why not `read_at` と綴る: このファイルで `_at` を付けているのは、置き場所を
/// 引数で受ける private 版が**同名の公開関数と衝突する**ものだけ（[`load`] / [`set`]）。
/// 衝突しないものは `store::read` や `quick_access::save` と同じ無印に揃える。
fn read_settings(dir: &Path) -> io::Result<Settings> {
    match std::fs::read_to_string(dir.join(FILE)) {
        Ok(text) => Ok(parse(&text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(parse("")),
        Err(e) => Err(e),
    }
}

/// 書いたものがそのまま読み戻せる形か。[`parse`] が落とすもの（前後の空白・
/// `#` 始まり・キーの中の `=`・改行）を、書く前に全部断る。
fn round_trips(key: &str, value: &str) -> bool {
    !key.is_empty()
        && key.trim() == key
        && !key.starts_with('#')
        && !key.contains('=')
        && value.trim() == value
        && !has_line_break(key)
        && !has_line_break(value)
}

/// 行の途中で改行しているか。前後の改行は `trim()` 側が捕まえるので、ここが要るのは
/// `a\nb` のような内側のぶん。
fn has_line_break(s: &str) -> bool {
    s.contains('\n') || s.contains('\r')
}

fn parse(text: &str) -> Settings {
    let lines = text
        .lines()
        .map(|raw| {
            let trimmed = raw.trim();
            // `#` で始まる行はコメント。`#theme=nord` が生きたキーに化けないよう、
            // `=` を見るより先に落とす。
            if trimmed.starts_with('#') {
                return Line::Other(raw.to_string());
            }
            match trimmed.split_once('=') {
                Some((key, value)) if !key.trim().is_empty() => {
                    Line::Pair(key.trim().to_string(), value.trim().to_string())
                }
                _ => Line::Other(raw.to_string()),
            }
        })
        .collect();
    Settings { lines }
}

fn save(dir: &Path, settings: &Settings) -> io::Result<()> {
    crate::store::write_text(dir, FILE, &settings.text())
}

/// 旧 `active-theme` を settings へ畳んで消す（#38 の 1 回きりの移行）。
///
/// Why not 呼ぶのを入口（`main.rs` / `cli.rs`）ごとに撒く: 次に足す入口が取りこぼす。
/// テーマを読む経路は必ず [`load`] を通るので、ここに置けば呼び忘れる場所ができない。
///
/// ⚠️ **旧ファイルを消すのは保存が通ってから。** 逆にすると、書けなかったときに
/// テーマがどこにも無い状態が残る（ディスクフル・読み取り専用・保存直前のクラッシュ。
/// 実際に nord が default に化けるのを再現した）。
fn migrate_legacy_theme(dir: &Path, settings: &mut Settings) {
    let path = dir.join(LEGACY_THEME_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let name = text.trim();

    // 既に settings 側にテーマが在るなら、そちらが新しい。旧ファイルは捨てるだけ。
    //
    // Why not 行が在るかどうかだけ見る: `theme=` と手で書いた人の旧ファイルが、
    // 移さないまま消える。読む側（`theme::read_active_name`）は空を「未設定」として
    // default へ畳むので、**空は在ることにならない**。ここの数え方を合わせておく。
    let has_theme = settings.get(THEME).map_or(false, |v| !v.is_empty());
    if !name.is_empty() && !has_theme {
        // 書けない形（手で改行を入れた等）なら諦める。旧ファイルを残すので、
        // 直せば次の起動で移る。
        if settings.put(THEME, name).is_err() {
            return;
        }
        if let Err(e) = save(dir, settings) {
            eprintln!("md: 設定を保存できませんでした: {e}");
            return;
        }
    }
    let _ = std::fs::remove_file(&path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("md-settings-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn write_raw(dir: &Path, name: &str, bytes: &[u8]) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), bytes).unwrap();
    }

    fn text_of(dir: &Path, name: &str) -> String {
        std::fs::read_to_string(dir.join(name)).unwrap()
    }

    #[test]
    fn a_value_survives_a_save_and_a_load() {
        let dir = temp("roundtrip");
        set_at(&dir, THEME, "nord").unwrap();
        assert_eq!(load_at(&dir).get(THEME), Some("nord"));
        set_at(&dir, THEME, "gruvbox").unwrap();
        assert_eq!(load_at(&dir).get(THEME), Some("gruvbox"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 新しい md が足したキーを、古い md が 1 度書いただけで失わないこと。
    #[test]
    fn a_key_nobody_here_knows_survives_a_write() {
        let dir = temp("unknown-key");
        write_raw(&dir, FILE, b"from-the-future=42\ntheme=nord\n");
        set_at(&dir, THEME, "terminal").unwrap();
        assert_eq!(text_of(&dir, FILE), "from-the-future=42\ntheme=terminal\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_comment_and_a_blank_line_survive_a_write() {
        let dir = temp("comment");
        write_raw(&dir, FILE, "# 手で書いたメモ\n\ntheme=nord\n".as_bytes());
        set_at(&dir, "other", "1").unwrap();
        assert_eq!(text_of(&dir, FILE), "# 手で書いたメモ\n\ntheme=nord\nother=1\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_commented_out_key_is_not_a_live_key() {
        assert_eq!(parse("#theme=nord\n").get(THEME), None);
        assert_eq!(parse("  # theme=nord\n").get(THEME), None);
    }

    #[test]
    fn spaces_around_the_equals_sign_do_not_change_the_value() {
        assert_eq!(parse("  theme  =  nord  \n").get(THEME), Some("nord"));
        // 値の中の `=` は値に残る。
        assert_eq!(parse("k=a=b\n").get("k"), Some("a=b"));
    }

    #[test]
    fn the_first_of_two_lines_with_the_same_key_is_the_one_that_counts() {
        let mut s = parse("theme=a\ntheme=b\n");
        assert_eq!(s.get(THEME), Some("a"));
        // 読む側と書く側が同じ行を見ていること。
        s.put(THEME, "c").unwrap();
        assert_eq!(s.text(), "theme=c\ntheme=b\n");
        assert_eq!(s.get(THEME), Some("c"));
    }

    /// 通せば次に読めなくなる key/value は、書く前に断ること。通すと `put` が
    /// 既存の行を見つけられず、呼ぶたびに 1 行ずつ伸びる。
    #[test]
    fn a_key_or_value_that_would_not_read_back_is_refused() {
        let mut s = parse("");
        for (key, value) in [
            ("theme", "nord\nextra=1"), // 値の改行 → 別のキーが生える
            ("theme", "nord\rx"),
            ("theme", "  nord"), // 前後の空白 → 読み戻すと落ちる
            ("theme", "nord  "),
            (" theme", "nord"),
            ("theme ", "nord"),
            ("", "nord"),
            ("a=b", "nord"),
            ("#theme", "nord"), // コメントとして落ちる
            ("a\nb", "nord"),
        ] {
            assert!(s.put(key, value).is_err(), "{key:?}={value:?} を通している");
        }
        assert_eq!(s.text(), "", "断ったものが入っている");
        // 空の値は読み戻せるので通す（テーマ側が default へ畳む）。
        s.put(THEME, "").unwrap();
        assert_eq!(parse(&s.text()).get(THEME), Some(""));
    }

    #[test]
    fn the_old_active_theme_file_moves_in_and_disappears() {
        let dir = temp("migrate");
        write_raw(&dir, LEGACY_THEME_FILE, b"nord\n");
        assert_eq!(load_at(&dir).get(THEME), Some("nord"));
        assert!(!dir.join(LEGACY_THEME_FILE).exists(), "旧ファイルが残っている");
        assert_eq!(text_of(&dir, FILE), "theme=nord\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 既に settings 側で選んであるなら、置き去りの旧ファイルに巻き戻されないこと。
    #[test]
    fn the_settings_file_wins_over_a_leftover_active_theme() {
        let dir = temp("migrate-conflict");
        write_raw(&dir, FILE, b"theme=gruvbox\n");
        write_raw(&dir, LEGACY_THEME_FILE, b"nord\n");
        assert_eq!(load_at(&dir).get(THEME), Some("gruvbox"));
        assert!(!dir.join(LEGACY_THEME_FILE).exists(), "旧ファイルが残っている");
        assert_eq!(text_of(&dir, FILE), "theme=gruvbox\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 手で改行を入れた旧ファイルが、別のキーに化けて入らないこと。
    #[test]
    fn a_hand_broken_active_theme_never_becomes_a_second_key() {
        let dir = temp("migrate-injection");
        write_raw(&dir, LEGACY_THEME_FILE, b"nord\nextra=1\n");
        let got = load_at(&dir);
        assert_eq!(got.get("extra"), None, "旧ファイルからキーが生えている");
        assert_eq!(got.get(THEME), None);
        // 移せなかったので旧ファイルは残る（手で直せば次に移る）。
        assert!(dir.join(LEGACY_THEME_FILE).exists());
        assert!(!dir.join(FILE).exists(), "移せていないのに settings を作っている");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn set_writable(dir: &Path, writable: bool) {
        let mut perm = std::fs::metadata(dir).unwrap().permissions();
        perm.set_readonly(!writable);
        std::fs::set_permissions(dir, perm).unwrap();
    }

    /// 保存が通らないときに旧ファイルを消さないこと。消すとテーマがどこにも
    /// 無くなる（このテストが防いでいるデータ損失）。
    #[test]
    fn a_failed_save_keeps_the_old_file_so_the_theme_is_not_lost() {
        let dir = temp("migrate-save-fails");
        write_raw(&dir, LEGACY_THEME_FILE, b"nord\n");
        // 書き込みだけを失敗させる。settings 自体は「無い」ままなので、読む側は
        // 通り、移行が保存で転ぶ経路に入る。
        set_writable(&dir, false);

        assert_eq!(load_at(&dir).get(THEME), Some("nord"), "今回のテーマまで失っている");
        assert!(dir.join(LEGACY_THEME_FILE).exists(), "書けていないのに旧ファイルを消した");
        assert!(!dir.join(FILE).exists());

        // 書けるようになったら移行できること。
        set_writable(&dir, true);
        assert_eq!(load_at(&dir).get(THEME), Some("nord"));
        assert!(!dir.join(LEGACY_THEME_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_active_theme_is_dropped_without_writing_anything() {
        let dir = temp("migrate-empty");
        write_raw(&dir, LEGACY_THEME_FILE, b"\n");
        assert_eq!(load_at(&dir).get(THEME), None);
        assert!(!dir.join(LEGACY_THEME_FILE).exists());
        assert!(!dir.join(FILE).exists(), "空から settings を作っている");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// read-modify-write が、読めなかった中身を巻き添えに消さないこと。
    #[test]
    fn a_settings_file_that_cannot_be_read_is_never_overwritten() {
        let dir = temp("unreadable");
        write_raw(&dir, FILE, b"theme=nord\nnote=\xff\xfe binary\n");
        let before = std::fs::read(dir.join(FILE)).unwrap();

        assert!(set_at(&dir, THEME, "dracula").is_err(), "読めないファイルに書いている");
        assert_eq!(std::fs::read(dir.join(FILE)).unwrap(), before, "中身が消えている");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 書く側がエラーで止まるのに対し、**読む側は空として続行する**（起動経路で
    /// 止まると窓が出ない）。割った両側それぞれに番人を置く。
    #[test]
    fn an_unreadable_file_still_lets_the_app_start() {
        let dir = temp("unreadable-read");
        write_raw(&dir, FILE, b"theme=nord\nnote=\xff\xfe binary\n");
        assert_eq!(load_at(&dir).get(THEME), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 読むだけの経路でも消さないこと。移行は書く側なので、読めなかった空を渡すと
    /// `md theme` の一覧表示だけで設定が全部飛ぶ（このテストが防いでいるデータ損失）。
    #[test]
    fn an_unreadable_file_is_not_wiped_by_the_migration_either() {
        let dir = temp("unreadable-migrate");
        write_raw(&dir, FILE, b"theme=nord\nnote=\xff\xfe binary\nkeep=me\n");
        write_raw(&dir, LEGACY_THEME_FILE, b"gruvbox\n");
        let before = std::fs::read(dir.join(FILE)).unwrap();

        let _ = load_at(&dir);
        assert_eq!(std::fs::read(dir.join(FILE)).unwrap(), before, "読んだだけで消えている");
        assert!(dir.join(LEGACY_THEME_FILE).exists(), "移せていないのに旧ファイルを消した");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 手で `theme=` と書いてある状態は「選んでいない」。旧ファイルの名前を
    /// 捨てずに移すこと（読む側が空を default へ畳むのと数え方を揃える）。
    #[test]
    fn an_empty_theme_line_does_not_count_as_a_choice() {
        let dir = temp("empty-theme");
        write_raw(&dir, FILE, b"theme=\n");
        write_raw(&dir, LEGACY_THEME_FILE, b"nord\n");
        assert_eq!(load_at(&dir).get(THEME), Some("nord"), "旧ファイルの名前を捨てた");
        assert!(!dir.join(LEGACY_THEME_FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reading_what_was_never_written_is_empty_and_leaves_no_directory() {
        let dir = temp("missing");
        assert_eq!(load_at(&dir).get(THEME), None);
        assert!(!dir.exists(), "読んだだけでディレクトリを作っている");
    }
}
