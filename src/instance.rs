//! 単一インスタンス化の配管（#31）。**受け側は 1 プロセスだけ**を保証し、2 回目以降の
//! `md` が開きたいファイルを既存の窓へ渡す。
//!
//! # 所有権はソケットではなく flock
//!
//! 「ソケットファイルが在るかどうか」で受け側を決めると、残骸から復旧する手順そのものが
//! 競合する。2 つが同時に「前の受け側は死んでいる」と判定して両方 unlink → 両方 bind
//! すると、勝者の listener は名前を失い、敗者のソケットだけが残って**両方が自分を
//! 受け側だと思う**。
//!
//! そこで `<socket>.lock` の [`flock`](std::fs::File::try_lock) を唯一のトークンにする。
//! 取れたか取れなかったかがその場で分かるので敗者が自分の負けを知れるし、プロセスが
//! 死ねば kernel が必ず解放するので「残骸ロック」という状態が存在しない。ソケット
//! ファイルは「ロック保持者だけが unlink し直す派生物」に格下げされ、**残骸のソケットは
//! 異常ではなく通常状態**になる。
//!
//! Why not 一時名で bind して `rename()` で貼り替える: 名前の持ち主は一意に決まるが、
//! 敗者の listener は生きたまま名前だけ失う。敗者が自分の負けを知れないので、そのまま
//! 窓を開いてしまい、潰したかった「同時に 2 枚」がそのまま残る。
//!
//! Why not `fcntl(F_SETLK)`: あちらはプロセス単位なので、同じプロセス内の別の fd から
//! 取り直すと成功してしまう（取り合いのテストが 1 プロセスで書けない）。無関係な
//! `close()` でロックが落ちる罠もある。`flock` は open file description 単位。
//!
//! # ワイヤ
//!
//! ```text
//! 受け側の挨拶（accept 直後、読む前に書く）  md-preview\0 1\0 pid=<受け側pid>\0
//! 送り側の要求                               md-preview\0 1\0 <key>=<value>\0 …
//! ```
//!
//! NUL 区切りにするのは、macOS のパスに入らない唯一のバイトが NUL だから。改行も
//! 空白も `=` も引用符もそのまま通る。長さの前置きは持たない（1 接続 1 通で、送り側が
//! 書き終えたら write 側を閉じる）。持つと「長さと実体の食い違い」という失敗モードを
//! 自分で作ることになる。
//!
//! **未知のキーは黙って無視する。** だからキーを足すときにバージョンは上げない
//! （#36 の `--notify` が後から同じソケットに乗れる）。上げるのは既存キーの意味が
//! 変わるときだけ。
//!
//! Why not ack を返す: 前面化は送り側が撃つ（`spike/activation` の実測）ので、送り側は
//! 転送の成否を待つ必要が無い。代わりに受け側が**送る前に**挨拶を返す形にした。前面化に
//! 要る受け側 pid と、プロトコルのバージョン不一致（`cargo install` で入れ替えた直後に
//! 古い窓が生きているのは日常）が、どちらも送る前に分かる。

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// ワイヤの先頭に置く目印。別のプログラムが同じパスに居たときに、こちらの解釈を
/// 押し付けずに諦めるための札。
const MAGIC: &str = "md-preview";

/// プロトコルのバージョン。
const VERSION: u32 = 1;

/// 1 通の上限。壊れた相手や別のプログラムに繋がれたときに、メモリを食い尽くさない。
const MAX_MESSAGE: u64 = 1 << 20;

/// 挨拶を待つ時間。受け側が詰まっている（生きているが accept の先で止まっている）
/// ときに、送り側が端末を握ったままにならない長さ。
const GREETING_TIMEOUT: Duration = Duration::from_millis(200);

/// `sun_path` の上限（macOS）。終端の 1 バイトを引いた長さまでしか bind できない。
const SUN_PATH_MAX: usize = 104;

