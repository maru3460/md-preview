//! git を起動する唯一の入口。
//!
//! 開いたフォルダは他人の（あるいは AI が書き換えた）リポジトリでありうる。git はそこの
//! `.git/config` と `.gitattributes` を読んで、fsmonitor・filter・フック・遅延取得で
//! 任意のコマンドを起動できる。ここでは次のように塞ぐ。
//! - 設定は `GIT_CONFIG_COUNT` / `KEY_n` / `VALUE_n` で渡す（どの設定ファイルよりも強く、
//!   これより強いのは `-c` だけで、ここでは `-c` を渡さない）。`-c` にしないのは、filter の
//!   名前に `=` が入ると `-c` ではキーと値の切れ目を取り違えるため
//! - 引き継いだ `GIT_*` は消す（`GIT_DIR` や `GIT_CONFIG_COUNT` で外から差し込まれないように）。
//!   利用者が自分の設定の置き場所を指す `GIT_CONFIG_GLOBAL` / `GIT_CONFIG_SYSTEM`（絶対パスの
//!   とき）/ `GIT_CONFIG_NOSYSTEM` だけは残す。消すと、利用者の global 設定（`core.excludesFile`
//!   など）を読めず、リポジトリ側の値を戻す先を取り違える
//! - リポジトリ側の設定（filter・excludesFile・部分クローン）は `git config` で 1 回読んでから
//!   本体を起動する。読んだ結果は設定ファイルの更新時刻を鍵に覚え、変わっていなければ
//!   `git config` を省く（差分のバッジと ⌘P の並べ替えが、呼ぶたびに git を 2 本起こさないように）
//! - すべての呼び出しに時間と出力量の上限を掛け、超えたらプロセスグループごと止める。
//!   止まらなくなる設定（FIFO の `.gitignore` など）は、上限でしか確実に止められない

use std::collections::HashMap;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 1 回の呼び出しの上限。⌘P の並べ替えと差分のバッジのためだけに呼ぶので、待たせるより諦める。
const TIMEOUT: Duration = Duration::from_secs(2);

/// 時間切れになったリポジトリに、git を呼ばずにおく時間。同じリポジトリで毎回 2 秒待たされない
/// ように。
const COOLDOWN: Duration = Duration::from_secs(30);

/// 出力の上限。`git show HEAD:<file>` がいちばん大きく、本文として描ける上限と揃える。
const OUTPUT_MAX: u64 = crate::fsread::TEXT_MAX_BYTES;

/// `GIT_NO_LAZY_FETCH` が入った版。これより古い git は、部分クローンのリポジトリで
/// 足りない blob を取りにいってしまう（`GIT_ALLOW_PROTOCOL=none` だけでは取得の試み自体は
/// 起きる）。
const LAZY_FETCH_GUARD: (u32, u32) = (2, 44);

#[derive(Debug)]
pub enum GitError {
    /// `dir` から上にリポジトリが無い。
    NotRepo,
    /// 呼ぶと危ない、または呼べない状態（特殊なファイル・古い git の部分クローン・時間切れ直後）。
    Refused,
    /// 時間の上限を超えた。そのリポジトリはしばらく呼ばない。
    TimedOut,
    /// 出力量の上限を超えた。HEAD 側の巨大な blob など、何度呼んでも同じ結果になるもの
    /// なので、時間切れと違ってリポジトリごと止めはしない。
    TooLarge,
    /// git が見つからない・起動できない。
    Spawn,
}

pub struct GitOutput {
    pub success: bool,
    pub stdout: Vec<u8>,
}

impl GitOutput {
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
}

