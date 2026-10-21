//! 本文・差分・埋め込み・配信で中身を読むときの唯一の入口。
//!
//! 開いたフォルダの中には、文書（AI が書いたものを含む）が指せるものなら何でも置ける。
//! シンボリックリンクの先が `/dev/zero` や FIFO だと、素の `std::fs::read` は返ってこないか
//! メモリを食い尽くす。デバイスによっては open するだけで副作用がある。なので、
//! 「通常ファイルで、上限より小さいもの」だけを読む。
//!
//! 手順は stat → open（`O_NONBLOCK | O_NOCTTY`）→ fstat → `O_NONBLOCK` を外して読む。
//! - stat を open より先にするのは、デバイスと FIFO を open させないため
//! - open の後で fstat し直すのは、stat と open の間に差し替えられても通さないため
//! - `O_NONBLOCK` は、差し替えで FIFO を掴んでも open で止まらないため。読む前に外すのは、
//!   iCloud から追い出されたファイル（dataless）の実体化を待たずに EAGAIN で返られないため
//!   （見込み。実機の dataless ファイルではまだ確かめていない）
//! - `O_CLOEXEC` は std が常に付けている

use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::Path;

/// テキストとして描く経路（本文・ソース表示・差分・`.html`）の上限。
/// 今まで開けていた大きさ（実用では数 MB まで）より十分上に置く。hljs を諦める閾値
/// （`request.rs` の `HIGHLIGHT_MAX_BYTES`）とは別物で、こちらは「そもそも読まない」線。
pub const TEXT_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// 配信する画像・動画・PDF・CSS・フォントなどを、まるごと返すときの上限。全量をメモリに
/// 載せて返すので、1GB 超のファイルを `<img>` で指されてもこの大きさで止まる。
/// 動画と音声の `Range` の要求は、[`open_regular`] で開いて要る範囲だけを読むので、
/// この上限を受けない（`request.rs` の `serve_range`）。
pub const ASSET_MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug)]
pub enum ReadError {
    /// 通常ファイルではない（ディレクトリ・デバイス・FIFO・ソケット）か、自動マウントの下。
    NotRegular,
    /// 上限を超えている。中身は実際の大きさ。
    TooLarge(u64),
    Io(io::Error),
}

/// 「大きすぎて開けない」ときに本文へ出す文言。経路ごとに書き分けないよう 1 か所に置く。
pub fn too_large_message(path: &Path, size: u64, limit: u64) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    format!(
        "ファイルが大きすぎるため表示しません: {}（{}。上限 {}）",
        name,
        human_size(size),
        human_size(limit)
    )
}

fn human_size(n: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if n >= MB {
        format!("{} MB", n.div_ceil(MB))
    } else {
        format!("{} KB", n.div_ceil(1024))
    }
}

/// 触るだけで自動マウントが走る場所（`/net` と `/Network`）か。stat も canonicalize も
/// マウントを起こすので、パスの文字列だけで先に断る。
///
/// Why not パスを解決してから見る: 解決する時点でマウントが起きる。シンボリックリンク
/// 越しに指された場合のマウントは防げない（解決後のパスを [`read_regular`] がもう一度見るので、
/// 読むことは防げる）。
///
/// 大文字小文字は区別しない（既定の APFS は区別しないので、`/NET/…` も同じ場所に着く）。
pub fn is_automount_path(path: &Path) -> bool {
    let mut parts = path.components();
    if parts.next() != Some(std::path::Component::RootDir) {
        return false;
    }
    match parts.next() {
        Some(std::path::Component::Normal(first)) => {
            let first = first.to_string_lossy();
            first.eq_ignore_ascii_case("net") || first.eq_ignore_ascii_case("Network")
        }
        _ => false,
    }
}

/// 通常ファイルを `limit` バイトまで読む。超えていれば [`ReadError::TooLarge`]。
pub fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>, ReadError> {
    let (file, len) = open_regular(path, limit)?;
    // 読んでいる最中に伸びるファイルもあるので、読む量そのものも上限で切る
    // （1 バイト多く読めたら上限超え）。
    let mut buf = Vec::with_capacity(len as usize);
    file.take(limit + 1).read_to_end(&mut buf).map_err(ReadError::Io)?;
    if buf.len() as u64 > limit {
        return Err(ReadError::TooLarge(buf.len() as u64));
    }
    Ok(buf)
}

