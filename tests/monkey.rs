//! モンキーテスト: ユーザー操作（ツリー展開・ファイルオープン・
//! raw/diff 切替）を、シード固定の乱数でランダムな操作列として `handle_request`
//! 相当に叩き込み、(1) パニックが起きないこと、(2) どの操作も一定時間内に返る
//! （＝固まらない）ことを確認する。
//!
//! `md /` で「ツリーをいじってたら固まった」現象を炙り出すのが主目的。犯人候補は
//! `has_md_descendant` の深さ無制限な全走査で、サイドバーに見えるサブフォルダの数
//! だけ同時に走ると重くなる。
//!
//! 通常の `cargo test` では走らせない（`#[ignore]`）。明示的に:
//!   cargo test --test monkey -- --ignored --nocapture
//!
//! 環境変数で挙動を調整する:
//!   MONKEY_SEED=<u64>     乱数シード（省略時は起動時刻から生成し、必ず表示する）
//!   MONKEY_ITERS=<n>      操作回数（既定 3000）
//!   MONKEY_FREEZE_MS=<ms> この時間を超えた操作を「固まり」と判定（既定 2000）
//!   MONKEY_ROOT=<path>    生成フィクスチャの代わりに実ディレクトリを歩く。
//!                         例: MONKEY_ROOT=/ で本物の `md /` 相当を再現（要注意・遅い）
//!   MONKEY_WIDTH=<n>      フィクスチャの横幅（トップ階層のフォルダ数、既定 12）
//!   MONKEY_DEPTH=<n>      フィクスチャの縦の深さ（ネスト段数、既定 150）

use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use md_preview::request::{handle_request, has_md_descendant, safe_join, RequestContext};

/// 決定論のための小さな PRNG（splitmix64）。乱数クレートを足さずに、シードから
/// 完全に再現可能な操作列を作るために自前で持つ。
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// 0..n の一様乱数（n==0 なら 0）。
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next_u64() % n as u64) as usize }
    }
    /// スライスからランダムに 1 個借りる。空なら None。
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> Option<&'a T> {
        if xs.is_empty() { None } else { Some(&xs[self.below(xs.len())]) }
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// 起動時刻ベースの初期シード（MONKEY_SEED 未指定時のフォールバック）。
fn time_seed() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x1234_5678)
}

/// 1 操作の実行結果。
enum Outcome {
    Ok(Duration),
    Panic,
    Freeze,
}

/// クロージャを別スレッドで走らせ、パニックを捕捉しつつタイムアウトを監視する。
/// タイムアウトしたスレッドは（Rust ではスレッドを殺せないので）そのままリーク
/// させる。固まりを「発見する」のが目的なので、発見後はテストを終わらせて OK。
fn run_guarded<F: FnOnce() + Send + 'static>(f: F, freeze: Duration) -> Outcome {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let start = Instant::now();
        let ok = std::panic::catch_unwind(AssertUnwindSafe(f)).is_ok();
        let _ = tx.send((ok, start.elapsed()));
    });
    match rx.recv_timeout(freeze) {
        Ok((true, dt)) => Outcome::Ok(dt),
        Ok((false, _)) => Outcome::Panic,
        Err(_) => Outcome::Freeze,
    }
}

/// 発生しうる操作。実際のフロントエンド（folder.js）が投げる
/// リクエストに 1 対 1 で対応させている。`rel` は root からの相対パス。
#[derive(Clone, Debug)]
enum Action {
    /// サイドバーでフォルダを展開: ディレクトリ一覧の取得。
    ListDir(String),
    /// フォルダ展開時に各サブフォルダへ飛ぶ md 有無判定（重い容疑者）。
    HasMd(String),
    /// ファイルをプレビュー表示。
    OpenFile(String),
    /// raw（ソース）表示に切替。
    Raw(String),
    /// diff 表示に切替。
    Diff(String),
    /// アセット直開き（GET /rel）。
    Asset(String),
    /// ハンドラを直接殴る不正・境界クエリ。
    Garbage(String),
}

/// root を空文字（ルート自身）と与えられた相対パスから、各操作を実行する。
/// `RequestContext` の付随フィールドは空で構わない（本文 HTML 生成やテーマは
/// 固まり/パニックの判定に関係しないため）。
fn perform(action: &Action, root: &Path) {
    let ctx = RequestContext {
        root_dir: root.to_path_buf(),
        index_html: Vec::new(),
        theme_css: String::new(),
        custom_css: String::new(),
    };
    match action {
        Action::ListDir(rel) => drop(handle_request(&ctx, "/", &format!("dir={}", rel))),
        Action::HasMd(rel) => {
            // has_md= 経路と同じ。safe_join を通してから全走査する。
            if let Some(p) = safe_join(&ctx.root_dir, rel) {
                let _ = has_md_descendant(&p);
            }
        }
        Action::OpenFile(rel) => drop(handle_request(&ctx, "/", &format!("file={}", rel))),
        Action::Raw(rel) => drop(handle_request(&ctx, "/", &format!("raw={}", rel))),
        Action::Diff(rel) => drop(handle_request(&ctx, "/", &format!("diff={}", rel))),
        Action::Asset(rel) => drop(handle_request(&ctx, &format!("/{}", rel), "")),
        Action::Garbage(q) => drop(handle_request(&ctx, "/", q)),
    }
}

