//! 単一インスタンス化の配管（#31）。**受け側はどの経路を通っても 1 つだけ**であること、
//! 残骸から勝手に復旧すること、パスに何が入っていてもワイヤで壊れないこと。
//!
//! ここは窓を作らない層なので、`tao` も `wry` も起こさずに端から端まで叩ける
//! （窓・前面化・Space は Playwright でも触れないので、そちらは手動確認が持つ）。
//!
//! 置き場所は必ず `Endpoint::at` で明示する。`user_default()` を使うと、テストが
//! 開発機で動いている本物の md の受け側を奪う。

use std::sync::mpsc;
use std::time::Duration;

use md_preview::instance::{claim, try_send, Claim, Endpoint, Message, Sent};

/// テストごとに独立したソケットの置き場所。`$TMPDIR` 直下に掘るのは、深い階層だと
/// `sun_path`（104 バイト）に収まらなくなるため。
///
/// **`name` は全テストで一意にすること。** pid は同じバイナリ内で共通なので、名前が
/// ぶつかると 2 本が同じソケットとロックを取り合ううえ、下の `remove_file` が相手の
/// `.lock` を消す。原因が分かりにくい落ち方になる。
fn endpoint(name: &str) -> Endpoint {
    let p = std::env::temp_dir().join(format!("md-si-{}-{}.sock", name, std::process::id()));
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(p.with_extension("sock.lock"));
    Endpoint::at(p)
}

fn owner(c: Claim) -> md_preview::instance::Owner {
    match c {
        Claim::Owner(o) => o,
        Claim::Taken => panic!("受け側の座が取れなかった"),
        Claim::Failed(e) => panic!("ロックできなかった: {e}"),
    }
}

// ── ワイヤ ──────────────────────────────────────────────────

#[test]
fn encode_decode_keeps_the_file_order() {
    let mut msg = Message::new(vec!["/a.md".into(), "/b.md".into(), "/c.md".into()]);
    msg.cwd = Some("/work".into());
    msg.sender_pid = Some(4242);
    msg.own = vec!["/tmp/md-stdin-1".into()];
    assert_eq!(Message::decode(&msg.encode()), Some(msg));
}

#[test]
fn paths_with_newlines_quotes_and_equals_survive() {
    // macOS のパスに入らないのは `/` と NUL だけ。区切りに NUL を選んである理由が
    // これで、改行も引用符も `=` も素通りしなければならない。
    let weird = "/tmp/a\nb/c=d e\"f\\g/日本語 🍡.md";
    let msg = Message::new(vec![weird.to_string()]);
    assert_eq!(Message::decode(&msg.encode()).unwrap().files, vec![weird.to_string()]);
}

#[test]
fn value_side_equals_is_not_split_again() {
    let msg = Message::new(vec!["/tmp/k=v=w.md".into()]);
    assert_eq!(Message::decode(&msg.encode()).unwrap().files, vec!["/tmp/k=v=w.md".to_string()]);
}

#[test]
fn broken_input_is_rejected() {
    assert_eq!(Message::decode(b""), None, "空");
    assert_eq!(Message::decode(b"other\0" as &[u8]), None, "マジック違い");
    assert_eq!(Message::decode(b"md-preview\0x\0" as &[u8]), None, "バージョンが数値でない");
    assert_eq!(Message::decode(b"md-preview\0" as &[u8]), None, "バージョンが無い");
    // 終端の NUL が無い＝途中で切れた通信。尻尾を拾うと半端なパスをタブに載せる。
    assert_eq!(Message::decode(b"md-preview\x001\x00file=/a.md" as &[u8]), None, "終端されていない");
}

#[test]
fn a_newer_protocol_is_not_interpreted() {
    // 相手が新しいときに古い解釈を押し付けない（`cargo install` で入れ替えた直後に
    // 古い窓が生きているのは日常）。
    assert_eq!(Message::decode(b"md-preview\0999\0file=/a.md\0" as &[u8]), None);
}

