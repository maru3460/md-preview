//! `md uninstall` — md がディスクへ置いたものを片付ける。
//!
//! `cargo uninstall` は `~/.cargo/bin/md` しか消さないので、設定と WebKit の
//! データが残る。ここで消すのはその残るぶんで、**本体の削除は cargo に委譲する**。
//!
//! 自分で unlink しないのは、cargo が `~/.cargo/.crates.toml` と
//! `.crates2.json` に「どの crate がどの bin を置いたか」を記録しているため。
//! バイナリだけ消すと台帳とズレて、`cargo install --list` に幽霊が残り、後から
//! `cargo uninstall` を打つと失敗する。「消したのに消えていない」状態を作る。

use std::io::Write;
use std::path::{Path, PathBuf};

/// 消す対象 1 件。
pub struct Target {
    pub path: PathBuf,
    /// 一覧に出す説明。「これは何で、消すと何が起きるか」が分かる粒度にする。
    pub label: &'static str,
    pub bytes: u64,
}

/// 一覧に出す対象を組み立てる。存在しないものは含めない。
///
/// `home` と `exe_name` を引数で受けるのは、テストが実物の HOME を触らずに
/// 済ませるため。`exe_name` は WebKit のデータ置き場の名前になる（下記参照）。
pub fn plan(home: &Path, exe_name: &str) -> Vec<Target> {
    let config = home.join(".config/md-preview");
    let webkit = home.join("Library/WebKit").join(exe_name);
    let caches = home.join("Library/Caches").join(exe_name);

    // WebKit 側の 2 つは名前が md 専用の名前空間ではない（後述の
    // `webkit_dir_is_ours` を参照）。所有権は WebsiteData を持つ方で 1 度だけ
    // 判定し、キャッシュ側もその判定に従わせる。同じ名前で決まる以上、片方が
    // 他人のものならもう片方も他人のものだからである。
    let ours = webkit_dir_is_ours(&webkit);

    let mut out = Vec::new();
    let mut push = |path: PathBuf, label: &'static str| {
        if path.exists() {
            let bytes = dir_size(&path);
            out.push(Target { path, label, bytes });
        }
    };

    push(config, "テーマ設定・ユーザー CSS・IME 用のバンドル");
    if ours {
        push(webkit, "UI の状態（オンボーディング済みフラグなど）");
        push(caches, "WebKit のキャッシュ");
    }
    out
}

/// `~/Library/WebKit/<name>` と `~/Library/Caches/<name>` の `<name>` は、
/// `CFBundleIdentifier` を持たないアプリの既定＝**実行ファイル名そのまま**で
/// 決まる。md 専用の名前空間ではないので、`md` という名前の別のプログラムが
/// あれば同じ場所を共有する。名前が一致しただけのディレクトリを `rm -rf` しない
/// よう、中身で確かめる。
///
/// 判定はオリジンの記録で行う。`mdpreview://` のものが 1 つでもあれば md が
/// 使った跡。1 つも無ければサイトデータ自体が無いので、消して失うものも無い
/// （＝どちらにせよ安全）。他人のオリジンだけがあるときは触らない。
fn webkit_dir_is_ours(dir: &Path) -> bool {
    let mut found_any = false;
    let mut found_ours = false;
    visit_origins(dir, 0, &mut |bytes| {
        found_any = true;
        // オリジンの記録は長さ接頭辞つきの生バイト列だが、スキーム名はそのまま
        // ASCII で載る。文字列としてパースせずバイト列で探す。
        if bytes.windows(9).any(|w| w == b"mdpreview") {
            found_ours = true;
        }
    });
    !found_any || found_ours
}

/// `dir` 以下の `origin` という名前のファイルを訪ねる。WebKit の置き方は
/// `WebsiteData/Default/<hash>/<hash>/origin` なので、深さは 5 で足りる。
/// 上限を切るのは、他人のディレクトリを掴んだときに深追いしないため。
fn visit_origins(dir: &Path, depth: usize, f: &mut impl FnMut(&[u8])) {
    if depth > 5 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        // symlink は辿らない。外を指すリンクで判定を誘導されないため。
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            visit_origins(&path, depth + 1, f);
        } else if kind.is_file() && path.file_name().map(|n| n == "origin").unwrap_or(false) {
            if let Ok(bytes) = std::fs::read(&path) {
                f(&bytes);
            }
        }
    }
}

