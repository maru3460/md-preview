//! モンキーテスト: ユーザー操作（ツリー展開・ファイルオープン・
//! raw/diff 切替）を、シード固定の乱数でランダムな操作列として `handle_request`
//! 相当に叩き込み、(1) パニックが起きないこと、(2) どの操作も一定時間内に返る
//! （＝固まらない）ことを確認する。
//!
//! `md /` で「ツリーをいじってたら固まった」現象を炙り出すのが主目的。犯人だった
//! ドット判定の全走査は予算付きになった（`request::md_presence`）ので、いまここが
//! 守っているのは「予算が実際に効いていること」である。最遅の操作が `HasMd` に
//! 戻ったら、予算が素通りしているか、どこかで予算の外を歩いている。
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

use md_preview::request::{handle_request, RequestContext};

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
    /// 所要時間と HTTP ステータス。ステータスを持ち帰るのは、**このテストが的に
    /// 当たっているかを測る唯一の手段**だから。アサーションが無い（固まらないことだけを
    /// 見る）ので、全部 404 で即返っていても走り切ってしまう。
    Ok(Duration, u16),
    Panic,
    Freeze,
}

/// クロージャを別スレッドで走らせ、パニックを捕捉しつつタイムアウトを監視する。
/// タイムアウトしたスレッドは（Rust ではスレッドを殺せないので）そのままリーク
/// させる。固まりを「発見する」のが目的なので、発見後はテストを終わらせて OK。
fn run_guarded<F: FnOnce() -> u16 + Send + 'static>(f: F, freeze: Duration) -> Outcome {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let start = Instant::now();
        let status = std::panic::catch_unwind(AssertUnwindSafe(f)).ok();
        let _ = tx.send((status, start.elapsed()));
    });
    match rx.recv_timeout(freeze) {
        Ok((Some(status), dt)) => Outcome::Ok(dt, status),
        Ok((None, _)) => Outcome::Panic,
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
    /// ハンドラを直接殴る不正・境界クエリ。`bool` は 404 を期待するか
    /// （識別子として不正な形＝`GARBAGE_REJECTED` なら true）。
    Garbage(String, bool),
}

/// プールが持つ root 相対パスを、ページが投げるのと同じ識別子（絶対パス）にする。
///
/// **ここを通さないと、このテストは丸ごと空振りする。** `request::id_to_path` は
/// 絶対パスでない識別子を 1 バイト目で弾くので、root 相対のまま載せると全アクションが
/// 404 で即返り、深いネストも巨大ファイルも一度も踏まない（アサーションが無く
/// 「固まらないこと」だけを見るテストなので、空振りしていても緑のままになる）。
fn id_of(root: &Path, rel: &str) -> String {
    if rel.is_empty() { root.to_string_lossy().into_owned() } else { root.join(rel).to_string_lossy().into_owned() }
}

