use std::path::PathBuf;

// Bundled (official) themes — embedded in the binary.
pub const DEFAULT_THEME_CSS: &str = include_str!("themes/default.css");
const MINIMAL_CSS: &str = include_str!("themes/minimal.css");
const EDITORIAL_CSS: &str = include_str!("themes/editorial.css");
const INK_CSS: &str = include_str!("themes/ink.css");
const NORD_CSS: &str = include_str!("themes/nord.css");
const PAPER_CSS: &str = include_str!("themes/paper.css");
const MONO_CSS: &str = include_str!("themes/mono.css");
const DRACULA_CSS: &str = include_str!("themes/dracula.css");
const GRUVBOX_CSS: &str = include_str!("themes/gruvbox.css");
const ROSE_PINE_CSS: &str = include_str!("themes/rose-pine.css");
const SOLARIZED_LIGHT_CSS: &str = include_str!("themes/solarized-light.css");
const TERMINAL_CSS: &str = include_str!("themes/terminal.css");
const BLUEPRINT_CSS: &str = include_str!("themes/blueprint.css");

// Syntax-highlighting CSS. It is part of the swappable style layer: a theme's
// appearance decides which one applies, so it lives here, not in html.rs.
const HLJS_LIGHT_CSS: &str = include_str!("hljs-light.min.css");
const HLJS_DARK_CSS: &str = include_str!("hljs-dark.min.css");

/// Whether a theme is fixed to one appearance or follows the OS setting.
/// This also governs which syntax-highlight palette is used, so a fixed-light
/// theme never gets dark code blocks on a dark-mode OS (and vice versa).
#[derive(Clone, Copy, PartialEq)]
pub enum Appearance {
    Light,
    Dark,
    Auto,
}

impl Appearance {
    /// Lowercase tag injected into the page as `window.MD_APPEARANCE`, so that
    /// JS-rendered diagrams (mermaid) match the theme instead of the OS setting.
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
    /// Representative palette for the `md theme` swatch: [bg, fg, accent, accent2, accent3].
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

/// Theme names are simple identifiers. Restricting to `[A-Za-z0-9_-]` makes
/// path traversal structurally impossible (no `/`, `.`, `..`).
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn config_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config/md-preview"))
}

fn user_themes_dir() -> Option<PathBuf> {
    config_dir().map(|d| d.join("themes"))
}

fn active_theme_path() -> Option<PathBuf> {
    config_dir().map(|d| d.join("active-theme"))
}

fn builtin(name: &str) -> Option<(&'static str, Appearance)> {
    BUILTIN.iter().find(|t| t.name == name).map(|t| (t.css, t.appearance))
}

/// User theme CSS, if `~/.config/md-preview/themes/<name>.css` exists.
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

/// Resolve a theme name to its paint CSS plus appearance. User themes win over
/// bundled ones; an unknown name warns and falls back to the default theme.
/// User themes have unknown appearance, so they follow the OS (Auto) — the same
/// behaviour as copying the default theme, which is the documented template.
pub fn resolve(name: &str) -> (String, Appearance) {
    if let Some(css) = user_css(name) {
        return (css, Appearance::Auto);
    }
    if let Some((css, ap)) = builtin(name) {
        return (css.to_string(), ap);
    }
    eprintln!("md: unknown theme '{}', falling back to 'default'", name);
    (DEFAULT_THEME_CSS.to_string(), Appearance::Auto)
}

/// The full swappable style layer: the appearance-matched syntax highlighting
/// CSS followed by the theme's paint. Loaded after base.css and before the
/// user's custom style.css.
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

/// The active theme name, from `~/.config/md-preview/active-theme`.
/// Missing/empty file → "default" (the file is only created by `md theme <name>`).
pub fn read_active_name() -> String {
    active_theme_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".to_string())
}

pub fn write_active_name(name: &str) -> std::io::Result<()> {
    let dir = config_dir()
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
        // The default theme follows the OS, so its style layer must carry the
        // dark-mode syntax-highlight override.
        assert!(resolve_style_layer("default").contains("prefers-color-scheme"));
    }

    #[test]
    fn fixed_themes_have_no_os_dependent_syntax_highlight() {
        // A fixed-appearance theme (light or dark) must NOT pull in any
        // prefers-color-scheme rule, or code blocks would flip with the OS
        // while the body stays put (the bug this guards against).
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
        // Falls back to default (Auto), so it is OS-following again.
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
