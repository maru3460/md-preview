//! macOS で日本語入力の変換候補パネルを出すための、最小のバンドルへの乗り換え。
//!
//! 素の実行ファイル（`cargo install` が置く `~/.cargo/bin/md`）は macOS から
//! アプリとして認識されない。すると LaunchServices へ身元を届け出できず、入力
//! メソッドが変換候補のウィンドウを組み立てられない（変換自体は入力メソッドの
//! 中で完結するので動く。候補リストだけが出ない）。
//!
//! 実測で成立する条件は 1 つだけだった:
//!
//! > **起動に使われたパスの隣に、ディスク上の実ファイルとして `Info.plist` が
//! > あること**
//!
//! そこで、その形のディレクトリを用意してから自分をそこから起動し直す。
//!
//! # なぜこの形なのか（実測で潰した選択肢）
//!
//! - **`.app` にしない**: LaunchServices の DB に永続エントリが残る（消したパスの
//!   登録が残り続けるのを実測）。Spotlight / Launchpad / Finder にも出る。拡張子も
//!   `Contents/MacOS/` 構造も不要で、実行ファイルと `Info.plist` を並べるだけで通る
//! - **`CFBundleIdentifier` を書かない**: WebKit のデータ置き場は「identifier が
//!   あればそれ、無ければ実行ファイル名」で決まる。書いた瞬間に既存の
//!   `~/Library/WebKit/md` と `~/Library/Caches/md` が孤児になる
//! - **コピーではなく symlink**: `cargo install` で本体が差し替わっても追随する。
//!   鮮度の比較も、9.6MB の二重持ちも要らない
//! - **プロセスの中から名乗るのは効かない**: `NSBundle` への注入も、Mach-O への
//!   `__TEXT,__info_plist` の焼き込みも実測で不成立。ディスク上に実ファイルが要る

use std::path::Path;

/// バンドル内の実行ファイル名。`Info.plist` の `CFBundleExecutable` と一致して
/// いなければならない（一致しないと macOS はバンドルと認めない）。
const BIN_NAME: &str = "md";

/// バンドルのディレクトリ名。**これがそのまま Dock やホバーに出るアプリ名になる。**
///
/// `Info.plist` の `CFBundleDisplayName` / `CFBundleName` は効かない（実測。書いても
/// `app` のままだった）。`.app` ではないディレクトリを LaunchServices はアプリの
/// バンドルとして扱いきらず、名前もアイコンもファイルシステム上のアイテムから
/// 取る。だから直す場所は plist ではなくここ。
const APP_DIR_NAME: &str = "md";

/// 名前を `app` から [`APP_DIR_NAME`] へ変える前に使っていたディレクトリ。
/// 見つけたら消す。
const OLD_APP_DIR_NAME: &str = "app";

/// 乗り換え済みの目印。`already_inside` の stat が失敗して判定が狂っても exec が
/// 繰り返されないようにするための、構造的な歯止め。
const RELAUNCHED_ENV: &str = "MD_BUNDLED";

/// 機構ごと無効にする逃げ道。
const DISABLE_ENV: &str = "MD_NO_BUNDLE";

/// 必要なキーは実測では `CFBundleExecutable` が実在する隣のファイルを指している
/// こと 1 つだけ。`NSHighResolutionCapable` は保険で、plist が無い今は AppKit の
/// 既定で Retina 描画になっているが、置くと「キーが無い = NO」と読まれる恐れがある。
///
/// Why not **`CFBundleDisplayName` / `CFBundleIconFile` を足す**: 実測で**どちらも
/// 読まれない**。前者を書いても Dock のホバーは `app`（＝ディレクトリ名）のまま、
/// 後者で隣に `.icns` を置いて指しても、終了してタイルが畳まれる間のアイコンは
/// 汎用のまま化けた。`.app` ではないディレクトリを LaunchServices はアプリの
/// バンドルとして扱いきらず、名前もアイコンも**ファイルシステム上のアイテム**から
/// 取っている。名前は [`APP_DIR_NAME`] を変えて直した。アイコンは #40 へ。
///
/// 下に残っている `CFBundleName` も同じ理由で読まれていない（これが `md` なのに
/// Dock には `app` が出ていた）。**残しているのは害が無いからで、効くからではない。**
/// 名前をここで直せると思って触らないこと。
const INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>md</string>
  <key>CFBundleName</key><string>md</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>NSHighResolutionCapable</key><true/>
