//! WebKit の先読みを切る機能フラグ（`platform::without_prefetch`）が、この OS でまだ使えること。
//!
//! OS の更新で非公開の口の名前が変わると、先読みは黙って戻る。設定画面にも出すが、開発機で
//! `cargo test` を回した時点で気づけるようにここでも見る。名前が残ったまま効かなくなる変化は
//! 拾えないので、OS の更新ごとに `tests/manual/security/real-device/` の手順（記録用のサーバに
//! 接続が来ないこと）を手で回す。
//!
//! 既定のテストの枠（libtest）を使わないのは、各テストを別のスレッドで走らせるため。WebKit は
//! メインスレッドの外で初期化されると落ちる（`WebKit::runInitializationCode` の brk）。
//! `harness = false` なら `main` がメインスレッドで走る。

#[cfg(target_os = "macos")]
#[allow(dead_code, unused_imports)]
#[path = "../src/platform.rs"]
mod platform;

#[cfg(target_os = "macos")]
fn main() {
    // 5 つの名前が OS の一覧にあること。
    let features = platform::webkit_features().expect("+[WKPreferences _features] が無い");
    let names: Vec<String> = features.into_iter().map(|(name, _)| name).collect();
    assert_eq!(platform::missing_features(&names), Vec::<&str>::new(), "見つからない機能フラグがある");

    // 実際に切った configuration を組んでも落ちず、全部 OFF として読み戻せること。
    let (_builder, off) = platform::without_prefetch(wry::WebViewBuilder::new());
    assert_eq!(off, platform::PrefetchOff::Applied { missing: Vec::new() }, "切れなかった先読みがある");

    println!("webkit_flags: ok（{} 個の機能フラグと DNS の先読みを切れた）", platform::PREFETCH_FEATURES.len());
}

#[cfg(not(target_os = "macos"))]
fn main() {}
