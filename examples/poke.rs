//! `spike/activation` の送り側。**計測が終わったらブランチごと捨てる。**
//!
//! #31 の転送で端末から叩かれる側（窓を持たない、アクティブでもない子プロセス）の
//! 代わり。受け側を前へ出す手を 1 つずつ試して、同じログへ記録する。
//!
//! ```sh
//! MD_FOCUS_LOG=/tmp/focus.log md README.md   # 受け側を立てる
//! MD_FOCUS_LOG=/tmp/focus.log poke           # 7 本を通しで測る（引数なし = all）
//! MD_FOCUS_LOG=/tmp/focus.log poke recv-from # 1 本だけ測る
//! ```
//!
//! 通しで測るのは、成功した回の直後に打鍵が md へ入ってしまい、人が Enter で
//! 回せないため。1 本ごとに端末へフォーカスを戻して条件を揃える。
//!
//! preset は 6 つ。左が送り側の手、右が受け側の手。
//!
//! | preset      | 送り側            | 受け側          |
//! | ----------- | ----------------- | --------------- |
//! | `none`      | 何もしない        | 何もしない      |
//! | `recv`      | 何もしない        | `NSApp.activate()` |
//! | `recv-from` | 何もしない        | `activate(from: 送り側)` |
//! | `send`      | 受け側を activate | 何もしない      |
//! | `yield-recv`| 受け側へ yield    | `NSApp.activate()` |
//! | `yield-from`| 受け側へ yield    | `activate(from: 送り側)` |
//! | `dead`      | 何もせず即死      | `activate(from: 送り側)` |
//!
//! `dead` だけは送り側が poke を置いた直後に終了する。#31 の「デタッチとソケットの
//! 順序」を決めるための一本で、譲り元の pid が死んでいても通るなら、送る側は転送を
//! 待たずに返してよい。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn poke_path() -> PathBuf {
    std::env::temp_dir().join("md-focus-poke")
}

fn pid_path() -> PathBuf {
    std::env::temp_dir().join("md-focus-pid")
}

fn stamp() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let ms = d.as_millis();
    let jst = (d.as_secs() + 9 * 3600) % 86_400;
    format!("{:02}:{:02}:{:02}.{:03} {}", jst / 3600, (jst % 3600) / 60, jst % 60, ms % 1000, ms)
}

fn log(line: &str) {
    println!("{}", line);
    let Some(p) = std::env::var_os("MD_FOCUS_LOG") else { return };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(PathBuf::from(p)) {
        let _ = writeln!(f, "{} {:<4} {}", stamp(), "send", line);
    }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
}

#[cfg(target_os = "macos")]
fn state() -> String {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSWorkspace};

    let Some(mtm) = MainThreadMarker::new() else { return String::new() };
    let app = NSApplication::sharedApplication(mtm);
    let front = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|a| {
            let name = a.localizedName().map(|n| n.to_string()).unwrap_or_default();
            format!("{}:{}", a.processIdentifier(), name)
        })
        .unwrap_or_else(|| "?".to_string());
    let idle = unsafe { CGEventSourceSecondsSinceLastEventType(1, 0xFFFF_FFFF) };
    format!("送り側 active={:<5} front={:<20} idle={:.1}s", app.isActive(), front, idle)
}

/// 送り側の手を打つ。受け側が既に居ないときは false。
#[cfg(target_os = "macos")]
fn sender_act(action: &str, target_pid: i32) -> bool {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSRunningApplication};

    if action == "nothing" {
        return true;
    }
    let Some(mtm) = MainThreadMarker::new() else { return false };
    let Some(target) = NSRunningApplication::runningApplicationWithProcessIdentifier(target_pid) else {
        log(&format!("受け側 pid={} が見つからない", target_pid));
        return false;
    };
    match action {
        // 「譲る」。自分がアクティブでないなら、そもそも譲るものを持っていない。
        "yield" => NSApplication::sharedApplication(mtm).yieldActivationToApplication(&target),
        // 送り側が受け側を直接前へ出す。閉じるときの platform::activate_pid と同じ形。
        "activate-target" => {
            let me = NSRunningApplication::currentApplication();
            target.activateFromApplication_options(&me, NSApplicationActivationOptions(0));
        }
        _ => {}
    }
    true
}

#[cfg(target_os = "macos")]
fn frontmost_pid() -> Option<i32> {
    use objc2_app_kit::NSWorkspace;
    NSWorkspace::sharedWorkspace().frontmostApplication().map(|a| a.processIdentifier())
}

#[cfg(not(target_os = "macos"))]
fn frontmost_pid() -> Option<i32> {
    None
}

#[cfg(not(target_os = "macos"))]
fn state() -> String {
    String::new()
}

#[cfg(not(target_os = "macos"))]
fn sender_act(_action: &str, _target_pid: i32) -> bool {
    true
}

/// 端末へフォーカスを戻す。次の 1 本を「端末が前面」の状態から始めるための下ごしらえ。
/// 測る API と同じものを使うので、ここが効かない環境では before に active=true が
/// 残り、その回は無効と分かる。
#[cfg(target_os = "macos")]
fn restore_focus(launcher_pid: i32) {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    let Some(t) = NSRunningApplication::runningApplicationWithProcessIdentifier(launcher_pid) else {
        return;
    };
    let me = NSRunningApplication::currentApplication();
    t.activateFromApplication_options(&me, NSApplicationActivationOptions(0));
}