#[test]
fn unknown_keys_are_ignored_and_the_rest_is_read() {
    // 前方互換の約束そのもの。これが崩れると #36 の `--notify` がキーを足せない。
    let wire = b"md-preview\x001\x00future=1\x00file=/a.md\x00\x00" as &[u8];
    let msg = Message::decode(wire).unwrap();
    assert_eq!(msg.files, vec!["/a.md".to_string()]);
}

// ── ロック ──────────────────────────────────────────────────

#[test]
fn only_one_can_claim_at_a_time() {
    let ep = endpoint("claim");
    let held = owner(claim(&ep));
    assert!(matches!(claim(&ep), Claim::Taken), "2 人目が受け側になれてしまった");
    drop(held);
}

#[test]
fn sixteen_racers_produce_exactly_one_owner() {
    let ep = endpoint("race");
    // 結果を捨てずに持ち帰る。ここで drop するとロックが即座に解放され、後続が
    // 次々と受け側になれてしまう（＝測りたいものが測れない）。
    let claims: Vec<Claim> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..16).map(|_| s.spawn(|| claim(&ep))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let won = claims.iter().filter(|c| matches!(c, Claim::Owner(_))).count();
    assert_eq!(won, 1, "同時に claim して受け側が {won} 個できた");
}

#[test]
fn the_seat_is_free_again_once_the_owner_is_gone() {
    // drop はプロセスの死と同じ意味（flock は kernel が必ず解放する）。
    // ⌘Q でもクラッシュでも残骸ロックが残らないのはこの性質による。
    let ep = endpoint("reclaim");
    drop(owner(claim(&ep)));
    assert!(matches!(claim(&ep), Claim::Owner(_)), "所有者が消えても座が空かない");
}

#[test]
fn a_leftover_socket_file_does_not_block_the_next_owner() {
    let ep = endpoint("stale-file");
    // (a) 別物の普通のファイルが置かれている
    std::fs::write(ep.socket_path(), b"leftover").unwrap();
    let first = owner(claim(&ep)).listen().expect("普通のファイルを退かせなかった");
    drop(first);

    // (b) 前の受け側が残したソケット（kill -9 / terminate の後の通常状態）
    let second = owner(claim(&ep)).listen().expect("残骸のソケットを退かせなかった");
    drop(second);
    assert!(ep.socket_path().exists(), "残骸は消さない限り残る（消すのは次の listen）");
    let third = owner(claim(&ep)).listen().expect("2 度目の残骸を退かせなかった");
    drop(third);
    let _ = std::fs::remove_file(ep.socket_path());
}

// ── 端から端まで ────────────────────────────────────────────

#[test]
fn nobody_home_is_not_an_error() {
    let ep = endpoint("empty");
    assert_eq!(try_send(&ep, &Message::new(vec!["/a.md".into()])), Sent::NoReceiver, "パスが無い");

    // listener を落とした後の残骸ソケットも「誰も居ない」。
    let listening = owner(claim(&ep)).listen().unwrap();
    drop(listening);
    assert_eq!(try_send(&ep, &Message::new(vec!["/a.md".into()])), Sent::NoReceiver, "残骸");
    let _ = std::fs::remove_file(ep.socket_path());
}

#[test]
fn a_message_reaches_the_receiver_and_the_sender_learns_its_pid() {
    let ep = endpoint("e2e");
    let (tx, rx) = mpsc::channel();
    let _handle = owner(claim(&ep)).listen().unwrap().serve(move |msg| {
        let _ = tx.send(msg);
    });

    let mut sent = Message::new(vec!["/a.md".into(), "/b.md".into()]);
    sent.own = vec!["/tmp/md-stdin-9".into()];
    match try_send(&ep, &sent) {
        Sent::Delivered { receiver_pid } => {
            assert_eq!(receiver_pid, Some(std::process::id() as i32), "挨拶で pid を渡す");
        }
        other => panic!("届かなかった: {other:?}"),
    }

    let got = rx.recv_timeout(Duration::from_secs(2)).expect("受け側に届かなかった");
    assert_eq!(got, sent);
}

