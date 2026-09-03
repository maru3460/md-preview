use std::path::PathBuf;

// 同梱（公式）テーマ。バイナリに埋め込む。
const DEFAULT_THEME_CSS: &str = include_str!("assets/themes/default.css");
const MINIMAL_CSS: &str = include_str!("assets/themes/minimal.css");
const EDITORIAL_CSS: &str = include_str!("assets/themes/editorial.css");
const INK_CSS: &str = include_str!("assets/themes/ink.css");
const NORD_CSS: &str = include_str!("assets/themes/nord.css");
const PAPER_CSS: &str = include_str!("assets/themes/paper.css");
const MONO_CSS: &str = include_str!("assets/themes/mono.css");
const DRACULA_CSS: &str = include_str!("assets/themes/dracula.css");
const GRUVBOX_CSS: &str = include_str!("assets/themes/gruvbox.css");
const ROSE_PINE_CSS: &str = include_str!("assets/themes/rose-pine.css");
const SOLARIZED_LIGHT_CSS: &str = include_str!("assets/themes/solarized-light.css");
const TERMINAL_CSS: &str = include_str!("assets/themes/terminal.css");
const BLUEPRINT_CSS: &str = include_str!("assets/themes/blueprint.css");

// シンタックスハイライト用 CSS。切り替え可能なスタイル層の一部で、どちらを
// 適用するかはテーマの appearance が決めるため、html.rs ではなくここに置く。
const HLJS_LIGHT_CSS: &str = include_str!("assets/vendor/hljs-light.min.css");
const HLJS_DARK_CSS: &str = include_str!("assets/vendor/hljs-dark.min.css");

/// テーマが特定の外観に固定されるか、OS 設定に追従するか。
/// これはシンタックスハイライトの配色選択も兼ねるため、ライト固定テーマが
/// ダークモードの OS でダークなコードブロックになることはない（逆も同様）。
#[derive(Clone, Copy, PartialEq)]
pub enum Appearance {
    Light,
    Dark,
    Auto,
}

impl Appearance {
    /// `window.MD_APPEARANCE` としてページへ注入する小文字タグ。JS で描画する図
    /// （mermaid）が OS 設定ではなくテーマに合わせられるようにするためのもの。
    pub fn as_str(self) -> &'static str {
        match self {
            Appearance::Light => "light",
            Appearance::Dark => "dark",
            Appearance::Auto => "auto",
        }
    }
}

