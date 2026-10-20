//! 通常ファイル以外と巨大なファイルを指されたときの、配信の各経路。
//!
//! 未追跡のシンボリックリンクの先が `/dev/zero` や FIFO でも、本文・ソース・差分・
//! バッジ・⌘P の一覧・`/__abs/` の配信が、すぐ返ってくること。1GB を超えるファイルを
//! 指しても、全量をメモリに載せないこと。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use md_preview::request::{handle_request, RequestContext};

/// 一時のリポジトリ。抜けるときに消す（疎な 1GB のファイルを残さないように）。
struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(tag: &str) -> Fixture {
    let d = std::env::temp_dir().join(format!("md-special-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let d = d.canonicalize().unwrap();
    let git = |args: &[&str]| {
        let ok = Command::new("git").arg("-C").arg(&d).args(args).output().unwrap().status.success();
        assert!(ok, "git {:?}", args);
    };
    git(&["init", "-q"]);
    std::fs::write(d.join("a.md"), "# a\n").unwrap();
    git(&["add", "a.md"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-qm", "init"]);
    Fixture(d)
}

/// 1 回の要求が 5 秒以内に返ること（git の時間の上限 2 秒を 2 回重ねても収まる幅）。
/// 止まる実装でも検査が返ってくるよう、別のスレッドで呼んで待つ時間を区切る。
fn quick(ctx: &Arc<RequestContext>, path: &str, query: &str) -> (u16, String) {
    let (tx, rx) = std::sync::mpsc::channel();
    let (ctx, p, q) = (ctx.clone(), path.to_string(), query.to_string());
    std::thread::spawn(move || {
        let resp = handle_request(&ctx, &p, &q);
        let _ = tx.send((resp.status().as_u16(), String::from_utf8_lossy(resp.body()).into_owned()));
    });
    rx.recv_timeout(Duration::from_secs(5))
        .unwrap_or_else(|_| panic!("{path}?{query} が返ってこない"))
}

fn ctx(root: &Path) -> Arc<RequestContext> {
    Arc::new(RequestContext::new(root.to_path_buf(), Vec::new(), String::new(), String::new()))
}

fn id(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[test]
fn devices_and_fifos_behind_untracked_symlinks_return_immediately() {
    let fx = fixture("dev");
    let root = &fx.0;
    let fifo = root.join("pipe");
    assert!(Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    std::os::unix::fs::symlink("/dev/zero", root.join("zero.md")).unwrap();
    std::os::unix::fs::symlink(&fifo, root.join("fifo.md")).unwrap();
    std::os::unix::fs::symlink("/dev/zero", root.join("zero.png")).unwrap();
    std::os::unix::fs::symlink("/dev/zero", root.join("zero.html")).unwrap();
    let ctx = ctx(root);

    for name in ["zero.md", "fifo.md"] {
        let target = id(&root.join(name));
        quick(&ctx, "/", &format!("file={target}"));
        quick(&ctx, "/", &format!("raw={target}"));
        quick(&ctx, "/", &format!("diff={target}"));
        let (_, stat) = quick(&ctx, "/", &format!("diffstat={target}"));
        assert_eq!(stat, r#"{"add":0,"del":0}"#, "{name}");
        let (status, _) = quick(&ctx, &format!("/{name}"), "");
        assert_eq!(status, 404, "{name} を配った");
    }
    quick(&ctx, "/", "changed=1");
    for path in ["/zero.png", "/zero.html", "/__abs/dev/zero"] {
        let (status, _) = quick(&ctx, path, "");
        assert_eq!(status, 404, "{path} を配った");
    }
}

#[test]
fn files_over_the_limit_are_refused_without_reading_them() {
    let fx = fixture("big");
    let root = &fx.0;
    // 疎なファイルなので、実際にはディスクを使わない。
    for name in ["big.png", "big.md", "big.html"] {
        std::fs::File::create(root.join(name)).unwrap().set_len(1024 * 1024 * 1024 + 1).unwrap();
    }
    let ctx = ctx(root);

    for path in ["/big.png", "/big.html"] {
        let (status, _) = quick(&ctx, path, "");
        assert_eq!(status, 404, "{path} を配った");
    }
    let (status, body) = quick(&ctx, "/", &format!("file={}", id(&root.join("big.md"))));
    assert_eq!(status, 200);
    assert!(body.contains("大きすぎる"), "専用の文言が出ていない: {body}");
    let (_, body) = quick(&ctx, "/", &format!("diff={}", id(&root.join("big.md"))));
    assert!(body.contains("大きすぎる"), "差分に専用の文言が出ていない: {body}");
}

/// 素の読み込み（`std::fs::read` など）を、テストのモジュールを除く `src/` から探し、
/// 例外の印の無いものを数える。
///
/// - 読み込みの例外の印は、すぐ上の行の `// fsread を通さない: <理由>`。いまは設定ディレクトリなど、
///   アプリ自身の持ち物を読む箇所にだけ付いている
/// - テストのモジュールは `#[cfg(..test..)]` の次の行が `mod ` で始まるところから先。ファイルの途中の
///   `#[cfg(test)]` 付きの関数（`theme.rs` など）では止めない
/// - `fs::{` は、`use std::fs::{read, …}` の後の素の `read(` を見落とさないため、import ごと拾う
#[test]
fn raw_reads_stay_in_fsread() {
    const MARK: &str = "// fsread を通さない: ";
    const RAW_READS: &[&str] = &[
        "fs::read(", "fs::read_to_string(", "File::open(", "File::options(", "OpenOptions::new(",
        "fs::copy(", "io::copy(", "fs::{",
    ];
    let mut offenders = Vec::new();
    let mut stack = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let lines: Vec<&str> = text.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let t = line.trim_start();
                if t.starts_with("#[cfg(") && t.contains("test")
                    && lines.get(i + 1).is_some_and(|n| n.trim_start().starts_with("mod "))
                {
                    break;
                }
                let marked = i > 0 && lines[i - 1].trim_start().starts_with(MARK);
                if !marked && RAW_READS.iter().any(|p| line.contains(p)) {
                    offenders.push(format!("{}:{}: {}", path.display(), i + 1, t));
                }
            }
        }
    }
    assert!(offenders.is_empty(), "印の無い素の読み込み:\n{}", offenders.join("\n"));
}