/// ソケットの名前。debug ビルドで分けるのは `cargo run` が日常の `md` の受け側を
/// 奪わないようにするため。**これで分かれるのは `cargo run` だけ。**
/// `cargo install --path . --root …` で入れた検証用のビルドはリリースなので同じ名前に
/// なり、日常の `md` の窓へ転送してしまう。そちらは [`SOCKET_ENV`] で分ける
/// （手順は CLAUDE.md にある）。
const SOCKET_NAME: &str = if cfg!(debug_assertions) { "md-preview-dev.sock" } else { "md-preview.sock" };

/// ソケットのパスを外から差し替える。テストが本番のソケットを踏まないため、そして
/// 開発機で日常の `md` と検証用のビルドを分けるため。
const SOCKET_ENV: &str = "MD_SOCKET";

/// ソケットとロックの置き場所。
#[derive(Clone, Debug)]
pub struct Endpoint {
    socket: PathBuf,
    lock: PathBuf,
}

impl Endpoint {
    /// 明示したパスを使う。テストの入口でもある。
    pub fn at(socket: PathBuf) -> Self {
        let mut lock = socket.clone().into_os_string();
        lock.push(".lock");
        Endpoint { socket, lock: PathBuf::from(lock) }
    }

    /// このユーザーの既定の置き場所。`MD_SOCKET` →`$TMPDIR` → `~/.config/md-preview/run`。
    ///
    /// 長すぎるパスは `None`。単一インスタンス化を諦めるだけで、窓は普通に開く
    /// （`$TMPDIR` は実測 49 文字前後なので通常は当たらない）。
    ///
    /// Why not `/tmp/md-preview-<uid>.sock`: uid を取るためだけに `libc` を直接依存へ
    /// 足すことになる。しかも `/tmp` は全ユーザー共有なので、先回りして同名のソケットを
    /// 置かれると「connect は通るが相手は他人」になり、所有者とモードの検証を自前で
    /// 書く羽目になる。`$TMPDIR` はユーザー毎（0700）で、`config_dir()` も同じ。
    pub fn user_default() -> Option<Endpoint> {
        let socket = match std::env::var_os(SOCKET_ENV) {
            Some(p) if !p.is_empty() => PathBuf::from(p),
            _ => {
                let tmp = std::env::temp_dir();
                if tmp.as_os_str().is_empty() {
                    let run = crate::config_dir()?.join("run");
                    create_private_dir(&run).ok()?;
                    // ここでも SOCKET_NAME を通す。名前を変えると debug と release が
                    // 同じソケットを共有する枝ができる（`std::env::temp_dir()` は
                    // TMPDIR 未設定でも `/tmp` を返すので普通は到達しないが、
                    // 到達しない枝に別の規則を置くと次に触る人がそれを正だと読む）。
                    run.join(SOCKET_NAME)
                } else {
                    tmp.join(SOCKET_NAME)
                }
            }
        };
        let ep = Endpoint::at(socket);
        if !ep.is_bindable() {
            return None;
        }
        Some(ep)
    }

    /// `sun_path` に収まるか。収まらないパスは bind で必ず失敗するので、そこまで
    /// 行かずに単一インスタンス化を諦める。
    pub fn is_bindable(&self) -> bool {
        self.socket.as_os_str().len() < SUN_PATH_MAX
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket
    }
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)
}

/// 転送する内容。`files` の順序がそのままタブの並びになる（先頭が表示される）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Message {
    /// タブに並べるファイルの識別子（絶対パス）。
    pub files: Vec<String>,
    /// 掃除の所有権を渡す一時ディレクトリ（stdin を実体化した場所）。
    /// 受け側は**自分でも門を通してから**引き取ること（ワイヤから来た値なので）。
    pub own: Vec<String>,
    /// 送り側の作業ディレクトリ。いまは診断用で、#34 がディレクトリを受けるときに要る。
    pub cwd: Option<String>,
    /// 送り側の pid。いまは診断用。
    pub sender_pid: Option<i32>,
    /// 受け側を前面に出すか。#38 の 4 値設定の受け皿で、いまは常に true。
    pub activate: bool,
}

