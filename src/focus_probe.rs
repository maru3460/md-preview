//! `spike/activation` 専用の一時パッチ。**計測が終わったらブランチごと捨てる。**
//!
//! 測るのは 1 点だけ。「背景に居る受け側を、非アクティブな CLI の子プロセスからの
//! 要求で前へ出せるか」。#31 の転送は窓を新しく作らないので、いま前面化を担って
//! いる「窓を作った約 30ms 後に OS が暗黙にくれるアクティベーション」が発生しない。
//!
//! 窓を持つプロセスの出力はデタッチすると /dev/null へ行くので、記録はファイルへ。
//! `MD_FOCUS_LOG=<path>` が在るときだけ全部が有効になる。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// 送り側が「起こしてくれ」と置くファイル。中身は `<受け側の動作> <送り側の pid>`。
/// ソケットの代わり。#31 の配管を先に作らずに前面化だけを測るための足場。
pub fn poke_path() -> PathBuf {
    std::env::temp_dir().join("md-focus-poke")
}

/// 受け側が `<自分の pid> <起動元の pid>` を置く場所。送り側が pid 引数を省けるのと、
/// 測り直す前に端末へフォーカスを戻すために起動元が要る。
pub fn pid_path() -> PathBuf {
    std::env::temp_dir().join("md-focus-pid")
}

pub fn log_path() -> Option<PathBuf> {
    std::env::var_os("MD_FOCUS_LOG").map(PathBuf::from)
}

pub fn enabled() -> bool {
    log_path().is_some()
}

/// 送り側と受け側の行を突き合わせるための時刻。JST の壁時計とエポック ms を両方出す。
pub fn stamp() -> String {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let ms = d.as_millis();
    let jst = (d.as_secs() + 9 * 3600) % 86_400;
    format!("{:02}:{:02}:{:02}.{:03} {}", jst / 3600, (jst % 3600) / 60, jst % 60, ms % 1000, ms)
}

pub fn log(side: &str, line: &str) {
    let Some(p) = log_path() else { return };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p) {
        let _ = writeln!(f, "{} {:<4} {}", stamp(), side, line);
    }
}

// 入力からの経過秒。エージェントの bash から叩いた回は「人が触っていない」対照
// 条件になってしまうので、人が打った回かどうかをログ自身に持たせる。
#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
}

#[cfg(target_os = "macos")]
pub fn idle_seconds() -> f64 {
    // 1 = kCGEventSourceStateHIDSystemState, 0xFFFFFFFF = kCGAnyInputEventType
    unsafe { CGEventSourceSecondsSinceLastEventType(1, 0xFFFF_FFFF) }
}

/// いまの前面化まわりの状態。メインスレッドから呼ぶこと。
#[cfg(target_os = "macos")]
pub fn state() -> String {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSWorkspace};

    let Some(mtm) = MainThreadMarker::new() else {
        return "（メインスレッドではない）".to_string();
    };
    let app = NSApplication::sharedApplication(mtm);
    let active = app.isActive();
    let key = app.keyWindow().is_some();

    let (visible, on_space) = match app.windows().firstObject() {
        Some(w) => (w.isVisible(), w.isOnActiveSpace()),
        None => (false, false),
    };

    let front = NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|a| {
            let name = a.localizedName().map(|n| n.to_string()).unwrap_or_default();
            format!("{}:{}", a.processIdentifier(), name)
        })
        .unwrap_or_else(|| "?".to_string());

    format!(
        "active={:<5} key={:<5} visible={:<5} onActiveSpace={:<5} front={:<20} idle={:.1}s",
        active, key, visible, on_space, front, idle_seconds()
    )
}

/// 受け側の動作。`none` は「何もしないと本当に前へ出ないのか」のベースライン。
#[cfg(target_os = "macos")]
pub fn act(action: &str, sender_pid: i32) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSRunningApplication};

    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);

    match action {
        // 自分で前へ出る。macOS 14+ の非 deprecated な activate()。
        "activate" => app.activate(),
        // 送り側を「譲り元」として指名して前へ出る。閉じるときの activate_pid の裏返し。
        "activate-from" => {
            let Some(sender) = NSRunningApplication::runningApplicationWithProcessIdentifier(sender_pid) else {
                log("recv", "activate-from: 送り側の pid が既に居ない");
                return;
            };
            NSRunningApplication::currentApplication()
                .activateFromApplication_options(&sender, NSApplicationActivationOptions(0));
        }
        _ => {}
    }
}

/// 起こしてくれの合図を待つ。ソケットが無いのでファイルの出現を見る。
pub fn spawn_poller(on: impl Fn(String, i32) + Send + 'static) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(60));
        let p = poke_path();
        let Ok(s) = std::fs::read_to_string(&p) else { continue };
        let _ = std::fs::remove_file(&p);
        let mut it = s.split_whitespace();
        let action = it.next().unwrap_or("none").to_string();
        let pid = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        on(action, pid);
    });
}

/// 受け側が立ち上がったことと、自分と起動元の pid を知らせる。
pub fn announce(launcher_pid: Option<i32>) {
    let pid = std::process::id();
    let launcher = launcher_pid.unwrap_or(0);
    let _ = std::fs::write(pid_path(), format!("{} {}", pid, launcher));
    let _ = std::fs::remove_file(poke_path());
    log("recv", &format!("--- 受け側 起動 pid={} 起動元 pid={} ---", pid, launcher));
}

#[cfg(not(target_os = "macos"))]
pub fn state() -> String {
    String::new()
}

#[cfg(not(target_os = "macos"))]
pub fn act(_action: &str, _sender_pid: i32) {}