/// ディレクトリ以下の合計バイト数。ハードリンクは重複して数えるが、一覧に出す
/// 概算なので気にしない。
fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.is_symlink() {
        return 0;
    }
    if meta.is_file() {
        return meta.len();
    }
    let Ok(entries) = std::fs::read_dir(path) else { return 0 };
    entries.flatten().map(|e| dir_size(&e.path())).sum()
}

/// `4.0K` `15M` のような読みやすい大きさ。
pub fn human_size(bytes: u64) -> String {
    const K: u64 = 1024;
    match bytes {
        b if b < K => format!("{}B", b),
        b if b < K * K => format!("{:.0}K", b as f64 / K as f64),
        b if b < K * K * K => format!("{:.1}M", b as f64 / (K * K) as f64),
        b => format!("{:.1}G", b as f64 / (K * K * K) as f64),
    }
}

/// `cargo uninstall` が実際に消す場所。いま走っている実行ファイル（`cargo run`
/// なら `target/debug/md`、乗り換え後なら `app/md` の symlink）とは限らないので、
/// 一覧には cargo が触る方を出す。出す場所と消える場所が違うと嘘になる。
fn cargo_bin(home: &Path, exe_name: &str) -> PathBuf {
    std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cargo"))
        .join("bin")
        .join(exe_name)
}

/// 一覧のテキスト。実行前の確認にも `--dry-run` にも同じものを使う。
pub fn plan_text(targets: &[Target], crate_name: &str, bin: &Path) -> String {
    let mut rows: Vec<(String, String, String)> = targets
        .iter()
        .map(|t| (t.path.display().to_string(), human_size(t.bytes), t.label.to_string()))
        .collect();
    rows.push((
        bin.display().to_string(),
        human_size(dir_size(bin)),
        format!("本体（cargo uninstall {} で消します）", crate_name),
    ));

    // 桁は実際のパスに合わせる。固定幅にすると HOME の長さで崩れる。
    let width = rows.iter().map(|(p, _, _)| p.chars().count()).max().unwrap_or(0);
    let mut s = String::from("md が置いたものを消します:\n\n");
    for (path, size, label) in rows {
        let pad = " ".repeat(width - path.chars().count());
        s.push_str(&format!("  {}{}  {:>6}   {}\n", path, pad, size, label));
    }
    s
}

pub fn run(rest: &[String]) {
    let (dry_run, assume_yes) = match rest {
        [] => (false, false),
        [flag] if flag == "--dry-run" => (true, false),
        [flag] if flag == "--yes" || flag == "-y" => (false, true),
        _ => {
            eprintln!("使い方: md uninstall [--dry-run | --yes]");
            std::process::exit(1);
        }
    };

    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        eprintln!("md: HOME が読めないため、何が消せるか判断できません");
        std::process::exit(1);
    };
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("md"));
    let exe_name = exe.file_name().and_then(|n| n.to_str()).unwrap_or("md").to_string();

    let targets = plan(&home, &exe_name);
    let crate_name = env!("CARGO_PKG_NAME");
    // cargo が置いた本体が見つからないとき（開発中の直接実行など）は、せめて
    // いま走っているものを出す。
    let bin = cargo_bin(&home, &exe_name);
    let bin = if bin.exists() { bin } else { exe };
    print!("{}", plan_text(&targets, crate_name, &bin));

    if dry_run {
        println!("\n（--dry-run なので何も消していません）");
        return;
    }
    if !assume_yes && !confirm() {
        println!("やめました。");
        return;
    }

    for t in &targets {
        if let Err(e) = std::fs::remove_dir_all(&t.path) {
            eprintln!("md: {} を消せませんでした: {}", t.path.display(), e);
        }
    }

    // 本体は cargo に任せる。cargo が居ないときは掃除だけで正常終了し、残りの
    // 手順を伝える（ここで失敗扱いにすると、掃除は済んでいるのに赤く出る）。
    match std::process::Command::new("cargo").arg("uninstall").arg(crate_name).status() {
        Ok(status) if status.success() => println!("\n片付けました。"),
        Ok(_) => {
            println!("\n設定とデータは消しました。");
            println!("本体は cargo uninstall {} で消してください。", crate_name);
        }
        Err(_) => {
            println!("\n設定とデータは消しました。cargo が見つかりません。");
            println!("本体は cargo uninstall {} で消してください。", crate_name);
        }
    }
}