/// 実ツリーを浅く探索して、既知ディレクトリ / ファイルの相対パスプールを更新する。
/// 実際のサイドバー操作（フォルダを開くと子が見える）を模して、乱数で選んだ既知
/// ディレクトリの直下だけを覗く。
fn discover(rel: &str, root: &Path, dirs: &mut Vec<String>, files: &mut Vec<String>) {
    let abs = if rel.is_empty() { root.to_path_buf() } else { root.join(rel) };
    let Ok(entries) = std::fs::read_dir(&abs) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let child_rel = if rel.is_empty() { name } else { format!("{}/{}", rel, name) };
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if !dirs.contains(&child_rel) { dirs.push(child_rel); }
        } else if !files.contains(&child_rel) {
            files.push(child_rel);
        }
        // プールが太りすぎないよう上限を設ける（メモリと選択の偏り対策）。
        if dirs.len() + files.len() > 5000 { break; }
    }
}

/// 不正・境界クエリの種。percent_decode / safe_join / json_string を意地悪な
/// 入力で殴る。
const GARBAGE: &[&str] = &[
    "dir=../../../../etc",
    "file=../../../../etc/passwd",
    "raw=%2e%2e%2f%2e%2e%2fetc",
    "file=%ff%fe%00",
    "dir=%",
    "file=%zz",
    "raw=",
    "diff=",
    "has_md=..",
    "file=nul\u{0000}byte",
    "dir=日本語/../のフォルダ",
    "file=a b c/スペース入り.md",
    "raw=very/deep/../../..//./x",
    "file=..%2f..%2fsecret",
];