/// 通常ファイルで `limit` バイト以下のものだけを、読める状態で開く。返すのは開いたファイルと、
/// 開いた時点の大きさ。一部だけを読む呼び出し側（Range の応答など）はこちらを使う。
pub fn open_regular(path: &Path, limit: u64) -> Result<(File, u64), ReadError> {
    if is_automount_path(path) {
        return Err(ReadError::NotRegular);
    }
    let meta = std::fs::metadata(path).map_err(ReadError::Io)?;
    if !meta.is_file() {
        return Err(ReadError::NotRegular);
    }
    if meta.len() > limit {
        return Err(ReadError::TooLarge(meta.len()));
    }

    // fsread を通さない: ここが通す側
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
        .open(path)
        .map_err(ReadError::Io)?;
    let meta = file.metadata().map_err(ReadError::Io)?;
    if !meta.is_file() {
        return Err(ReadError::NotRegular);
    }
    if meta.len() > limit {
        return Err(ReadError::TooLarge(meta.len()));
    }
    clear_nonblock(&file).map_err(ReadError::Io)?;
    Ok((file, meta.len()))
}

/// [`read_regular`] の UTF-8 版。非 UTF-8 は `Ok(None)`（バイナリとして扱うかは呼び出し側）。
pub fn read_regular_string(path: &Path, limit: u64) -> Result<Option<String>, ReadError> {
    read_regular(path, limit).map(|b| String::from_utf8(b).ok())
}

fn clear_nonblock(file: &File) -> io::Result<()> {
    let fd = file.as_raw_fd();
    // SAFETY: fd は `file` が持っている開いた記述子で、この呼び出しの間は閉じられない。
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: 同上。
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 一時ディレクトリ。抜けるときに消す。
    struct TmpDir(std::path::PathBuf);
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    impl std::ops::Deref for TmpDir {
        type Target = std::path::PathBuf;
        fn deref(&self) -> &std::path::PathBuf {
            &self.0
        }
    }

    fn tmpdir(tag: &str) -> TmpDir {
        let d = std::env::temp_dir().join(format!("md-fsread-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        TmpDir(d)
    }

    #[test]
    fn reads_a_regular_file() {
        let d = tmpdir("regular");
        let f = d.join("a.md");
        std::fs::write(&f, "# a\n").unwrap();
        assert_eq!(read_regular(&f, TEXT_MAX_BYTES).unwrap(), b"# a\n");
        assert_eq!(read_regular_string(&f, TEXT_MAX_BYTES).unwrap().as_deref(), Some("# a\n"));
    }

    #[test]
    fn refuses_files_over_the_limit() {
        let d = tmpdir("large");
        let f = d.join("big.txt");
        std::fs::write(&f, vec![b'x'; 11]).unwrap();
        assert!(matches!(read_regular(&f, 10), Err(ReadError::TooLarge(11))));
        assert_eq!(read_regular(&f, 11).unwrap().len(), 11);
    }

    #[test]
    fn refuses_directories_devices_and_fifos_without_blocking() {
        let d = tmpdir("special");
        let fifo = d.join("fifo");
        let status = std::process::Command::new("mkfifo").arg(&fifo).status().unwrap();
        assert!(status.success());
        let zero = d.join("zero.md");
        std::os::unix::fs::symlink("/dev/zero", &zero).unwrap();
        let fifo_link = d.join("fifo.md");
        std::os::unix::fs::symlink(&fifo, &fifo_link).unwrap();

        for p in [d.to_path_buf(), "/dev/zero".into(), "/dev/null".into(), zero, fifo, fifo_link] {
            // 止まる実装でも検査が返ってくるよう、別のスレッドで読んで待つ時間を区切る。
            let (tx, rx) = std::sync::mpsc::channel();
            let target = p.clone();
            std::thread::spawn(move || {
                let _ = tx.send(matches!(read_regular(&target, TEXT_MAX_BYTES), Err(ReadError::NotRegular)));
            });
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(refused) => assert!(refused, "{} を読んだ", p.display()),
                Err(_) => panic!("{} で止まった", p.display()),
            }
        }
    }

    #[test]
    fn refuses_automount_paths_by_name() {
        assert!(is_automount_path(Path::new("/net/host/share/a.md")));
        assert!(is_automount_path(Path::new("/Network/Servers/x")));
        assert!(!is_automount_path(Path::new("/network-notes/a.md")));
        assert!(!is_automount_path(Path::new("/Users/me/net/a.md")));
        assert!(is_automount_path(Path::new("/NET/host/a.md")));
        assert!(is_automount_path(Path::new("/network/Servers")));
        assert!(!is_automount_path(Path::new("net/host/a.md")));
        assert!(matches!(read_regular(Path::new("/net/x/a.md"), 10), Err(ReadError::NotRegular)));
    }

    #[test]
    fn non_utf8_is_none_not_an_error() {
        let d = tmpdir("binary");
        let f = d.join("b.bin");
        std::fs::write(&f, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(read_regular_string(&f, TEXT_MAX_BYTES).unwrap(), None);
    }
}