/// `git -C <dir> <args>` を、上の守りを全部掛けて走らせる。終了コードが 0 でなくても
/// `Ok`（`success: false`）で返す。0 以外を「無い」と読むかどうかは呼び出し側が決める。
pub fn run(dir: &Path, args: &[&str]) -> Result<GitOutput, GitError> {
    let repo = find_repo(dir).ok_or(GitError::NotRepo)?;
    if in_cooldown(&repo) {
        return Err(GitError::Refused);
    }
    let lock = repo_lock(&repo);
    let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
    // 鍵を待っている間に、前の呼び出しが時間切れになっていることがある。待っていた全員が
    // それぞれ 2 秒ずつ待たされないよう、鍵を取った後にもう一度見る。
    if in_cooldown(&repo) {
        return Err(GitError::Refused);
    }

    if !special_files_are_regular(&repo, dir) {
        return Err(GitError::Refused);
    }
    let git = git_path().ok_or(GitError::Spawn)?;

    let repo_cfg = repo_config(git, dir, &repo).inspect_err(|e| note_timeout(&repo, e))?;
    if repo_cfg.partial_clone && version(git).is_none_or(|v| v < LAZY_FETCH_GUARD) {
        return Err(GitError::Refused);
    }

    let mut cmd = base_command(git, dir);
    let overrides = overrides(&repo_cfg);
    cmd.env("GIT_CONFIG_COUNT", overrides.len().to_string());
    for (i, (k, v)) in overrides.iter().enumerate() {
        cmd.env(format!("GIT_CONFIG_KEY_{i}"), k);
        cmd.env(format!("GIT_CONFIG_VALUE_{i}"), v);
    }
    cmd.args(args);
    spawn_limited(cmd).inspect_err(|e| note_timeout(&repo, e))
}

/// `dir` から上へ `.git`（ディレクトリか、ワークツリー・サブモジュールの `gitdir:` ファイル）
/// を探す。git を起動せずに「リポジトリの外」を判定でき、時間切れと同時実行の鍵にも使う。
fn find_repo(dir: &Path) -> Option<PathBuf> {
    dir.ancestors()
        .find(|d| {
            std::fs::metadata(d.join(".git"))
                .map(|m| m.is_dir() || m.is_file())
                .unwrap_or(false)
        })
        .map(Path::to_path_buf)
}

/// git が素で open するファイルのうち、置き場所の決まっているものが通常ファイルか。
/// `.git` がディレクトリのときは、その中の設定・HEAD・index・refs・alternates も見る
/// （`gitdir:` ファイルのときは置き場所が別なので、時間の上限に任せる）。
/// FIFO やデバイスにされると git が open で止まる（時間の上限で止められるが、そのたびに
/// 2 秒待たせ、続く 30 秒は差分の機能ごと止まる）。
/// サブディレクトリの `.gitignore` などは数が読めないので、時間の上限に任せる。
fn special_files_are_regular(repo: &Path, dir: &Path) -> bool {
    let mut paths = vec![repo.join(".gitattributes"), repo.join(".gitignore")];
    if repo.join(".git").is_dir() {
        for f in ["config", "config.worktree", "HEAD", "index", "packed-refs", "info/attributes", "info/exclude", "objects/info/alternates"] {
            paths.push(repo.join(".git").join(f));
        }
    }
    if dir != repo {
        paths.push(dir.join(".gitattributes"));
        paths.push(dir.join(".gitignore"));
    }
    paths.iter().all(|p| match std::fs::metadata(p) {
        Ok(m) => m.is_file(),
        Err(_) => true,
    })
}

/// git の絶対パス。PATH の絶対パスの項目だけを見る（`.` などの相対の項目は、プロセスの
/// 作業ディレクトリ次第で開いたフォルダの `git` を拾いうる）。
fn git_path() -> Option<&'static Path> {
    static GIT: OnceLock<Option<PathBuf>> = OnceLock::new();
    GIT.get_or_init(|| {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .filter(|d| d.is_absolute())
            .map(|d| d.join("git"))
            .find(|p| is_executable(p))
    })
    .as_deref()
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