</dict></plist>
"#;

/// バンドルを用意して、そこから自分を起動し直す。成功するとこの関数からは戻らない。
///
/// 用意にも exec にも失敗したら、黙って素のまま続行する。候補リストが出ないだけで
/// 全機能は動くので、ここで終了させる価値は無い。
///
/// ウィンドウを開かない経路（`--help` / `--version` / `--sample` / `md theme` /
/// `--html`）から呼んではいけない。文字を出すだけのコマンドがディレクトリを作り、
/// プロセスを入れ替えることになる。
#[cfg(target_os = "macos")]
pub fn relaunch_in_flat_bundle() {
    if std::env::var_os(DISABLE_ENV).is_some() || std::env::var_os(RELAUNCHED_ENV).is_some() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let Some(app_dir) = app_dir() else { return };
    if already_inside(&exe, &app_dir) {
        return;
    }
    if prepare(&app_dir, &exe).is_err() {
        return;
    }

    use std::os::unix::process::CommandExt;
    // stdin/stdout/stderr と process_group は触らない。exec は fd をそのまま
    // 引き継ぐので、パイプ入力も端末も繋がったまま乗り換わる。
    let _ = std::process::Command::new(app_dir.join(BIN_NAME))
        .args(std::env::args_os().skip(1))
        .env(RELAUNCHED_ENV, "1")
        .exec();
}

#[cfg(not(target_os = "macos"))]
pub fn relaunch_in_flat_bundle() {}

/// 自分が既にバンドルの中から起動されているか。
///
/// パスの前方一致では判定できない。`current_exe()` は macOS では symlink も `..`
/// も解決せず、exec に使われたパスをそのまま返すため、`app/../app/md` のような
/// 形で簡単に破れる。ディレクトリの実体（dev + ino）で比べる。
#[cfg(target_os = "macos")]
fn already_inside(exe: &Path, app_dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let Some(parent) = exe.parent() else { return false };
    let (Ok(a), Ok(b)) = (std::fs::metadata(parent), std::fs::metadata(app_dir)) else {
        return false;
    };
    a.dev() == b.dev() && a.ino() == b.ino()
}

/// バンドルの置き場所 `~/.config/md-preview/md`。
fn app_dir() -> Option<std::path::PathBuf> {
    crate::config_dir().map(|d| d.join(APP_DIR_NAME))
}