#[test]
#[ignore = "手動起動のストレステスト。`--ignored --nocapture` で走らせる"]
fn monkey_folder_navigation() {
    let seed = std::env::var("MONKEY_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or_else(time_seed);
    let iters = env_u64("MONKEY_ITERS", 3000) as usize;
    let freeze = Duration::from_millis(env_u64("MONKEY_FREEZE_MS", 2000));
    let mut rng = Rng(seed);

    eprintln!("=== monkey: seed={} iters={} freeze={}ms ===", seed, iters, freeze.as_millis());
    eprintln!("再現するには MONKEY_SEED={} を付けて同じ引数で再実行するのだ", seed);

    // ルートの決定: MONKEY_ROOT があれば実ツリー、無ければ病的フィクスチャを生成。
    let (root, _fixture) = match std::env::var("MONKEY_ROOT") {
        Ok(p) => {
            let root = PathBuf::from(&p).canonicalize().expect("MONKEY_ROOT を解決できない");
            eprintln!("実ツリーを歩く: {}", root.display());
            (root, None)
        }
        Err(_) => {
            let fx = Fixture::build(seed);
            eprintln!("フィクスチャ生成: {}", fx.root.display());
            (fx.root.clone(), Some(fx))
        }
    };

    let mut dirs: Vec<String> = vec![String::new()];
    let mut files: Vec<String> = Vec::new();
    // 最初にルート直下だけは見えている状態にする。
    discover("", &root, &mut dirs, &mut files);

    let mut slowest = Duration::ZERO;
    let mut slowest_action: Option<Action> = None;

    for i in 0..iters {
        // 操作をランダムに選ぶ。フォルダ展開系（ListDir/HasMd）を厚めにして、
        // 「ツリーをいじる」挙動に寄せる。
        let roll = rng.below(100);
        let action = if roll < 30 {
            let rel = rng.pick(&dirs).cloned().unwrap_or_default();
            Action::ListDir(rel)
        } else if roll < 55 {
            let rel = rng.pick(&dirs).cloned().unwrap_or_default();
            Action::HasMd(rel)
        } else if roll < 75 {
            match rng.pick(&files) { Some(f) => Action::OpenFile(f.clone()), None => Action::ListDir(String::new()) }
        } else if roll < 83 {
            match rng.pick(&files) { Some(f) => Action::Raw(f.clone()), None => Action::ListDir(String::new()) }
        } else if roll < 90 {
            match rng.pick(&files) { Some(f) => Action::Diff(f.clone()), None => Action::ListDir(String::new()) }
        } else if roll < 95 {
            match rng.pick(&files) { Some(f) => Action::Asset(f.clone()), None => Action::ListDir(String::new()) }
        } else {
            Action::Garbage(rng.pick(GARBAGE).unwrap().to_string())
        };

        // ListDir は「展開」なので、実行のついでに子を発見してプールを広げる
        // （実際のサイドバー操作と同じ）。
        if let Action::ListDir(rel) = &action {
            discover(rel, &root, &mut dirs, &mut files);
        }

        let act_for_thread = action.clone();
        let root_for_thread = root.clone();
        let outcome = run_guarded(move || perform(&act_for_thread, &root_for_thread), freeze);

        match outcome {
            Outcome::Ok(dt) => {
                if dt > slowest {
                    slowest = dt;
                    slowest_action = Some(action.clone());
                }
            }
            Outcome::Panic => {
                panic!(
                    "パニック発生！ 再現: MONKEY_SEED={} step={} action={:?}",
                    seed, i, action
                );
            }
            Outcome::Freeze => {
                panic!(
                    "固まり検出（>{}ms）！ 再現: MONKEY_SEED={} step={} action={:?}\n\
                     root={}",
                    freeze.as_millis(), seed, i, action, root.display()
                );
            }
        }

        if i % 500 == 499 {
            eprintln!("  {} 操作完了 / 既知dir={} file={} / 最遅={}ms",
                i + 1, dirs.len(), files.len(), slowest.as_millis());
        }
    }

    eprintln!(
        "=== 完走: {} 操作, パニック/固まり無し。最遅操作={}ms {:?} ===",
        iters, slowest.as_millis(), slowest_action
    );
}

/// 病的なディレクトリツリー。Drop で自動削除する。
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn build(seed: u64) -> Fixture {
        let width = env_u64("MONKEY_WIDTH", 12) as usize;
        let depth = env_u64("MONKEY_DEPTH", 150) as usize;
        let root = std::env::temp_dir().join(format!("md-monkey-{}", seed));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("フィクスチャ root を作れない");

        // (1) 横に広く、md を 1 つも置かない枝。has_md_descendant を最後まで
        //     歩かせる worst case。
        for w in 0..width {
            let d = root.join(format!("wide{:02}", w));
            std::fs::create_dir_all(&d).unwrap();
            for f in 0..30 {
                write_file(&d.join(format!("note{:02}.txt", f)), b"no markdown here\n");
                write_file(&d.join(format!("code{:02}.rs", f)), b"fn main() {}\n");
            }
        }

        // (2) 縦に深いネスト。深さ無制限の再帰を刺激する。最深部にだけ md を置く。
        let mut deep = root.join("deep");
        std::fs::create_dir_all(&deep).unwrap();
        for level in 0..depth {
            deep = deep.join("d");
            if std::fs::create_dir(&deep).is_err() {
                // PATH_MAX に当たったら打ち切る（macOS は 1024 前後）。
                break;
            }
            if level % 40 == 0 {
                write_file(&deep.join("mid.txt"), b"x\n");
            }
        }
        write_file(&deep.join("bottom.md"), b"# deep\n");

        // (3) 変な名前たち。percent_decode / safe_join / json_string を殴る。
        let weird = root.join("weird");
        std::fs::create_dir_all(&weird).unwrap();
        for name in [
            "スペース 入り.md",
            "日本語ファイル.md",
            "dots..name.md",
            "%20encoded.md",
            "quote\"and'apos.md",
            "back\\slash.txt",
            "tab\tname.txt",
            "emoji😀.md",
        ] {
            // 一部の名前は OS が拒否しうるので、失敗は無視する。
            let _ = std::fs::write(weird.join(name), b"# weird\n");
        }

        // (4) 巨大ファイル。HIGHLIGHT_MAX_BYTES 超過でハイライト無効経路を通す。
        let big = root.join("huge.log");
        let line = b"2026-07-10 INFO this is a log line that repeats many times\n";
        let mut buf = Vec::with_capacity(2_200_000);
        while buf.len() < 2_000_000 { buf.extend_from_slice(line); }
        write_file(&big, &buf);

        // (5) 意地悪な Markdown 本文（frontmatter 境界・未閉じ・巨大表）。
        let md = root.join("nasty.md");
        let mut s = String::from("---\ntitle: no close\nkey without colon\n");
        s.push_str("| a | b |\n|---|---|\n");
        for r in 0..2000 { s.push_str(&format!("| r{r} | <script>x</script> |\n")); }
        s.push_str("\n> [!NOTE]\n> unterminated ```rust\nfn f(){\n");
        write_file(&md, s.as_bytes());

        // (6) symlink ループ（has_md_descendant は辿らない想定だが、他経路の保険）。
        #[cfg(unix)]
        {
            let loop_dir = root.join("loopdir");
            std::fs::create_dir_all(&loop_dir).unwrap();
            let _ = std::os::unix::fs::symlink(&loop_dir, loop_dir.join("self"));
        }

        Fixture { root }
    }
}

fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, bytes);
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // テスト成功時のみ掃除される（パニック時はスレッド巻き戻しで Drop が走る
        // が、原因調査用に残したい場合は MONKEY_KEEP=1 で残す）。
        if std::env::var("MONKEY_KEEP").is_ok() {
            eprintln!("フィクスチャを残す: {}", self.root.display());
            return;
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