/// git の版（major, minor）。起動の経路ではなく、初めて部分クローンに当たったときに 1 回だけ
/// 確かめる（ふだんの起動で git を 1 本余計に走らせないため）。
fn version(git: &Path) -> Option<(u32, u32)> {
    static VERSION: OnceLock<Option<(u32, u32)>> = OnceLock::new();
    *VERSION.get_or_init(|| {
        let mut cmd = base_command(git, Path::new("/"));
        cmd.arg("version");
        let out = spawn_limited(cmd).ok()?;
        parse_version(&out.stdout_str())
    })
}

fn parse_version(s: &str) -> Option<(u32, u32)> {
    // 形式: "git version 2.50.1 (Apple Git-155)"
    let ver = s.split_whitespace().nth(2)?;
    let mut it = ver.split('.');
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

/// リポジトリ側の設定のうち、上書きを決めるのに要るもの。
#[derive(Default, Debug, PartialEq, Clone)]
struct RepoConfig {
    /// スコープが local / worktree の filter の名前（大文字小文字を保つ）。
    filters: Vec<String>,
    /// スコープが local / worktree の `core.excludesFile` がある。FIFO やデバイスを指されると
    /// git が open で止まるので、利用者側の値に戻す。
    repo_excludes: bool,
    /// global / system の `core.excludesFile`（最後に出たもの）。
    user_excludes: Option<String>,
    /// `extensions.partialclone` か `remote.*.promisor` がある。
    partial_clone: bool,
}

const CONFIG_REGEX: &str = r"^(filter\..+\.(clean|process|smudge|required)|core\.excludesfile|extensions\.partialclone|remote\..+\.promisor)$";

/// [`read_repo_config`] の結果を、設定ファイルの印（[`config_stamps`]）が同じ間は使い回す。
/// 鍵は `repo`（[`find_repo`]）で、呼ぶ側はそのリポジトリの鍵（[`repo_lock`]）を取っている。
///
/// 印を取れないリポジトリ（`.git` が `gitdir:` のファイル。ワークツリーやサブモジュール）は
/// 覚えず、毎回読む。
fn repo_config(git: &Path, dir: &Path, repo: &Path) -> Result<RepoConfig, GitError> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, (Vec<Stamp>, RepoConfig)>>> = OnceLock::new();
    let Some(stamps) = config_stamps(repo) else { return read_repo_config(git, dir) };
    let cache = CACHE.get_or_init(Default::default);
    {
        let map = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((seen, cfg)) = map.get(repo) {
            if *seen == stamps {
                return Ok(cfg.clone());
            }
        }
    }
    let cfg = read_repo_config(git, dir)?;
    let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
    map.insert(repo.to_path_buf(), (stamps, cfg.clone()));
    Ok(cfg)
}

/// 印を取るために読む設定ファイルの大きさの上限。手で書く設定ファイルはこれより小さい。
const CONFIG_MAX_BYTES: u64 = 1024 * 1024;

/// 設定ファイル 1 つの印。無ければ `None`（無いことも印のうち。後から作られたら変わる）。
type Stamp = (PathBuf, Option<(std::time::SystemTime, u64)>);