impl Message {
    pub fn new(files: Vec<String>) -> Self {
        Message { files, activate: true, ..Default::default() }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        push_token(&mut out, MAGIC);
        push_token(&mut out, &VERSION.to_string());
        if let Some(cwd) = &self.cwd {
            push_token(&mut out, &format!("cwd={}", cwd));
        }
        if let Some(pid) = self.sender_pid {
            push_token(&mut out, &format!("pid={}", pid));
        }
        push_token(&mut out, if self.activate { "activate=1" } else { "activate=0" });
        for f in &self.files {
            push_token(&mut out, &format!("file={}", f));
        }
        for d in &self.own {
            push_token(&mut out, &format!("own={}", d));
        }
        out
    }

    /// 壊れた入力は `None`。マジックとバージョンが読めなければ、他人のプログラムか
    /// 別世代の md なので何も解釈しない。
    pub fn decode(bytes: &[u8]) -> Option<Message> {
        let mut tokens = split_tokens(bytes).into_iter();
        if tokens.next()? != MAGIC {
            return None;
        }
        let version: u32 = tokens.next()?.parse().ok()?;
        if version > VERSION {
            return None;
        }
        let mut msg = Message { activate: true, ..Default::default() };
        for token in tokens {
            let Some((key, value)) = token.split_once('=') else { continue };
            match key {
                "file" => msg.files.push(value.to_string()),
                "own" => msg.own.push(value.to_string()),
                "cwd" => msg.cwd = Some(value.to_string()),
                "pid" => msg.sender_pid = value.parse().ok(),
                "activate" => msg.activate = value != "0",
                // 未知のキーは無視する。前方互換の約束そのものなので、ここで
                // エラーにしてはいけない。
                _ => {}
            }
        }
        Some(msg)
    }
}

/// 値に NUL は入らない（パスに入らないバイトを区切りに選んである）。万一入っていたら
/// 区切りが崩れるので、そこで切り捨てる。
fn push_token(out: &mut Vec<u8>, token: &str) {
    let bytes = token.as_bytes();
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    out.extend_from_slice(&bytes[..end]);
    out.push(0);
}

/// 末尾の NUL までを 1 トークンとする。終端されていない尻尾（途中で切れた通信）は捨てる。
fn split_tokens(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in bytes.split(|b| *b == 0) {
        out.push(String::from_utf8_lossy(chunk).into_owned());
    }
    // split は末尾の区切りの後ろに空要素を作る。それが無いなら終端されていない。
    match out.pop() {
        Some(tail) if tail.is_empty() => out,
        _ => Vec::new(),
    }
}

/// [`claim`] の結果。
pub enum Claim {
    /// 自分が受け側。`listen()` を呼ぶまでソケットは開かない。
    Owner(Owner),
    /// 生きた受け側が既に居る（まだ bind していないだけ、ということもある）。
    Taken,
    /// ロックそのものが取れない（置き場所が無い、ロックを持てない FS）。
    /// 単一インスタンス化を諦めて、従来どおり窓を開く。
    Failed(std::io::Error),
}

/// 受け側の座を取りに行く。**プロセスが生きている限りロックを手放さない**ので、
/// 返ってきた [`Owner`] / [`Listening`] を drop してはいけない。
pub fn claim(ep: &Endpoint) -> Claim {
    let lock = match File::options().read(true).write(true).create(true).open(&ep.lock) {
        Ok(f) => f,
        Err(e) => return Claim::Failed(e),
    };
    match lock.try_lock() {
        Ok(()) => Claim::Owner(Owner { lock, ep: ep.clone() }),
        Err(std::fs::TryLockError::WouldBlock) => Claim::Taken,
        Err(std::fs::TryLockError::Error(e)) => Claim::Failed(e),
    }
}

/// ロックを握った状態。まだソケットは開いていない。
pub struct Owner {
    lock: File,
    ep: Endpoint,
}

