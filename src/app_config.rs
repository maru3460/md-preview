//! 起動時の入力（stdin / フォルダ / cwd 内ファイル / cwd 外の単一ファイル）から、
//! ウィンドウを開くのに必要な設定一式を組み立てる。
//!
//! ここが決めるのは「どのモードで開くか」だけで、ファイルをどう描画するかは
//! [`crate::request::render_file`] に委ねる（配信経路と描画を必ず一致させるため）。

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::html::{build_folder_html, build_html, json_string, render_full_document, FOLDER_JS, INIT_JS};
use crate::request::{self, ViewMode};

/// 標準入力から読んだ内容を、自己デタッチした子へ渡すための一時ファイルのパス。
/// 子は標準入力を持たないので、親が読んでファイル経由で渡す。
pub const STDIN_FILE_ENV: &str = "MD_STDIN_FILE";

/// 単一ページ表示のウィンドウ幅。本文 720px ＋ 右の見出しナビ(TOC) が収まる。
const WIDTH_SINGLE: f64 = 1100.0;
/// フォルダ表示のウィンドウ幅。ファイルツリー(250px) ＋ 本文 ＋ 見出しナビ(TOC)。
const WIDTH_FOLDER: f64 = 1280.0;
/// ウィンドウの高さ（モード共通）。
pub const WINDOW_HEIGHT: f64 = 700.0;

/// ウィンドウ起動に必要な、入力モードごとに決まる設定一式。
pub struct AppConfig {
    pub title: String,
    pub init_script: &'static str,
    pub html_bytes: Vec<u8>,
    pub window_width: f64,
    pub root_dir: PathBuf,
    pub single_file_path: Option<PathBuf>,
    pub watch_enabled: bool,
    /// 右クリックメニューの出し分けに使う実行モード。"folder" | "single" | "stdin"。
    pub menu_mode: &'static str,
    /// 単一ファイルモードで、コメントの file:line に使う相対パス（cwd 外なので basename）。
    /// stdin モードは実体のファイルが無いので "(stdin)" とラベルする。
    /// folder / cwd モードは JS 側が現在ファイルの相対パスを持つので None。
    pub file_rel: Option<String>,
}