/// リポジトリ側の設定が変わったかを見るための、設定ファイルの印。取れないときは `None`
/// （そのリポジトリは覚えず、毎回読む）。
///
/// 見るのは `.git/config`・`.git/config.worktree` と、利用者側の global の設定ファイル
/// （`core.excludesFile` の戻し先をそこから読むため）。
///
/// 取れないのは次のとき。
/// - `.git` がディレクトリでない（`gitdir:` のファイル。ワークツリーやサブモジュール）
/// - `.git/HEAD` が無い。git はそれをリポジトリと認めず上を探すので、印を付ける先を取り違える
///   （展開したアーカイブに残った空の `.git/` など）
/// - `.git/config` か `config.worktree` に `[include]` / `[includeIf]` がある。include の先
///   （とその先、`onbranch:` ならブランチ）まで印に入れないと、先だけ書き換えられたときに
///   古い結果で filter を空にし損ねる。Why not 先を自前で辿る: git の設定ファイルの読み方
///   （タブ区切りの見出し、`;` と `#` のコメント、引用のエスケープ、`~/` と相対パス）を
///   真似る行が増えるわりに、リポジトリ側の設定に include があることは稀
fn config_stamps(repo: &Path) -> Option<Vec<Stamp>> {
    let git_dir = repo.join(".git");
    if !git_dir.is_dir() || !git_dir.join("HEAD").is_file() {
        return None;
    }
    let locals = [git_dir.join("config"), git_dir.join("config.worktree")];
    for p in &locals {
        // 通常ファイルかは special_files_are_regular が見ているが、上限はここで掛ける（設定
        // ファイルが巨大なら、読み切れなかったものとして覚えない側へ倒す）。
        if !p.exists() {
            continue;
        }
        match crate::fsread::read_regular_string(p, CONFIG_MAX_BYTES) {
            Ok(Some(text)) if !has_include(&text) => {}
            Ok(None) => {}
            _ => return None,
        }
    }
    let paths = locals.into_iter().chain(global_config_paths());
    Some(paths.map(|p| {
        let stamp = std::fs::metadata(&p).ok().and_then(|m| Some((m.modified().ok()?, m.len())));
        (p, stamp)
    }).collect())
}

/// 設定ファイルに `[include]` / `[includeIf …]` の見出しがあるか。見出しの名前は大文字小文字を
/// 問わず、後ろの区切りは空白（タブも）・`]`・引用のどれでもよい。
fn has_include(text: &str) -> bool {
    text.lines().any(|line| {
        let Some(rest) = line.trim_start().strip_prefix('[') else { return false };
        let name = rest.split(|c: char| c.is_whitespace() || c == ']' || c == '"').next().unwrap_or("");
        name.eq_ignore_ascii_case("include") || name.eq_ignore_ascii_case("includeif")
    })
}

/// 利用者側の global の設定ファイル。`GIT_CONFIG_GLOBAL`（絶対パスのときだけ残す。
/// [`KEEP_ENV`]）があればそれだけ、無ければ git の既定（`$XDG_CONFIG_HOME/git/config` と
/// `~/.gitconfig`）。
fn global_config_paths() -> Vec<PathBuf> {
    if let Some(p) = std::env::var_os("GIT_CONFIG_GLOBAL").map(PathBuf::from).filter(|p| p.is_absolute()) {
        return vec![p];
    }
    let xdg = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("git/config"));
    let home = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".gitconfig"));
    xdg.into_iter().chain(home).collect()
}

/// filter と excludesFile と部分クローンの設定を、`git config` の 1 回で読む。本体と同じ `-C` で
/// 走らせる（サブモジュールの中なら、そのサブモジュールの設定が読まれるように）。
fn read_repo_config(git: &Path, dir: &Path) -> Result<RepoConfig, GitError> {
    let mut cmd = base_command(git, dir);
    cmd.args(["config", "--show-scope", "--null", "--get-regexp", CONFIG_REGEX]);
    let out = spawn_limited(cmd)?;
    // 1 件も無いときは終了コード 1 で、出力は空。どちらも同じ読み方で足りる。
    Ok(parse_repo_config(&out.stdout))
}

fn parse_repo_config(raw: &[u8]) -> RepoConfig {
    let mut cfg = RepoConfig::default();
    // 形式: `<scope>\0<key>\n<value>\0`。値の無いキー（`[core] x` だけ）は `<key>\0`。
    let mut fields = raw.split(|b| *b == 0);
    while let (Some(scope), Some(entry)) = (fields.next(), fields.next()) {
        let entry = String::from_utf8_lossy(entry);
        let (key, value) = match entry.split_once('\n') {
            Some((k, v)) => (k, Some(v)),
            None => (entry.as_ref(), None),
        };
        let repo_side = matches!(scope, b"local" | b"worktree");
        if let Some(rest) = key.strip_prefix("filter.") {
            if let (true, Some((name, _))) = (repo_side, rest.rsplit_once('.')) {
                if !cfg.filters.iter().any(|f| f == name) {
                    cfg.filters.push(name.to_string());
                }
            }
        } else if key == "core.excludesfile" {
            if repo_side {
                cfg.repo_excludes = true;
            } else if let Some(v) = value {
                // 後から出るものほど強いスコープなので、最後の値が効いている値。
                cfg.user_excludes = Some(v.to_string());
            }
        } else if key == "extensions.partialclone" || key.ends_with(".promisor") {
            cfg.partial_clone = true;
        }
    }
    cfg
}