impl Owner {
    /// ソケットを開く。**残骸の有無を判定しない。** ロック保持者だけがこの経路に
    /// 来るので、無条件に unlink → bind して競合しない。
    pub fn listen(self) -> std::io::Result<Listening> {
        // 中身は診断用（誰も判断に使わない）。判断に使うのは flock が取れたかどうかだけ。
        // truncate してから書く。前の所有者の方が pid が長いと尻尾が残る。
        let _ = self.lock.set_len(0);
        let _ = (&self.lock).write_all(format!("{}\n", std::process::id()).as_bytes());
        let _ = std::fs::remove_file(&self.ep.socket);
        let listener = UnixListener::bind(&self.ep.socket)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.ep.socket, std::fs::Permissions::from_mode(0o600));
        }
        // 自分が bind したソケットの身元。後始末で「他人のソケットを消していないか」を
        // 見るために覚えておく（パスの一致では足りない。`.lock` が $TMPDIR の定期掃除で
        // 消えると二重所有が起こりえて、そのとき古い方の unlink が新しい所有者の
        // ソケットを消すと、以降すべての md が座無しで窓を開く幽霊状態になる）。
        let ino = file_identity(&self.ep.socket);
        Ok(Listening { _lock: self.lock, listener, ep: self.ep, ino })
    }
}

/// bind まで済んだ状態。
pub struct Listening {
    _lock: File,
    listener: UnixListener,
    ep: Endpoint,
    ino: Option<(u64, u64)>,
}

impl Listening {
    /// accept ループを別スレッドで回す。`self` ごと移すので、ロックはプロセスが
    /// 死ぬまで保持される。
    ///
    /// 接続ごとにスレッドを立てない。流量は「たまに 1 通」で、読み書きに時間制限を
    /// 張ってあるので、詰まった送り側が後続を遅らせるのは高々その時間まで。
    ///
    /// 返る [`Handle`] は後始末のためだけのもの。呼び出し側は GUI の終了経路で
    /// [`Handle::unlink`] を呼ぶ（呼ばなくても次の起動が直すので、衛生の話）。
    pub fn serve(self, mut on_message: impl FnMut(Message) + Send + 'static) -> Handle {
        let handle = Handle { socket: self.ep.socket.clone(), ino: self.ino };
        std::thread::spawn(move || {
            for stream in self.listener.incoming() {
                // Err でループを抜けない。抜けると窓は生きたまま受信できない幽霊になる。
                let Ok(stream) = stream else { continue };
                if let Some(msg) = read_request(stream) {
                    on_message(msg);
                }
            }
        });
        handle
    }
}

/// 後始末の口。`tao` の `EventLoop::run` は終了時に `process::exit` するので `Drop` は
/// 走らない。走らない `Drop` は嘘なので実装せず、終了経路から明示的に呼ぶ形にする。
pub struct Handle {
    socket: PathBuf,
    ino: Option<(u64, u64)>,
}