impl AppConfig {
    /// ページの JS から読む起動時グローバル。ウィンドウ表示では初期化スクリプト
    /// （WKUserScript）として注入される。これはページのスクリプトより先に走り、
    /// ページの CSP の対象外なので、本文の inline script が禁止されていても
    /// 各モジュールはこれらを読める。
    ///
    /// - `MD_APPEARANCE`     解決済みテーマの外観。JS で描く図（mermaid）を OS 設定では
    ///                       なくテーマに追従させる。
    /// - `MD_MENU_MODE`      右クリックメニューとヘルプの出し分けに使う実行モード。
    /// - `MD_FILE_REL`       単一ファイル/stdin モードで、コメントの file:line に使うラベル
    ///                       （単一=相対パス / stdin="(stdin)"）。folder は JS 側が現在
    ///                       ファイルを持つので空。
    /// - `MD_RENDERABLE_EXT` レンダリング対象の拡張子。定義元は `request::RENDERABLE_EXT`。
    pub fn page_globals(&self, appearance: crate::theme::Appearance) -> String {
        let renderable = request::RENDERABLE_EXT
            .iter()
            .map(|e| json_string(e))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "window.MD_APPEARANCE = {}; window.MD_MENU_MODE = {}; window.MD_FILE_REL = {}; window.MD_RENDERABLE_EXT = [{}];",
            json_string(appearance.as_str()),
            json_string(self.menu_mode),
            json_string(self.file_rel.as_deref().unwrap_or("")),
            renderable,
        )
    }

    /// ツリー付きのフォルダモード。`initial_file` は最初に開く root 相対パス。
    fn folder(
        root: PathBuf,
        title: String,
        theme_css: &str,
        custom_css: &str,
        initial_file: Option<&str>,
    ) -> Self {
        let html = build_folder_html(&title, theme_css, custom_css, initial_file);
        AppConfig {
            title,
            init_script: FOLDER_JS,
            html_bytes: html.into_bytes(),
            window_width: WIDTH_FOLDER,
            root_dir: root,
            single_file_path: None,
            watch_enabled: true,
            menu_mode: "folder",
            file_rel: None,
        }
    }

    /// 1 枚もののページ（stdin / cwd 外の単一ファイル）。
    /// 監視の有無は「実体のファイルがあるか」で決まる（stdin には無い）。
    fn single_page(
        title: String,
        html: String,
        root_dir: PathBuf,
        single_file_path: Option<PathBuf>,
        menu_mode: &'static str,
        file_rel: Option<String>,
    ) -> Self {
        AppConfig {
            title,
            init_script: INIT_JS,
            html_bytes: html.into_bytes(),
            window_width: WIDTH_SINGLE,
            root_dir,
            watch_enabled: single_file_path.is_some(),
            single_file_path,
            menu_mode,
            file_rel,
        }
    }

    /// パイプで渡された markdown を 1 枚のページとして表示する。監視は行わない。
    pub fn from_stdin(theme_css: &str, custom_css: &str, current_dir: &Option<PathBuf>) -> Self {
        let markdown = read_stdin_source();
        let root = current_dir.clone().unwrap_or_else(|| PathBuf::from("."));
        // stdin にはファイルの居場所が無いので、単独行ファイルリンクは cwd 基準で解決する。
        let html = render_full_document(&markdown, "stdin", theme_css, custom_css, Some(&root));
        // 実体のファイルが無いので、コメントの file:line にはパイプ入力と分かるラベルを使う。
        Self::single_page("stdin".to_string(), html, root, None, "stdin", Some("(stdin)".to_string()))
    }

    /// 引数で渡されたパスを解決し、フォルダ / cwd 内ファイル / 単一ファイルの
    /// いずれかに応じた設定を組み立てる。
    pub fn from_path(
        arg: &str,
        theme_css: &str,
        custom_css: &str,
        current_dir: &Option<PathBuf>,
    ) -> Self {
        let path = resolve_arg_path(arg);
        let dir_name = |p: &Path| p.file_name().and_then(|n| n.to_str()).unwrap_or(".").to_string();

        if path.is_dir() {
            // フォルダ指定: root はそのフォルダ。初期表示するファイルは無い。
            let title = dir_name(&path);
            return Self::folder(path, title, theme_css, custom_css, None);
        }

        // cwd 配下のファイル: cwd を root にしたフォルダモードで開き、その 1 枚を初期表示。
        let in_cwd = current_dir.as_ref().filter(|cwd| path.starts_with(cwd));
        if let Some(cwd) = in_cwd {
            let rel = path.strip_prefix(cwd).unwrap().to_string_lossy().into_owned();
            let title = dir_name(cwd);
            return Self::folder(cwd.clone(), title, theme_css, custom_css, Some(&rel));
        }

        // cwd 外の単一ファイル: 1 枚もの表示。親ディレクトリを root にして監視する。
        // 描画の中身（md はレンダリング / html は iframe / それ以外はソースビュー /
        // 非 UTF-8 は通知）は render_file が決める。フォルダの ?file= 経路もホット
        // リロードの ?body=1 も同じ関数を通るので、挙動は構造的に揃う。
        let title = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Markdown Preview")
            .to_string();
        // root=親ディレクトリなので、iframe の src に使う相対パスはファイル名。
        // コメントの file:line にも同じ basename を使う。
        let rel = path.file_name().and_then(|n| n.to_str()).unwrap_or(&title).to_string();
        let Some(rendered) = request::render_file(&path, &rel, ViewMode::Normal) else {
            // 読み込み自体の失敗だけ終了扱い（表示できるものは窓を出す方に倒す。
            // GUI 起動では stderr が見えず「無反応」になるため）。
            eprintln!("md: '{}' を読み込めませんでした", path.display());
            std::process::exit(1);
        };
        let html = build_html(&rendered.html, &title, theme_css, custom_css, rendered.body_class);
        let base_dir = path.parent().unwrap_or(&path).to_path_buf();
        Self::single_page(title, html, base_dir, Some(path), "single", Some(rel))
    }
}

/// 引数のパスを絶対パスへ解決する。開けないパスはここで終わる。
/// 自己デタッチの前にも通す。子は標準エラー出力を持たないので、存在しないパスの
/// エラーは親から呼び出し元へ返す必要がある。
pub fn resolve_arg_path(arg: &str) -> PathBuf {
    Path::new(arg).canonicalize().unwrap_or_else(|e| {
        eprintln!("md: '{}' を開けませんでした: {}", arg, e);
        std::process::exit(1);
    })
}

/// パイプで渡された markdown を取り出す。自己デタッチした場合は親が読み終えて
/// 一時ファイルに置いているので、そちらから読んで後片付けする。
pub fn read_stdin_source() -> String {
    if let Some(path) = std::env::var_os(STDIN_FILE_ENV) {
        let markdown = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("md: 標準入力の一時ファイルを読み込めませんでした: {}", e);
            std::process::exit(1);
        });
        let _ = std::fs::remove_file(&path);
        return markdown;
    }

    let mut markdown = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut markdown) {
        eprintln!("md: 標準入力を読み込めませんでした: {}", e);
        std::process::exit(1);
    }
    markdown
}