/// `core.excludesFile` が設定されていないときに git が読む場所。
fn default_excludes() -> String {
    std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|d| d.join("git/ignore").to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// 毎回渡す設定の上書き。
fn overrides(cfg: &RepoConfig) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![
        ("core.quotepath".into(), "false".into()),
        ("core.hooksPath".into(), "/dev/null".into()),
        // いま使っている plumbing（diff-index・show）は読まない。porcelain の `git diff` を
        // 足したときにリポジトリの指すファイル（FIFO など）を読ませないための先回り。空文字だと
        // `git diff` が「orderfile を読めない」で失敗するので /dev/null にする。
        ("diff.orderFile".into(), "/dev/null".into()),
        ("core.attributesFile".into(), String::new()),
        // フックのコマンドのパスだけでなく、真偽値の true（git 内蔵の監視）も止める。内蔵の
        // 監視は呼ぶたびにデーモンを起こし、こちらがプロセスグループごと止めるので、次の
        // 呼び出しでまた起こして 1 秒ほど待たされる。diff-index は index を更新しないので、
        // 監視があっても速くならない。
        ("core.fsmonitor".into(), "false".into()),
    ];
    if cfg.repo_excludes {
        out.push(("core.excludesFile".into(), cfg.user_excludes.clone().unwrap_or_else(default_excludes)));
    }
    for name in &cfg.filters {
        for part in ["clean", "process", "smudge"] {
            out.push((format!("filter.{name}.{part}"), String::new()));
        }
        out.push((format!("filter.{name}.required"), "false".into()));
    }
    out
}

/// 引き継いだ `GIT_*` のうち残すもの。どれも利用者が自分の設定ファイルの置き場所を決めるもの
/// （パスのものは絶対パスのときだけ）。
const KEEP_ENV: [&str; 3] = ["GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM", "GIT_CONFIG_NOSYSTEM"];

/// 守りの土台だけを敷いた `git -C <dir>`。
fn base_command(git: &Path, dir: &Path) -> Command {
    let mut cmd = Command::new(git);
    // 設定の置き場所を指すものだけは利用者の環境のまま（モジュールの doc）。それ以外の
    // `GIT_*` は、何をするものでも消す（外から差し込まれる道を名指しで塞ぐより、残すものを
    // 名指しにする方が、git に変数が増えても閉じたまま）。
    // 残すのは絶対パスのときだけ。相対パスは git が `-C <dir>`（開いたリポジトリの中）で
    // 解決するので、リポジトリに同じ名前で置かれたファイルが利用者の設定として読まれる。
    for (key, value) in std::env::vars_os() {
        let k = key.to_string_lossy();
        if !k.starts_with("GIT_") {
            continue;
        }
        let keep = KEEP_ENV.contains(&k.as_ref())
            && (k == "GIT_CONFIG_NOSYSTEM" || Path::new(&value).is_absolute());
        if !keep {
            cmd.env_remove(&key);
        }
    }
    cmd.env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_ALLOW_PROTOCOL", "none")
        // 読むだけのコマンドでも、index の統計を書き戻すことがある。利用者の index を変えない。
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(dir)
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        // 子が孫（filter や監視のコマンド）を起こしても、まとめて止められるように。
        .process_group(0);
    cmd
}