/// `app_dir` に `Info.plist` と本体への symlink を揃える。
///
/// どちらも中身が既に正しければ触らない。起動のたびに書き直すと、`cargo install`
/// と競合したときに壊れた状態を掴む窓が広がる。
#[cfg(target_os = "macos")]
fn prepare(app_dir: &Path, exe: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(app_dir)?;

    // 名前を変える前の置き場所を片付ける。残しておくと「`app` って何」がずっと
    // 残るので、見つけた側が消す。
    //
    // 中身の形を確かめてから消す。`~/.config/md-preview` は md の名前空間なので
    // 素朴に消しても実害は無いはずだが、`uninstall.rs` の `webkit_dir_is_ours` が
    // 同じ流儀（自分が作った形だと確認してから消す）を採っているので揃える。
    //
    // Why not **一度きりにする**: 済んだ印を置くと、それ自体が次に「これ何」に
    // なる。毎起動の `exists` 1 回は無視できる。旧版の md と併用すると
    // 「旧版が作る → 新版が消す」で振動するが、旧版も次の起動で作り直すので
    // 自己修復する（壊れるのは旧版の IME 候補が 1 回出ないことだけ）。
    if let Some(old) = app_dir.parent().map(|p| p.join(OLD_APP_DIR_NAME)) {
        if old != app_dir && old.join("Info.plist").is_file() && old.join(BIN_NAME).is_symlink() {
            let _ = std::fs::remove_dir_all(old);
        }
    }

    let plist = app_dir.join("Info.plist");
    if std::fs::read_to_string(&plist).map(|s| s != INFO_PLIST).unwrap_or(true) {
        std::fs::write(&plist, INFO_PLIST)?;
    }

    let link = app_dir.join(BIN_NAME);
    if std::fs::read_link(&link).map(|t| t != exe).unwrap_or(true) {
        // 張り替えは remove してから。symlink(2) は既存パスがあると必ず失敗する。
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(exe, &link)?;
    }
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn info_plist_points_at_the_neighbouring_executable() {
        // ここがズレると macOS はバンドルと認めない。
        assert!(INFO_PLIST.contains(&format!("<string>{}</string>", BIN_NAME)));
        assert!(INFO_PLIST.contains("CFBundleExecutable"));
        // identifier を書くと WebKit のデータ置き場が変わってしまう。
        assert!(!INFO_PLIST.contains("CFBundleIdentifier"));
    }

    #[test]
    fn prepare_creates_the_bundle_and_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("md-bundle-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = dir.join(APP_DIR_NAME);
        let exe = dir.join("fake-md");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&exe, b"x").unwrap();

        prepare(&app, &exe).unwrap();
        assert_eq!(std::fs::read_to_string(app.join("Info.plist")).unwrap(), INFO_PLIST);
        assert_eq!(std::fs::read_link(app.join(BIN_NAME)).unwrap(), exe);

        // 2 回目は既に正しいので触らない。壊れもしない。
        prepare(&app, &exe).unwrap();
        assert_eq!(std::fs::read_link(app.join(BIN_NAME)).unwrap(), exe);

        // 本体が別の場所へ移ったら張り替える（cargo install で入れ替わった時）。
        let moved = dir.join("fake-md-2");
        std::fs::write(&moved, b"x").unwrap();
        prepare(&app, &moved).unwrap();
        assert_eq!(std::fs::read_link(app.join(BIN_NAME)).unwrap(), moved);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prepare_removes_the_bundle_that_used_the_old_name() {
        let dir = std::env::temp_dir().join(format!("md-oldname-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let exe = dir.join("fake-md");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&exe, b"x").unwrap();

        // 旧版が作った形（Info.plist と本体への symlink）を置く。
        let old = dir.join(OLD_APP_DIR_NAME);
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("Info.plist"), INFO_PLIST).unwrap();
        std::os::unix::fs::symlink(&exe, old.join(BIN_NAME)).unwrap();
        // 隣にある、md のものではないディレクトリ。
        let stranger = dir.join("themes");
        std::fs::create_dir_all(&stranger).unwrap();

        prepare(&dir.join(APP_DIR_NAME), &exe).unwrap();

        assert!(!old.exists(), "旧名のバンドルは消える");
        assert!(stranger.exists(), "形が違う隣人には手を出さない");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preparing_into_the_old_name_does_not_delete_itself() {
        // `app_dir` 自身が旧名だったときに自爆しないこと。いまの `app_dir()` は
        // 新しい名前を返すので通らない道だが、**自分を消す分岐が存在しない**ことを
        // 固定しておく（ここが壊れると起動のたびにバンドルが消える）。
        let dir = std::env::temp_dir().join(format!("md-selfdel-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let exe = dir.join("fake-md");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&exe, b"x").unwrap();

        let app = dir.join(OLD_APP_DIR_NAME);
        prepare(&app, &exe).unwrap();
        prepare(&app, &exe).unwrap();

        assert_eq!(std::fs::read_link(app.join(BIN_NAME)).unwrap(), exe);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn already_inside_survives_a_dot_dot_path() {
        let dir = std::env::temp_dir().join(format!("md-inside-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = dir.join(APP_DIR_NAME);
        std::fs::create_dir_all(&app).unwrap();

        // 外から起動された（＝乗り換えが要る）。
        assert!(!already_inside(&dir.join("md"), &app));
        // バンドルの中から起動された。
        assert!(already_inside(&app.join("md"), &app));
        // `..` を挟んでも同じディレクトリだと分かる（文字列比較では破れる形）。
        assert!(already_inside(&app.join("..").join(APP_DIR_NAME).join("md"), &app));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
