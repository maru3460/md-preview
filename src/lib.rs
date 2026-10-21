//! GUI 非依存の中核ロジック（Markdown→HTML 変換、リクエスト処理、diff、テーマ、
//! 起動設定、端末サブコマンド）をライブラリとして公開する。バイナリ（main.rs）と
//! 統合テスト・開発用サーバ（examples/serve.rs）の両方から同じコードを叩けるようにするための
//! 薄いエントリポイント。
//! wry / tao / objc2 に触れる platform.rs と main.rs はバイナリ側に残す。

pub mod app_config;
pub mod bundle;
pub mod cli;
pub mod csp;
pub mod diff;
pub mod embed;
pub mod extmedia;
pub mod fsread;
pub mod git;
pub mod html;
pub mod instance;
pub mod navigation;
pub mod notifications;
pub mod origin;
pub mod quick_access;
pub mod request;
pub mod settings;
pub mod store;
pub mod theme;
pub mod uninstall;
pub mod urlpath;

/// md がユーザーごとの持ち物を置く場所 `~/.config/md-preview`。テーマ設定・
/// ユーザー CSS・IME 用の flat bundle（[`bundle`]）が同居する。
///
/// 定義元をここ 1 つに寄せている。同じパスを組み立てる箇所が増えるたびに
/// 「掃除するときにどこを消せばいいか」の答えが散るため。
///
/// 環境変数 `MD_CONFIG_DIR` があれば、そちらを丸ごと使う。検証用のビルドで設定（文書の
/// JavaScript を動かす、など）を ON にしても、日常の md の設定まで ON にしないため
/// （`MD_SOCKET` でソケットを分けるのと同じ考え方）。
///
/// これだけではソケットは分かれない。ソケットは `$TMPDIR` に置き、`config_dir` の下
/// （`run/`）へ落ちるのは `$TMPDIR` が使えないときだけなので（`instance.rs`）、検証用の
/// ビルドには `MD_SOCKET` も要る。
///
/// Why not 相対パスを読まずに `~/.config` へ戻す: 分けたつもりの設定が、黙って日常の md の
/// 設定に混ざる。分ける口で起きてはいけない向きなので、作業ディレクトリから読む。
pub fn config_dir() -> Option<std::path::PathBuf> {
    pick_config_dir(
        std::env::var_os(CONFIG_DIR_ENV),
        std::env::var_os("HOME"),
        || std::env::current_dir().ok(),
    )
}

/// [`config_dir`] を差し替える環境変数。
pub const CONFIG_DIR_ENV: &str = "MD_CONFIG_DIR";

/// [`config_dir`] の中身。環境変数を引数で受けるのは、テストでプロセスの環境を書き換えずに
/// 済ませるため（書き換えると、並んで走る他のテストの設定の置き場所まで動く）。
fn pick_config_dir(
    over: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    cwd: impl FnOnce() -> Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    if let Some(dir) = over.filter(|v| !v.is_empty()) {
        let dir = std::path::PathBuf::from(dir);
        if dir.is_absolute() {
            return Some(dir);
        }
        return cwd().map(|cwd| cwd.join(dir));
    }
    home.map(|h| std::path::PathBuf::from(h).join(".config/md-preview"))
}

/// ユーザーの追加スタイル `~/.config/md-preview/style.css`。無ければ空。
/// base.css → テーマ → これ、の順に読み込まれる最後の層。
pub fn user_style_css() -> String {
    config_dir()
        // fsread を通さない: 設定ディレクトリの利用者の CSS
        .and_then(|d| std::fs::read_to_string(d.join("style.css")).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::pick_config_dir;
    use std::path::PathBuf;

    fn pick(over: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
        pick_config_dir(over.map(Into::into), home.map(Into::into), || Some(PathBuf::from("/work")))
    }

    #[test]
    fn the_config_dir_is_under_home_by_default() {
        assert_eq!(pick(None, Some("/Users/me")), Some(PathBuf::from("/Users/me/.config/md-preview")));
        assert_eq!(pick(None, None), None);
        // 空は「書いていない」と同じ。
        assert_eq!(pick(Some(""), Some("/Users/me")), Some(PathBuf::from("/Users/me/.config/md-preview")));
    }

    /// 差し替えたときは、HOME の下の日常の設定を一切見ないこと。
    #[test]
    fn md_config_dir_replaces_the_whole_config_dir() {
        assert_eq!(pick(Some("/tmp/md-dev"), Some("/Users/me")), Some(PathBuf::from("/tmp/md-dev")));
        assert_eq!(pick(Some("/tmp/md-dev"), None), Some(PathBuf::from("/tmp/md-dev")));
        // 相対パスは作業ディレクトリから読む（HOME の下へは戻らない）。
        assert_eq!(pick(Some("dev-config"), Some("/Users/me")), Some(PathBuf::from("/work/dev-config")));
    }
}
