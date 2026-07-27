//! GUI 非依存の中核ロジック（Markdown→HTML 変換、リクエスト処理、diff、テーマ）を
//! ライブラリとして公開する。バイナリ（main.rs）とファザー / 統合テストの両方から
//! 同じコードを叩けるようにするための薄いエントリポイント。
//! wry / tao / objc2 に触れる platform.rs と main.rs はバイナリ側に残す。

pub mod diff;
pub mod embed;
pub mod html;
pub mod request;
pub mod theme;