/// 時間と出力量の上限を掛けて走らせる。超えたら SIGTERM、続けてプロセスグループごと SIGKILL。
///
/// 終了と出力は、それぞれ専用のスレッドで待って通り道で受ける。`try_wait` を刻んで回すと、
/// 1 本ごとに刻みのぶんだけ遅れる。出力を join で待たないのは、グループの外へ抜けた孫が
/// 出力の口を握ったままのとき、ここで永遠に止まる（そのリポジトリの鍵も握ったまま）ため。
fn spawn_limited(mut cmd: Command) -> Result<GitOutput, GitError> {
    let mut child = cmd.spawn().map_err(|_| GitError::Spawn)?;
    let pgid = child.id() as libc::pid_t;
    let mut stdout = child.stdout.take().ok_or(GitError::Spawn)?;
    let (out_tx, out_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stdout).take(OUTPUT_MAX + 1).read_to_end(&mut buf);
        let _ = out_tx.send(buf);
    });
    let (exit_tx, exit_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = exit_tx.send(child.wait().ok());
    });

    let deadline = Instant::now() + TIMEOUT;
    // 先に出力を待つ。上限まで出し切った子は書き込みで止まるので、出力が揃った時点で
    // 終了を待ちにいく。
    let stdout = out_rx.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    let status = match &stdout {
        Ok(buf) if buf.len() as u64 > OUTPUT_MAX => None,
        Ok(_) => exit_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).ok().flatten(),
        Err(_) => None,
    };
    let Some(status) = status else {
        kill_group(pgid, &exit_rx);
        return Err(match stdout {
            Ok(buf) if buf.len() as u64 > OUTPUT_MAX => GitError::TooLarge,
            _ => GitError::TimedOut,
        });
    };
    let stdout = stdout.unwrap_or_default();
    Ok(GitOutput { success: status.success(), stdout })
}

/// SIGTERM を送り、少し待って終わらなければグループごと SIGKILL。
///
/// Why not 正常に終わった後もグループを止める: git が起こしたデーモン（内蔵の fsmonitor など）
/// まで巻き込み、次の呼び出しで起動し直しになって 1 秒ほど待たされる。孫が出力の口を
/// 握り続ける場合は、上の通り道の時間切れで抜ける。
fn kill_group(pgid: libc::pid_t, exit_rx: &std::sync::mpsc::Receiver<Option<std::process::ExitStatus>>) {
    // SAFETY: killpg は記述子もメモリも触らない。pgid は自分が作った子のグループ。
    unsafe { libc::killpg(pgid, libc::SIGTERM) };
    if exit_rx.recv_timeout(Duration::from_millis(100)).is_err() {
        kill_group_quietly(pgid);
    }
}

fn kill_group_quietly(pgid: libc::pid_t) {
    // SAFETY: 同上。グループがもう無ければ ESRCH で何もしない。
    unsafe { libc::killpg(pgid, libc::SIGKILL) };
}

fn repo_lock(repo: &Path) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    let mut map = LOCKS.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    map.entry(repo.to_path_buf()).or_default().clone()
}

fn cooldowns() -> &'static Mutex<HashMap<PathBuf, Instant>> {
    static COOLDOWNS: OnceLock<Mutex<HashMap<PathBuf, Instant>>> = OnceLock::new();
    COOLDOWNS.get_or_init(Default::default)
}

fn in_cooldown(repo: &Path) -> bool {
    let map = cooldowns().lock().unwrap_or_else(|e| e.into_inner());
    map.get(repo).is_some_and(|until| Instant::now() < *until)
}