/// root と与えられた相対パスから、各操作を実行する。
/// `RequestContext` の付随フィールドは空で構わない（本文 HTML 生成やテーマは
/// 固まり/パニックの判定に関係しないため）。
///
/// クエリに載せる形は 2 通りある。`dir` / `has_md` / `file` / `raw` / `diff` は
/// **識別子**（絶対パス）、`Asset` は **URL**（root 相対）。名前空間が違うので
/// 混ぜないこと——混ぜると片方の関門（`id_to_path` / `safe_join`）に一度も届かない。
fn perform(action: &Action, root: &Path) -> u16 {
    let ctx = RequestContext {
        root_dir: root.to_path_buf(),
        index_html: Vec::new(),
        theme_css: String::new(),
        custom_css: String::new(),
    };
    match action {
        Action::ListDir(rel) => handle_request(&ctx, "/", &format!("dir={}", id_of(root, rel))),
        Action::HasMd(rel) => handle_request(&ctx, "/", &format!("has_md={}", id_of(root, rel))),
        Action::OpenFile(rel) => handle_request(&ctx, "/", &format!("file={}", id_of(root, rel))),
        Action::Raw(rel) => handle_request(&ctx, "/", &format!("raw={}", id_of(root, rel))),
        Action::Diff(rel) => handle_request(&ctx, "/", &format!("diff={}", id_of(root, rel))),
        Action::Asset(rel) => handle_request(&ctx, &format!("/{}", rel), ""),
        Action::Garbage(q, _) => handle_request(&ctx, "/", q),
    }
    .status()
    .as_u16()
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

/// 識別子として不正な種。percent_decode を殴りつつ、`id_to_path` の
/// 「絶対パスでなければ弾く」に必ず引っかかる。
///
/// **404 が返ることをアサートする。** ここを通ってしまったら関門が緩んだということ
/// で、それはタブの二重化（#33）が戻ってくる入口になる。コメントで期待を書くだけに
/// すると、通るようになっても緑のまま気づけない。
const GARBAGE_REJECTED: &[&str] = &[
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

/// 識別子として形は正しい（絶対パス）が、意地の悪い種。`id_to_path` の関門を
/// 越えて canonicalize と root の内外判定まで届く。ステータスは問わない
/// （200 も 404 もありうる）。固まらないこと・パニックしないことだけを見る。
/// `{ROOT}` は実行時に root の絶対パスへ差し替える。
const GARBAGE_ABSOLUTE: &[&str] = &[
    "dir=/etc",
    "dir=/",
    "file=/etc/passwd",
    "file={ROOT}/../outside/x.md",
    "dir={ROOT}/../..",
    "file={ROOT}/%00",
    "raw={ROOT}/日本語/../のファイル.md",
    "has_md={ROOT}",
    "file={ROOT}",
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
    // 識別子を載せる操作の命中を**種別ごと**に数える。全体で 1 本にすると、
    // 比率の小さい `?file=` / `?raw=` / `?diff=` が全滅しても合計は半分を割らず、
    // いちばんありそうな事故を見逃す。添字は KIND_NAMES と対応。
    let mut aimed = [0usize; 5];
    let mut hit = [0usize; 5];

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
            // 半々で「弾かれるべき種」と「関門の向こうへ届く種」を投げる。
            let rejected = rng.below(2) == 0;
            let pool = if rejected { GARBAGE_REJECTED } else { GARBAGE_ABSOLUTE };
            let seed = rng.pick(pool).unwrap();
            Action::Garbage(seed.replace("{ROOT}", &root.to_string_lossy()), rejected)
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
            Outcome::Ok(dt, status) => {
                if let Some(k) = kind_index(&action) {
                    aimed[k] += 1;
                    if status == 200 {
                        hit[k] += 1;
                    }
                }
                // 識別子として不正な種は必ず弾かれること。ここが 404 でなくなったら、
                // root 相対がまた通るようになったということ（タブ二重化の入口）。
                if let Action::Garbage(q, true) = &action {
                    assert_eq!(
                        status, 404,
                        "識別子として不正な種が通った: {:?} → {}\n再現: MONKEY_SEED={}",
                        q, status, seed
                    );
                }
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
            eprintln!("  {} 操作完了 / 既知dir={} file={} / 最遅={}ms / 命中={}",
                i + 1, dirs.len(), files.len(), slowest.as_millis(), hit_summary(&hit, &aimed));
        }
    }

    eprintln!(
        "=== 完走: {} 操作, パニック/固まり無し。最遅操作={}ms {:?} / 命中={} ===",
        iters, slowest.as_millis(), slowest_action, hit_summary(&hit, &aimed)
    );

    // 命中率の下限。守っているのは「このテストが的に当たっていること」そのもの。
    // 識別子の形を変えたときに全アクションが 404 で即返るようになっても、パニックも
    // 固まりも起きないので、ここが無いと緑のまま気づけない（#33 で実際にそうなった）。
    //
    // プールは `discover` が使う直前に実ツリーを舐め直して作るので、ほぼ全部が
    // 200 になるのが正常（実測 99.9%）。9 割で切ってあるのは、走っている間に
    // 外からファイルが消える余地だけを残すため。
    for (k, name) in KIND_NAMES.iter().enumerate() {
        assert!(
            aimed[k] > 0,
            "{} を一度も投げていない。アクションの抽選かプールが壊れている",
            name
        );
        assert!(
            hit[k] * 10 >= aimed[k] * 9,
            "{} の命中が {}/{} しかない。識別子の形がサーバと食い違っていないか\n\
             （root={} / 200 以外はほぼ 404 のはず）",
            name, hit[k], aimed[k], root.display()
        );
    }
}

/// 命中を数える対象の種別。`Asset` は URL 名前空間、`Garbage` は落ちるのが正常
/// なので、どちらもここには入れない。
const KIND_NAMES: [&str; 5] = ["?dir=", "?has_md=", "?file=", "?raw=", "?diff="];

fn kind_index(action: &Action) -> Option<usize> {
    match action {
        Action::ListDir(_) => Some(0),
        Action::HasMd(_) => Some(1),
        Action::OpenFile(_) => Some(2),
        Action::Raw(_) => Some(3),
        Action::Diff(_) => Some(4),
        Action::Asset(_) | Action::Garbage(..) => None,
    }
}

fn hit_summary(hit: &[usize; 5], aimed: &[usize; 5]) -> String {
    KIND_NAMES
        .iter()
        .enumerate()
        .map(|(k, name)| format!("{}{}/{}", name, hit[k], aimed[k]))
        .collect::<Vec<_>>()
        .join(" ")
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
        // 実アプリの root は必ず canonicalize 済み（`app_config::resolve_arg_path`）。
        // 揃えないと、macOS の temp_dir が `/var` → `/private/var` のリンクなので
        // `resolve_tree_dir` の root 内判定が全部外れ、ツリー系が 404 で空振りする。
        let root = root.canonicalize().expect("フィクスチャ root を解決できない");

        // (1) 横に広く、md を 1 つも置かない枝。サブディレクトリが無いので予算には
        //     届かないが、早期 return が効かない（＝最後まで舐める）形の再現。
        for w in 0..width {
            let d = root.join(format!("wide{:02}", w));
            std::fs::create_dir_all(&d).unwrap();
            for f in 0..30 {
                write_file(&d.join(format!("note{:02}.txt", f)), b"no markdown here\n");
                write_file(&d.join(format!("code{:02}.rs", f)), b"fn main() {}\n");
            }
        }

        // (2) 縦に深いネスト。最深部にだけ md を置くので、ドット判定は深さ予算で
        //     刈られて Unknown を返す（それが期待挙動）。
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

        // (6) symlink ループ。走査系は file_type() でリンクを追わないので辿らない
        //     （request.rs の単体テストで担保済み）。ここは他経路の保険。
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