/// 端末から y を受け取る。端末が無い（パイプ・エージェント経由）ときは同意を
/// 取れないので消さない。無人で走らせたいときは `--yes` を明示する。
fn confirm() -> bool {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        eprintln!("\nmd: 端末が無いため確認が取れません。--yes を付けてください。");
        std::process::exit(1);
    }
    print!("\n続けますか？ [y/N] ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim(), "y" | "Y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `WebsiteData/Default/<hash>/<hash>/origin` を 1 つ作る。
    fn write_origin(root: &Path, name: &str, content: &[u8]) {
        let dir = root.join("WebsiteData/Default").join(name).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("origin"), content).unwrap();
    }

    fn temp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("md-uninstall-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn webkit_dir_with_our_origin_is_ours() {
        let dir = temp("ours");
        write_origin(&dir, "aaa", b"\x00\x09mdpreview\x09localhost");
        assert!(webkit_dir_is_ours(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn webkit_dir_with_only_foreign_origins_is_left_alone() {
        // `md` という名前の別のプログラムが同じ場所を使っている状況。
        let dir = temp("foreign");
        write_origin(&dir, "bbb", b"\x00\x09https\x09example.com");
        assert!(!webkit_dir_is_ours(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn webkit_dir_without_any_origin_is_safe_to_remove() {
        // サイトデータが無いので、誰のものでも失うものが無い。
        let dir = temp("empty");
        std::fs::create_dir_all(dir.join("WebsiteData/Default")).unwrap();
        assert!(webkit_dir_is_ours(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plan_skips_what_is_not_there_and_lists_what_is() {
        let home = temp("plan");
        // 設定だけがある状態（テーマを一度も切り替えていなければこれも無い）。
        std::fs::create_dir_all(home.join(".config/md-preview")).unwrap();
        std::fs::write(home.join(".config/md-preview/active-theme"), b"nord").unwrap();

        let targets = plan(&home, "md");
        assert_eq!(targets.len(), 1, "存在しない Library 側まで並べている");
        assert!(targets[0].path.ends_with(".config/md-preview"));
        assert_eq!(targets[0].bytes, 4);

        // WebKit 側が md のものとして存在すれば 3 件になる。
        let webkit = home.join("Library/WebKit/md");
        write_origin(&webkit, "ccc", b"mdpreview");
        std::fs::create_dir_all(home.join("Library/Caches/md")).unwrap();
        assert_eq!(plan(&home, "md").len(), 3);

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn plan_leaves_a_foreign_library_dir_out_entirely() {
        let home = temp("plan-foreign");
        std::fs::create_dir_all(home.join(".config/md-preview")).unwrap();
        let webkit = home.join("Library/WebKit/md");
        write_origin(&webkit, "ddd", b"https\x09example.com");
        std::fs::create_dir_all(home.join("Library/Caches/md")).unwrap();

        let targets = plan(&home, "md");
        assert_eq!(targets.len(), 1, "他人の WebKit/Caches を消そうとしている");

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn plan_text_aligns_on_the_longest_path_and_names_the_binary() {
        let home = temp("text");
        std::fs::create_dir_all(home.join(".config/md-preview")).unwrap();
        let targets = plan(&home, "md");
        let text = plan_text(&targets, "md-preview", &home.join(".cargo/bin/md"));

        // 大きさの列が全行で同じ位置から始まる（HOME の長さに引きずられない）。
        let cols: Vec<usize> = text
            .lines()
            .filter(|l| l.starts_with("  /"))
            .map(|l| l.rfind("   ").unwrap())
            .collect();
        assert!(cols.len() >= 2);
        assert!(cols.windows(2).all(|w| w[0] == w[1]), "桁が揃っていない:\n{text}");
        assert!(text.contains("cargo uninstall md-preview"));

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn sizes_are_readable() {
        assert_eq!(human_size(4), "4B");
        assert_eq!(human_size(4096), "4K");
        assert_eq!(human_size(15 * 1024 * 1024), "15.0M");
    }
}
