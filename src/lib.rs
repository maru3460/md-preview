//! GUI 非依存の中核ロジック（Markdown→HTML 変換、リクエスト処理、diff、テーマ、
//! 起動設定、端末サブコマンド）をライブラリとして公開する。バイナリ（main.rs）と
//! 統合テスト・開発用サーバ（examples/serve.rs）の両方から同じコードを叩けるようにするための
//! 薄いエントリポイント。
//! wry / tao / objc2 に触れる platform.rs と main.rs はバイナリ側に残す。

pub mod app_config;
pub mod cli;
pub mod diff;
pub mod embed;
pub mod html;
pub mod request;
pub mod theme;

/// ユーザーの追加スタイル `~/.config/md-preview/style.css`。無ければ空。
/// base.css → テーマ → これ、の順に読み込まれる最後の層。
pub fn user_style_css() -> String {
    std::env::var("HOME")
        .ok()
        .and_then(|home| std::fs::read_to_string(format!("{}/.config/md-preview/style.css", home)).ok())
        .unwrap_or_default()
}