#[cfg(not(target_os = "macos"))]
fn restore_focus(_launcher_pid: i32) {}

const PRESETS: [&str; 8] =
    ["none", "recv", "recv-from", "send", "yield-recv", "yield-from", "dead", "send-dead"];

/// 送り側が poke を置いた直後に死ぬ preset。#31 の「送る側は転送の完了を待つのか」を
/// 決める。`dead` は受け側が譲り元を解決できずに落ちた。`send-dead` は要求を出すのが
/// 送り側自身なので、出したあとに死んでも効くかもしれない。
fn sender_dies(preset: &str) -> bool {
    preset == "dead" || preset == "send-dead"
}

fn actions(preset: &str) -> Option<(&'static str, &'static str)> {
    Some(match preset {
        "none" => ("nothing", "none"),
        "recv" => ("nothing", "activate"),
        "recv-from" => ("nothing", "activate-from"),
        "send" => ("activate-target", "none"),
        "yield-recv" => ("yield", "activate"),
        "yield-from" => ("yield", "activate-from"),
        "dead" => ("nothing", "activate-from"),
        "send-dead" => ("activate-target", "none"),
        _ => return None,
    })
}

/// 1 本測る。送り側が先に死ぬ `dead` だけは、置いたら即座に返る。
fn run_one(preset: &str, pid: i32) {
    let (send_action, recv_action) = actions(preset).expect("preset");

    log(&format!("=== preset={} 受け側 pid={} 送り側 pid={} ===", preset, pid, std::process::id()));
    // 送り側の NSWorkspace は当てにならない。run loop を回さない短命プロセスでは
    // frontmostApplication のキャッシュが更新されず、起動時点の値を握ったままになる
    // （窓が出た直後に叩くと「md が前面」と誤報する）。判定は必ず recv の before 行で。
    if frontmost_pid() == Some(pid) {
        log("（送り側からは受け側が前面に見える。キャッシュが古いだけかもしれない）");
    }
    log(&format!("before   {}", state()));

    // 送り側が手を打つ preset では、先に「何もしない」poke を投げて受け側の素の状態を
    // 撮っておく。ログは 1 本なので、2 回目の poke 行より前が before になる。
    if send_action != "nothing" {
        let _ = std::fs::write(poke_path(), format!("none {}", std::process::id()));
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    if !sender_act(send_action, pid) {
        std::process::exit(1);
    }
    log(&format!("act={:<16} {}", send_action, state()));

    if let Err(e) = std::fs::write(poke_path(), format!("{} {}", recv_action, std::process::id())) {
        eprintln!("poke を書けなかった: {}", e);
        std::process::exit(1);
    }

    // 受け側が after1500 を撮り終えるまで送り側を生かしておく。activate(from:) の
    // 譲り元として pid が生きている必要があるため（死んだ pid は解決できない）。
    // dead はその前提自体を測る一本なので、置いたら即座に返る。
    if sender_dies(preset) {
        log("送り側は poke を置いて即終了する");
        return;
    }
    std::thread::sleep(std::time::Duration::from_millis(2000));
    log(&format!("終わり   {}", state()));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let preset = args.get(1).map(String::as_str).unwrap_or("all");

    if preset != "all" && actions(preset).is_none() {
        eprintln!("poke <all|{}> [受け側の pid]", PRESETS.join("|"));
        std::process::exit(1);
    }

    // 受け側が置いた `<受け側の pid> <起動元の pid>`。
    let placed = std::fs::read_to_string(pid_path()).unwrap_or_default();
    let mut it = placed.split_whitespace();
    let placed_pid: Option<i32> = it.next().and_then(|v| v.parse().ok());
    let launcher: i32 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);

    let pid: i32 = args
        .get(2)
        .and_then(|v| v.parse().ok())
        .or(placed_pid)
        .unwrap_or_else(|| {
            eprintln!("受け側の pid が分からない（{} が無い）", pid_path().display());
            std::process::exit(1);
        });

    if preset != "all" {
        if launcher != 0 && frontmost_pid() == Some(pid) {
            restore_focus(launcher);
            std::thread::sleep(std::time::Duration::from_millis(1800));
        }
        run_one(preset, pid);
        return;
    }

    // 通しで測る。1 本ごとに端末へフォーカスを戻すので、どの回も「端末が前面」から
    // 始まる。人が Enter を押して回すと、成功した回の直後は打鍵が md に入ってしまう。
    for p in PRESETS.iter() {
        // 毎回ここから。1 本目も、窓が出た直後は窓が前面なので同じ下ごしらえが要る。
        if launcher != 0 {
            restore_focus(launcher);
            // Space の切り替えは 250〜1500ms 後に終わる。落ち着くまで待ってから次へ。
            std::thread::sleep(std::time::Duration::from_millis(1800));
            log(&format!("--- 端末({})へ戻した {}", launcher, state()));
        }

        if sender_dies(p) {
            // 送り側が先に死ぬ条件は、自分を子として起動して作る。
            let exe = std::env::current_exe().expect("current_exe");
            let _ = std::process::Command::new(exe).arg(p).arg(pid.to_string()).status();
            std::thread::sleep(std::time::Duration::from_millis(2500));
        } else {
            run_one(p, pid);
        }
    }
    log("=== 通し 終わり ===");
}