impl Handle {
    /// ソケットファイルを消す。**取りこぼしても壊れない**（次の起動の `listen()` が
    /// 無条件に unlink → bind する）ので、クラッシュや `terminate:` に対策は打たない。
    ///
    /// 消す前に身元を照合するのは、取りこぼしより**取り違え**の方が高くつくため。
    /// 他人が bind し直したソケットを消すと「ロックは埋まっているのにソケットが無い」
    /// 状態になり、以降すべての `md` が座を取れないまま 2 枚目を開き続ける。
    pub fn unlink(&self) {
        if self.ino.is_some() && file_identity(&self.socket) != self.ino {
            return;
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// ファイルの身元（デバイス番号と inode）。同じパスに別のファイルが置き直されたことを
/// 見分けるために使う（`bundle.rs` の `already_inside` と同じ手）。
fn file_identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::symlink_metadata(path).ok()?;
    Some((m.dev(), m.ino()))
}

/// 受け側が 1 接続を捌く。挨拶を書いてから要求を読む。
fn read_request(mut stream: UnixStream) -> Option<Message> {
    let _ = stream.set_write_timeout(Some(GREETING_TIMEOUT));
    let _ = stream.set_read_timeout(Some(GREETING_TIMEOUT));
    let mut greeting = Vec::new();
    push_token(&mut greeting, MAGIC);
    push_token(&mut greeting, &VERSION.to_string());
    push_token(&mut greeting, &format!("pid={}", std::process::id()));
    stream.write_all(&greeting).ok()?;
    stream.flush().ok()?;

    let mut buf = Vec::new();
    // 送り側は書き終えたら write 側を閉じるので、ここは EOF で戻る。
    Read::take(&mut stream, MAX_MESSAGE).read_to_end(&mut buf).ok()?;
    // 上限ちょうどは「切り捨てた」疑いがある。切れた位置がたまたま NUL の直後だと
    // `split_tokens` が終端済みと読み、**パスの途中までを開く**ことになる。
    if buf.len() as u64 >= MAX_MESSAGE {
        return None;
    }
    Message::decode(&buf)
}

/// [`try_send`] の結果。
#[derive(Debug, PartialEq, Eq)]
pub enum Sent {
    /// 書き切った。`receiver_pid` は挨拶から読めた受け側の pid で、前面化に使う。
    /// 挨拶が読めなかった（詰まっている）ときは `None` で、前面化はしない。
    Delivered { receiver_pid: Option<i32> },
    /// 受け側が居ない。冷スタートへ落ちる。
    NoReceiver,
    /// 相手の方が新しい。**何も書かずに**冷スタートへ落ちる。
    Incompatible { version: u32 },
}

/// 既存の受け側へ 1 通送る。**ack は待たない。**
pub fn try_send(ep: &Endpoint, msg: &Message) -> Sent {
    let Ok(mut stream) = UnixStream::connect(&ep.socket) else {
        return Sent::NoReceiver;
    };
    let _ = stream.set_read_timeout(Some(GREETING_TIMEOUT));
    let _ = stream.set_write_timeout(Some(GREETING_TIMEOUT));

    // 挨拶が読めないまま書くのは「生きているが accept の先で詰まっている」相手だけ。
    // そこで諦めて冷スタートすると、後から捌かれる 1 通とぶつかって窓が 2 枚になる。
    //
    // ただし**マジックが違う相手には 1 バイトも書かない**。MAGIC は「他人のプログラムに
    // こちらの解釈を押し付けない札」なので、読む側で使って書く側で無視したら意味が無い。
    let receiver_pid = match read_greeting(&mut stream) {
        Some(Greeting::Ok { pid }) => pid,
        Some(Greeting::Incompatible(version)) => return Sent::Incompatible { version },
        Some(Greeting::Stranger) => return Sent::NoReceiver,
        // タイムアウト / EOF。送るだけ送って、前面化はしない（生きている確信が無い）。
        None => None,
    };

    if stream.write_all(&msg.encode()).is_err() {
        return Sent::NoReceiver;
    }
    let _ = stream.flush();
    // 受け側の read_to_end を EOF で返すために、書き終えたら write 側を閉じる。
    let _ = stream.shutdown(std::net::Shutdown::Write);
    Sent::Delivered { receiver_pid }
}

enum Greeting {
    Ok { pid: Option<i32> },
    Incompatible(u32),
    /// 挨拶は返ってきたが md のものではない。同じパスに別のプログラムが居る。
    Stranger,
}

/// 挨拶は 3 トークンで終わる。相手が黙っているときに待ち続けないよう、読み取りには
/// 時間制限が張ってある（呼び出し側で設定済み）。
fn read_greeting(stream: &mut UnixStream) -> Option<Greeting> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 64];
    while buf.iter().filter(|b| **b == 0).count() < 3 {
        match stream.read(&mut chunk) {
            Ok(0) => return None,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => return None,
        }
        if buf.len() > 256 {
            return None;
        }
    }
    let tokens = split_tokens(&buf);
    let mut it = tokens.into_iter();
    if it.next().as_deref() != Some(MAGIC) {
        return Some(Greeting::Stranger);
    }
    let Some(version) = it.next().and_then(|v| v.parse::<u32>().ok()) else {
        return Some(Greeting::Stranger);
    };
    if version > VERSION {
        return Some(Greeting::Incompatible(version));
    }
    let pid = it
        .find_map(|t| t.strip_prefix("pid=").map(str::to_string))
        .and_then(|v| v.parse().ok());
    Some(Greeting::Ok { pid })
}
