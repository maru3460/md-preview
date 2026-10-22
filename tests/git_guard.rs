//! git が開いたフォルダの設定でコマンドを起動しないこと。
//!
//! リポジトリ側に「起動されたら目印のファイルを作るコマンド」を仕込み、差分の 3 つの入口
//! （`render_diff_inner`・`diff_stat`・`changed_files`）を叩いても、目印が 1 つもできないこと。
//! 止まらなくなるリポジトリでも時間内に返り、子プロセスが残らないこと。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use md_preview::diff::{changed_files, diff_stat, render_diff_inner};

/// 一時の作業場所。抜けるときに消す。
struct Fixture {
    dir: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("md-git-guard-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("marks")).unwrap();
        Fixture { dir: dir.canonicalize().unwrap() }
    }

    fn marks(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.dir.join("marks"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    /// 起動されたら `marks/<name>` を作り、標準入力をそのまま出すスクリプト（filter にも使える）。
    fn script(&self, name: &str) -> PathBuf {
        let path = self.dir.join(format!("{name}.sh"));
        std::fs::write(&path, format!("#!/bin/sh\ntouch '{}/marks/{name}'\ncat\n", self.dir.display())).unwrap();
        let mut perm = std::fs::metadata(&path).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        std::fs::set_permissions(&path, perm).unwrap();
        path
    }

    /// `a.md` をコミットし、作業ツリーで書き換えた（差分のある）リポジトリ。
    fn repo(&self, name: &str) -> PathBuf {
        let repo = self.dir.join(name);
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        std::fs::write(repo.join("a.md"), "# a\n").unwrap();
        git(&repo, &["add", "a.md"]);
        git(&repo, &["commit", "-qm", "init"]);
        std::fs::write(repo.join("a.md"), "# a\nchanged\n").unwrap();
        std::fs::write(repo.join("new.md"), "new\n").unwrap();
        repo
    }
}

/// テストの準備に使う素の git（守りの外。仕込む側なので）。
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.email=t@example.com", "-c", "user.name=t", "-c", "commit.gpgsign=false"])
        .args(["-c", "protocol.file.allow=always"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {:?}: {}", args, String::from_utf8_lossy(&out.stderr));
}

/// 3 つの入口を全部叩く。それぞれ `limit` 以内に返ること。
fn hit_all(repo: &Path, limit: Duration) -> String {
    let file = repo.join("a.md");
    let start = Instant::now();
    let diff = render_diff_inner(&file);
    diff_stat(&file);
    diff_stat(&repo.join("new.md"));
    changed_files(repo);
    let took = start.elapsed();
    assert!(took < limit, "{} が {took:?} かかった", repo.display());
    diff
}

const FAST: Duration = Duration::from_secs(5);

#[test]
fn repository_config_launches_nothing() {
    let fx = Fixture::new("vectors");

    // fsmonitor のフック。
    let r = fx.repo("fsmonitor");
    git(&r, &["config", "core.fsmonitor", fx.script("fsmonitor").to_str().unwrap()]);
    hit_all(&r, FAST);

    // filter（clean / smudge / process、required 付き）。
    let r = fx.repo("filter");
    std::fs::write(r.join(".gitattributes"), "*.md filter=evil\n").unwrap();
    for part in ["clean", "smudge", "process"] {
        git(&r, &["config", &format!("filter.evil.{part}"), fx.script(&format!("filter-{part}")).to_str().unwrap()]);
    }
    git(&r, &["config", "filter.evil.required", "true"]);
    hit_all(&r, FAST);

    // 名前に `=` を含む filter（`-c` で渡すとキーと値の切れ目を取り違える）。
    let r = fx.repo("eq");
    std::fs::write(r.join(".gitattributes"), "*.md filter=a=b\n").unwrap();
    git(&r, &["config", "filter.a=b.clean", fx.script("filter-eq").to_str().unwrap()]);
    hit_all(&r, FAST);

    // フック（index を書いたときに走る post-index-change。`.git/hooks` と core.hooksPath の両方）。
    // いま使っているコマンドはどれも index を書かないので、hooksPath を止めなくても走らない。
    // index を書くコマンドを足したときに気づくための見張りとして置いている。
    let r = fx.repo("hooks");
    let hook = std::fs::read_to_string(fx.script("hook")).unwrap();
    for dir in [r.join(".git/hooks"), fx.dir.join("hooks-elsewhere")] {
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("post-index-change");
        std::fs::write(&p, &hook).unwrap();
        let mut perm = std::fs::metadata(&p).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
        std::fs::set_permissions(&p, perm).unwrap();
    }
    git(&r, &["config", "core.hooksPath", fx.dir.join("hooks-elsewhere").to_str().unwrap()]);
    hit_all(&r, FAST);

    // サブモジュールの中の filter（親の一覧からは入らず、中のファイルを直接見ても起動しない）。
    let r = fx.repo("super");
    let sub = r.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    git(&sub, &["init", "-q"]);
    std::fs::write(sub.join("s.md"), "s\n").unwrap();
    git(&sub, &["add", "s.md"]);
    git(&sub, &["commit", "-qm", "sub"]);
    git(&r, &["add", "sub"]);
    git(&r, &["commit", "-qm", "add sub"]);
    std::fs::write(sub.join(".gitattributes"), "*.md filter=subevil\n").unwrap();
    git(&sub, &["config", "filter.subevil.clean", fx.script("submodule-filter").to_str().unwrap()]);
    std::fs::write(sub.join("s.md"), "s\nchanged\n").unwrap();
    hit_all(&r, FAST);
    render_diff_inner(&sub.join("s.md"));
    diff_stat(&sub.join("s.md"));

    // 一度きれいな状態で読んだ後に仕込まれた filter（設定の読み取りを覚えていても、
    // `.git/config` の更新で読み直し、include のある設定は覚えないこと）。
    let r = fx.repo("later");
    hit_all(&r, FAST);
    std::fs::write(r.join(".gitattributes"), "*.md filter=late\n").unwrap();
    git(&r, &["config", "filter.late.clean", fx.script("filter-clean").to_str().unwrap()]);
    hit_all(&r, FAST);
    // include の先に後から filter を足す。読んだ時点の先には filter が無いので、覚えが古い
    // ままだと空にされずに起動する。
    let inc = fx.dir.join("late.inc");
    std::fs::write(&inc, "[x]\n\ty = z\n").unwrap();
    git(&r, &["config", "include.path", inc.to_str().unwrap()]);
    hit_all(&r, FAST);
    std::fs::write(r.join(".gitattributes"), "*.md filter=inc\n").unwrap();
    std::fs::write(&inc, format!("[filter \"inc\"]\n\tclean = {}\n", fx.script("filter-clean").display())).unwrap();
    hit_all(&r, FAST);

    assert_eq!(fx.marks(), Vec::<String>::new(), "リポジトリの設定でコマンドが起動した");
}

#[test]
fn lazy_fetch_is_not_attempted_and_shows_as_unavailable() {
    let fx = Fixture::new("lazy");
    let origin = fx.repo("origin");
    git(&origin, &["add", "a.md"]);
    git(&origin, &["commit", "-qm", "second"]);
    git(&origin, &["config", "uploadpack.allowFilter", "true"]);

    // blob を持たない部分クローン。足りない blob を取りにいくと uploadpack（＝目印）が走る。
    let clone = fx.dir.join("clone");
    let url = format!("file://{}", origin.display());
    git(&fx.dir, &["clone", "-q", "--filter=blob:none", "--no-checkout", &url, clone.to_str().unwrap()]);
    git(&clone, &["config", "remote.origin.uploadpack", fx.script("lazy-fetch").to_str().unwrap()]);
    git(&clone, &["read-tree", "HEAD"]);
    std::fs::write(clone.join("a.md"), "# a\nlocal\n").unwrap();

    let diff = hit_all(&clone, FAST);
    assert_eq!(fx.marks(), Vec::<String>::new(), "足りない blob を取りにいった");
    assert!(diff.contains("差分を出せません"), "未追跡として全行追加にした: {diff}");
}

#[test]
fn hanging_repository_returns_in_time_and_leaves_no_children() {
    let fx = Fixture::new("hang");
    let r = fx.repo("hang");
    // 置き場所の決まっていない `.gitignore`（サブディレクトリ）を FIFO にする。
    // 未追跡の一覧（ls-files --others）がこれを open して止まる。
    std::fs::create_dir_all(r.join("deep")).unwrap();
    std::fs::write(r.join("deep/x.md"), "x\n").unwrap();
    assert!(Command::new("mkfifo").arg(r.join("deep/.gitignore")).status().unwrap().success());

    let start = Instant::now();
    changed_files(&r);
    assert!(start.elapsed() < Duration::from_secs(4), "時間の上限で止まらなかった: {:?}", start.elapsed());

    // 時間切れの後はしばらく呼ばないので、続けて叩いてもすぐ返る。
    let start = Instant::now();
    hit_all(&r, Duration::from_millis(500));
    assert!(start.elapsed() < Duration::from_millis(500));

    // 止めた git が残っていないこと。
    std::thread::sleep(Duration::from_millis(200));
    let left = Command::new("pgrep").args(["-f", r.to_str().unwrap()]).output().unwrap();
    assert!(left.stdout.is_empty(), "子プロセスが残った: {}", String::from_utf8_lossy(&left.stdout));
}

#[test]
fn special_files_at_known_places_refuse_git_immediately() {
    let fx = Fixture::new("special");
    for (i, rel) in [".gitattributes", ".git/config", ".git/HEAD", ".git/objects/info/alternates"].iter().enumerate() {
        let r = fx.repo(&format!("fifo-{i}"));
        let target = r.join(rel);
        let _ = std::fs::remove_file(&target);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        assert!(Command::new("mkfifo").arg(&target).status().unwrap().success());
        hit_all(&r, Duration::from_secs(1));
    }
}

#[test]
fn ordinary_repository_still_counts_changes() {
    let fx = Fixture::new("ordinary");
    let r = fx.repo("plain");
    assert_eq!(diff_stat(&r.join("a.md")), (1, 0));
    assert_eq!(diff_stat(&r.join("new.md")), (1, 0));
    let mut names: Vec<String> = changed_files(&r).into_iter().map(|(p, _, _)| p).collect();
    names.sort();
    let expect = |n: &str| r.join(n).to_string_lossy().into_owned();
    assert_eq!(names, vec![expect("a.md"), expect("new.md")]);
    // 触っただけ（中身は同じ）のファイルは一覧に出ない（diff-index の --numstat は出さない）。
    std::fs::write(r.join("a.md"), "# a\n").unwrap();
    let names: Vec<String> = changed_files(&r).into_iter().map(|(p, _, _)| p).collect();
    assert_eq!(names, vec![expect("new.md")]);
    let diff = render_diff_inner(&r.join("new.md"));
    assert!(!diff.contains("差分を出せません"), "{diff}");
}