fn note_timeout(repo: &Path, e: &GitError) {
    if matches!(e, GitError::TimedOut) {
        let mut map = cooldowns().lock().unwrap_or_else(|e| e.into_inner());
        map.insert(repo.to_path_buf(), Instant::now() + COOLDOWN);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scoped_config_and_keeps_filter_case() {
        let raw = b"global\0filter.lfs.clean\ngit-lfs clean\0local\0filter.My=Lfs.clean\ntouch x\0local\0filter.My=Lfs.required\ntrue\0worktree\0filter.wt.process\nx\0";
        let cfg = parse_repo_config(raw);
        assert_eq!(cfg.filters, vec!["My=Lfs".to_string(), "wt".to_string()]);
        assert!(!cfg.partial_clone);
    }

    #[test]
    fn fsmonitor_is_always_off() {
        assert!(overrides(&parse_repo_config(b"")).iter().any(|(k, v)| k == "core.fsmonitor" && v == "false"));
    }

    #[test]
    fn repo_side_excludes_file_is_put_back_to_the_users_value() {
        let cfg = parse_repo_config(b"global\0core.excludesfile\n/home/me/.gitignore_global\0local\0core.excludesfile\n/tmp/fifo\0");
        let ov = overrides(&cfg);
        assert!(ov.iter().any(|(k, v)| k == "core.excludesFile" && v == "/home/me/.gitignore_global"));
        // 利用者側に無ければ、git の既定の場所へ。
        let cfg = parse_repo_config(b"local\0core.excludesfile\n/tmp/fifo\0");
        assert!(overrides(&cfg).iter().any(|(k, v)| k == "core.excludesFile" && v.ends_with("git/ignore")));
        // リポジトリ側に無ければ触らない。
        let cfg = parse_repo_config(b"global\0core.excludesfile\n/home/me/.gitignore_global\0");
        assert!(!overrides(&cfg).iter().any(|(k, _)| k == "core.excludesFile"));
    }

    #[test]
    fn detects_partial_clone() {
        assert!(parse_repo_config(b"local\0remote.origin.promisor\ntrue\0").partial_clone);
        assert!(parse_repo_config(b"local\0extensions.partialclone\norigin\0").partial_clone);
    }

    #[test]
    fn has_include_sees_include_and_include_if_headings_only() {
        assert!(has_include("[core]\n\tbare = false\n[include]\n\tpath = ../x\n"));
        assert!(has_include("[includeIf \"gitdir:~/work/\"]\n\tpath = x\n"));
        assert!(has_include("  [INCLUDEIF\t\"onbranch:main\"]\n"));
        assert!(!has_include("[core]\n\tincludepath = x\n[user]\n\tname = include\n"));
        assert!(!has_include("[includes]\n\tpath = x\n"));
    }

    #[test]
    fn config_stamps_change_with_the_config_and_are_not_taken_for_odd_repositories() {
        let dir = std::env::temp_dir().join(format!("md-git-stamps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(".git/config"), "[core]\n\tbare = false\n").unwrap();
        // HEAD の無い `.git` はリポジトリではない（git は上を探す）。
        assert!(config_stamps(&dir).is_none());
        std::fs::write(dir.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let a = config_stamps(&dir).unwrap();
        std::fs::write(dir.join(".git/config"), "[core]\n\tbare = false\n[filter \"x\"]\n\tclean = touch y\n").unwrap();
        let b = config_stamps(&dir).unwrap();
        assert_ne!(a, b, "設定が変われば印が変わる");
        // config.worktree が後から作られても変わる（無いことも印のうち）。
        std::fs::write(dir.join(".git/config.worktree"), "[core]\n").unwrap();
        assert_ne!(b, config_stamps(&dir).unwrap());
        // include があれば覚えない。
        std::fs::write(dir.join(".git/config.worktree"), "[include]\n\tpath = ../x\n").unwrap();
        assert!(config_stamps(&dir).is_none());
        // `.git` が gitdir: のファイルなら印は取れない（覚えない）。
        let wt = dir.join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: ../.git/worktrees/wt\n").unwrap();
        assert!(config_stamps(&wt).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_apple_git_version() {
        assert_eq!(parse_version("git version 2.50.1 (Apple Git-155)\n"), Some((2, 50)));
        assert_eq!(parse_version("git version 2.43.0\n"), Some((2, 43)));
        assert_eq!(parse_version("nonsense"), None);
    }
}