#[test]
fn a_newer_receiver_gets_nothing_written_to_it() {
    // 挨拶だけを偽装した最小の受け側。要求を 1 バイトも書かないことを、相手側で確かめる。
    let ep = endpoint("newer");
    let listener = std::os::unix::net::UnixListener::bind(ep.socket_path()).unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut s, _) = listener.accept().unwrap();
        s.write_all(b"md-preview\x00999\x00pid=1\x00").unwrap();
        s.flush().unwrap();
        let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
        let mut buf = Vec::new();
        let _ = s.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });

    assert_eq!(
        try_send(&ep, &Message::new(vec!["/a.md".into()])),
        Sent::Incompatible { version: 999 }
    );
    assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap().is_empty(), "書いてしまった");
    let _ = std::fs::remove_file(ep.socket_path());
}

#[test]
fn a_silent_receiver_still_gets_the_message_but_no_activation() {
    // accept するが挨拶を返さない（生きているが詰まっている）。送るだけ送って、
    // 前面化はしない——相手が生きているかの確信が無いので。
    let ep = endpoint("silent");
    let listener = std::os::unix::net::UnixListener::bind(ep.socket_path()).unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        use std::io::Read;
        let (mut s, _) = listener.accept().unwrap();
        let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
        let mut buf = Vec::new();
        let _ = s.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });

    assert_eq!(
        try_send(&ep, &Message::new(vec!["/a.md".into()])),
        Sent::Delivered { receiver_pid: None }
    );
    let got = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(Message::decode(&got).unwrap().files, vec!["/a.md".to_string()]);
    let _ = std::fs::remove_file(ep.socket_path());
}

#[test]
fn a_stranger_on_the_socket_gets_nothing_written_to_it() {
    // MAGIC は「他人のプログラムにこちらの解釈を押し付けないための札」。読む側だけで
    // 使って書く側で無視したら意味が無いので、挨拶が md のものでなければ 1 バイトも
    // 書かずに冷スタートへ落ちる。
    let ep = endpoint("stranger");
    let listener = std::os::unix::net::UnixListener::bind(ep.socket_path()).unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        let (mut s, _) = listener.accept().unwrap();
        s.write_all(b"SSH-2.0-OpenSSH\0\0\0").unwrap();
        s.flush().unwrap();
        let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
        let mut buf = Vec::new();
        let _ = s.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });

    assert_eq!(try_send(&ep, &Message::new(vec!["/a.md".into()])), Sent::NoReceiver);
    assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap().is_empty(), "書いてしまった");
    let _ = std::fs::remove_file(ep.socket_path());
}

#[test]
fn the_cleanup_does_not_delete_someone_elses_socket() {
    // `.lock` が $TMPDIR の定期掃除で消えると二重所有が起こりうる。そのとき古い方の
    // 後始末が新しい所有者のソケットを消すと、「ロックは埋まっているのにソケットが無い」
    // 幽霊状態になり、以降すべての md が座を取れないまま 2 枚目を開き続ける。
    let ep = endpoint("unlink-guard");
    let handle = owner(claim(&ep)).listen().unwrap().serve(|_| {});
    // 別人が同じパスに bind し直した状況を作る。
    std::fs::remove_file(ep.socket_path()).unwrap();
    let _other = std::os::unix::net::UnixListener::bind(ep.socket_path()).unwrap();

    handle.unlink();

    assert!(ep.socket_path().exists(), "他人のソケットを消してしまった");
    let _ = std::fs::remove_file(ep.socket_path());
}

#[test]
fn a_path_too_long_for_sun_path_is_refused_before_binding() {
    let long = std::env::temp_dir().join("x".repeat(120));
    assert!(!Endpoint::at(long).is_bindable());
}
