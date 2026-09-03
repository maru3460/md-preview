//! GUI 非依存の中核ロジック（Markdown→HTML 変換、リクエスト処理、diff、テーマ、
//! 起動設定、端末サブコマンド）をライブラリとして公開する。バイナリ（main.rs）と
//! 統合テスト・開発用サーバ（examples/serve.rs）の両方から同じコードを叩けるようにするための
//! 薄いエントリポイント。
//! wry / tao / objc2 に触れる platform.rs と main.rs はバイナリ側に残す。

pub mod app_config;
pub mod bundle;
pub mod cli;
pub mod diff;
pub mod embed;
pub mod html;
pub mod request;
pub mod theme;
pub mod urlpath;

/// md がユーザーごとの持ち物を置く場所 `~/.config/md-preview`。テーマ設定・
/// ユーザー CSS・IME 用の flat bundle（[`bundle`]）が同居する。
///
/// 定義元をここ 1 つに寄せている。同じパスを組み立てる箇所が増えるたびに
/// 「掃除するときにどこを消せばいいか」の答えが散るため。
pub fn config_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config/md-preview"))
}

/// ユーザーの追加スタイル `~/.config/md-preview/style.css`。無ければ空。
/// base.css → テーマ → これ、の順に読み込まれる最後の層。
pub fn user_style_css() -> String {
    config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("style.css")).ok())
        .unwrap_or_default()
}