pub struct Theme {
    pub name: &'static str,
    pub css: &'static str,
    pub appearance: Appearance,
    /// `md theme` の色見本用の代表パレット: [bg, fg, accent, accent2, accent3]。
    pub swatch: [&'static str; 5],
}

pub const BUILTIN: &[Theme] = &[
    Theme { name: "default", css: DEFAULT_THEME_CSS, appearance: Appearance::Auto,
        swatch: ["#ffffff", "#1f2328", "#0969da", "#1a7f37", "#cf222e"] },
    Theme { name: "minimal", css: MINIMAL_CSS, appearance: Appearance::Light,
        swatch: ["#ffffff", "#37352f", "#2e7cd6", "#2f9e44", "#e03131"] },
    Theme { name: "editorial", css: EDITORIAL_CSS, appearance: Appearance::Light,
        swatch: ["#fffefb", "#1a1a1a", "#8a1c1c", "#3a6ea5", "#a9761f"] },
    Theme { name: "ink", css: INK_CSS, appearance: Appearance::Light,
        swatch: ["#ffffff", "#000000", "#000000", "#ebebeb", "#ffeb3b"] },
    Theme { name: "paper", css: PAPER_CSS, appearance: Appearance::Light,
        swatch: ["#faf6ee", "#33302a", "#1f6f6b", "#2a6f8c", "#9b3b2c"] },
    Theme { name: "mono", css: MONO_CSS, appearance: Appearance::Light,
        swatch: ["#f7f5ef", "#2b2a26", "#9b2c2c", "#3a5a8c", "#3a6b3a"] },
    Theme { name: "solarized-light", css: SOLARIZED_LIGHT_CSS, appearance: Appearance::Light,
        swatch: ["#fdf6e3", "#657b83", "#268bd2", "#859900", "#dc322f"] },
    Theme { name: "nord", css: NORD_CSS, appearance: Appearance::Dark,
        swatch: ["#2e3440", "#d8dee9", "#88c0d0", "#a3be8c", "#bf616a"] },
    Theme { name: "dracula", css: DRACULA_CSS, appearance: Appearance::Dark,
        swatch: ["#282a36", "#f8f8f2", "#bd93f9", "#8be9fd", "#ff79c6"] },
    Theme { name: "gruvbox", css: GRUVBOX_CSS, appearance: Appearance::Dark,
        swatch: ["#282828", "#ebdbb2", "#fe8019", "#83a598", "#b8bb26"] },
    Theme { name: "rose-pine", css: ROSE_PINE_CSS, appearance: Appearance::Dark,
        swatch: ["#191724", "#e0def4", "#c4a7e7", "#9ccfd8", "#eb6f92"] },
    Theme { name: "terminal", css: TERMINAL_CSS, appearance: Appearance::Dark,
        swatch: ["#0b0f0b", "#6ee787", "#b9f6c7", "#ffe066", "#ff6b6b"] },
    Theme { name: "blueprint", css: BLUEPRINT_CSS, appearance: Appearance::Dark,
        swatch: ["#0e2a4a", "#cfe3f5", "#6fb3e0", "#7fd7c4", "#ffffff"] },
];

/// テーマ名は単純な識別子。`[A-Za-z0-9_-]` に限定することで、パストラバーサルを
/// 構造的に不可能にする（`/`・`.`・`..` を含められない）。
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn user_themes_dir() -> Option<PathBuf> {
    crate::config_dir().map(|d| d.join("themes"))
}

fn active_theme_path() -> Option<PathBuf> {
    crate::config_dir().map(|d| d.join("active-theme"))
}

fn builtin(name: &str) -> Option<(&'static str, Appearance)> {
    BUILTIN.iter().find(|t| t.name == name).map(|t| (t.css, t.appearance))
}

/// `~/.config/md-preview/themes/<name>.css` があればユーザーテーマの CSS を返す。
fn user_css(name: &str) -> Option<String> {
    if !valid_name(name) {
        return None;
    }
    let dir = user_themes_dir()?;
    std::fs::read_to_string(dir.join(format!("{name}.css"))).ok()
}

pub fn theme_exists(name: &str) -> bool {
    user_css(name).is_some() || builtin(name).is_some()
}

/// テーマ名を、その paint CSS と appearance に解決する。ユーザーテーマが同梱テーマ
/// より優先され、未知の名前は警告して default テーマにフォールバックする。
/// ユーザーテーマは appearance 不明なので OS に追従（Auto）する。これは default
/// テーマをコピーしたとき（ドキュメント記載のテンプレート）と同じ挙動。
pub fn resolve(name: &str) -> (String, Appearance) {
    if let Some(css) = user_css(name) {
        return (css, Appearance::Auto);
    }
    if let Some((css, ap)) = builtin(name) {
        return (css.to_string(), ap);
    }
    // この警告が届くのは `md --html` だけ。窓を開く経路では resolve が走るのは
    // デタッチ後の子で、その stderr は /dev/null（親に繋ぐと OS のログが混ざるため）。
    // 設定画面でテーマを扱えるようにするとき（#38）に、窓の中で見せる形へ移す。
    eprintln!("md: '{}' というテーマがないため 'default' を使用します", name);
    (DEFAULT_THEME_CSS.to_string(), Appearance::Auto)
}

/// 切り替え可能なスタイル層の全体。appearance に合わせたシンタックスハイライト
/// CSS に続けてテーマの paint を並べる。base.css の後、ユーザーの style.css の
/// 前に読み込まれる。
pub fn style_layer(appearance: Appearance, paint: &str) -> String {
    let hljs = match appearance {
        Appearance::Light => HLJS_LIGHT_CSS.to_string(),
        Appearance::Dark => HLJS_DARK_CSS.to_string(),
        Appearance::Auto => format!(
            "{}\n@media(prefers-color-scheme:dark){{{}}}",
            HLJS_LIGHT_CSS, HLJS_DARK_CSS
        ),
    };
    format!("{}\n{}", hljs, paint)
}

#[cfg(test)]
fn resolve_style_layer(name: &str) -> String {
    let (paint, appearance) = resolve(name);
    style_layer(appearance, &paint)
}

/// 使用中のテーマ名を `~/.config/md-preview/active-theme` から読む。
/// ファイルが無い/空なら "default"（このファイルは `md theme <name>` でのみ作られる）。
pub fn read_active_name() -> String {
    active_theme_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".to_string())
}

pub fn write_active_name(name: &str) -> std::io::Result<()> {
    let dir = crate::config_dir()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME not set"))?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("active-theme"), name)
}

pub fn user_theme_names() -> Vec<String> {
    let Some(dir) = user_themes_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("css") {
                p.file_stem().and_then(|s| s.to_str()).map(str::to_string)
            } else {
                None
            }
        })
        .filter(|n| valid_name(n))
        .collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_theme_keeps_os_following_syntax_highlight() {
        // default テーマは OS に追従するので、そのスタイル層はダークモード用の
        // シンタックスハイライトの上書きを含んでいなければならない。
        assert!(resolve_style_layer("default").contains("prefers-color-scheme"));
    }

    #[test]
    fn fixed_themes_have_no_os_dependent_syntax_highlight() {
        // 外観固定のテーマ（ライト/ダーク）は prefers-color-scheme ルールを一切
        // 引き込んではいけない。さもないと本文はそのままなのにコードブロックだけが
        // OS に合わせて反転してしまう（このテストが防いでいるバグ）。
        for name in [
            "minimal", "editorial", "ink", "paper", "mono", "solarized-light",
            "nord", "dracula", "gruvbox", "rose-pine", "terminal", "blueprint",
        ] {
            assert!(
                !resolve_style_layer(name).contains("prefers-color-scheme"),
                "{name} leaked a prefers-color-scheme rule"
            );
        }
    }

    #[test]
    fn unknown_theme_falls_back_to_default() {
        // default（Auto）にフォールバックするので、再び OS 追従になる。
        assert!(resolve_style_layer("nope-not-real").contains("prefers-color-scheme"));
    }

    #[test]
    fn traversal_names_are_rejected() {
        assert!(!valid_name("../../etc/passwd"));
        assert!(!valid_name("foo/bar"));
        assert!(!valid_name("foo.bar"));
        assert!(valid_name("my-theme_2"));
    }
}
