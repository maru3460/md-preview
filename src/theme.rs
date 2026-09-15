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
#[derive(Clone, Copy, PartialEq, Debug)]
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
    /// 代表パレット: [bg, fg, accent, accent2, accent3]。`md theme` の色見本に出すほか、
    /// **外観を固定したテーマでは、先頭の bg が窓の下地色の出所**にもなっている
    /// （[`window_bg`]）。本文の背景と揃っていないと起動直後に色が変わって見えるので、
    /// 近い色で済ませてはいけない（`every_fixed_theme_swatch_bg_is_its_body_background`
    /// が照合している）。OS 追従テーマの下地はここではなく `AUTO_*_BG` から来る。
    ///
    /// accent は各テーマ CSS の `--md-accent` と同じ値にする（default はリンク色と
    /// 選択色が別だが、ここは他のテーマと同じく選択色の方を出す）。
    pub swatch: [&'static str; 5],
}

pub const BUILTIN: &[Theme] = &[
    Theme { name: "default", css: DEFAULT_THEME_CSS, appearance: Appearance::Auto,
        swatch: ["#ffffff", "#1f2328", "#3b82f6", "#1a7f37", "#cf222e"] },
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

fn builtin(name: &str) -> Option<&'static Theme> {
    BUILTIN.iter().find(|t| t.name == name)
}

/// 未知のテーマ名のフォールバック先。`default` が `BUILTIN` に居る前提を
/// ここ 1 箇所に閉じる。
fn default_theme() -> &'static Theme {
    builtin("default").expect("default テーマは BUILTIN に必ずある")
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

/// テーマ名を、その paint CSS・appearance・当たった同梱テーマに解決する。
/// ユーザーテーマが同梱テーマより優先され、未知の名前は警告して default テーマに
/// フォールバックする。ユーザーテーマは appearance 不明なので OS に追従（Auto）する。
/// これは default テーマをコピーしたとき（ドキュメント記載のテンプレート）と同じ挙動。
///
/// 3 つめを返すのは、窓の下地色（[`window_bg`]）が同じ解決結果から色を取るため。
/// 名前をもう一度引き直す形にすると、フォールバック後の default ではなく元の名前で
/// 引いてしまい、CSS と下地色が別のテーマを指せてしまう。
/// ユーザーテーマは `BUILTIN` に無いので None になる。
pub fn resolve(name: &str) -> (String, Appearance, Option<&'static Theme>) {
    if let Some(css) = user_css(name) {
        return (css, Appearance::Auto, None);
    }
    if let Some(t) = builtin(name) {
        return (t.css.to_string(), t.appearance, Some(t));
    }
    // この警告が届くのは `md --html` だけ。窓を開く経路では resolve が走るのは
    // デタッチ後の子で、その stderr は /dev/null（親に繋ぐと OS のログが混ざるため）。
    // 設定画面でテーマを扱えるようにするとき（#38）に、窓の中で見せる形へ移す。
    eprintln!("md: '{}' というテーマがないため 'default' を使用します", name);
    let d = default_theme();
    (d.css.to_string(), d.appearance, Some(d))
}

/// 窓の下地色の受け皿。default テーマ本文の背景と同じ値で、下のテストが実物と照合する。
///
/// 使い道は 2 つある。OS 追従テーマ（[`Appearance::Auto`]）と swatch を持たない
/// ユーザーテーマが OS 設定で選ぶときと、外観固定テーマの swatch が読めなかったときの
/// フォールバックである（[`window_bg`]）。名前は Auto 由来だが、参照元は Auto に
/// 限らない。
const AUTO_LIGHT_BG: [u8; 3] = [0xff, 0xff, 0xff];
const AUTO_DARK_BG: [u8; 3] = [0x0d, 0x11, 0x17];

/// 窓の下地色。[`resolve`] が返したテーマをそのまま渡す。
///
/// 窓は中身を待たずに出すので、webview が最初のフレームを描くまでの間はこの色の板が
/// 見える（実測で 160ms 前後）。テーマ本文の背景と揃えておかないと、中身が出た瞬間に
/// 色が変わって見える。
///
/// `theme` の None は「`BUILTIN` に無いテーマ」、つまりユーザーテーマを指す
/// （[`resolve`] がそう返す）。外観が不明なので OS 追従と同じ扱いになる。
///
/// 戻り値が None になるのは「テーマが OS 追従なのに、その OS の外観が判らない」ときだけ。
/// 呼び出し側は窓を塗らず macOS 既定の背景色に任せること。既定色は OS のライト /
/// ダークに自動で追随するので、当てずっぽうで塗るより外れが小さい（白を決め打つと、
/// ダークな OS でいちばん目立つ形で外す）。
pub fn window_bg(theme: Option<&Theme>, os_dark: Option<bool>) -> Option<[u8; 3]> {
    let pair = |dark: bool| if dark { AUTO_DARK_BG } else { AUTO_LIGHT_BG };
    match theme {
        // 外観固定のテーマは swatch の bg がそのまま本文の背景。OS 設定に依らない。
        // swatch が読めないときも OS 既定へ逃がさない。逃がすと、ダーク固定テーマが
        // ライトな OS で白い下地になる。テーマ自身が答（Light か Dark か）を持って
        // いるのに、知らない方へ聞きに行くことになる。
        Some(t) if t.appearance != Appearance::Auto => Some(
            parse_hex_rgb(t.swatch[0]).unwrap_or_else(|| pair(t.appearance == Appearance::Dark)),
        ),
        // OS 追従のテーマと、swatch を持たないユーザーテーマ。
        _ => Some(pair(os_dark?)),
    }
}

/// `#rrggbb` を 3 バイトへ。swatch は自前の定数なので、想定外の形は None で捨てて
/// 呼び出し側のフォールバックに任せる。
fn parse_hex_rgb(hex: &str) -> Option<[u8; 3]> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let mut out = [0u8; 3];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(h.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
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
    let (paint, appearance, _) = resolve(name);
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

    /// テーマ CSS の `body { ... }` ブロックが指定している背景色を全部拾う
    /// （`@media` の中のものも含む）。
    ///
    /// ファイル全体を `contains` で見てはいけない。パネルやコードブロックの地色に
    /// 同じ値が転がっているだけで通ってしまい、肝心の body を変えても落ちない
    /// テストになる（実際 default.css には `#0d1117` が body 以外にもある）。
    fn body_bgs(css: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = css;
        while let Some(i) = rest.find("body {") {
            // `.markdown-body {` や `tbody {` は別のセレクタ。直前が識別子の途中なら飛ばす。
            let is_own_selector = i == 0
                || !matches!(rest.as_bytes()[i - 1],
                    b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'#');
            let after = &rest[i + "body {".len()..];
            let Some(end) = after.find('}') else { break };
            if is_own_selector {
                for line in after[..end].lines() {
                    if let Some(v) = line.trim().strip_prefix("background-color:") {
                        out.push(v.trim().trim_end_matches(';').to_string());
                    }
                }
            }
            rest = &after[end..];
        }
        out
    }

    #[test]
    fn auto_window_bg_matches_the_default_theme_body() {
        // 窓の下地とテーマ本文がズレると、中身が出た瞬間に色が変わって見える。
        // default は OS 追従なので、body の背景もライト / ダークの 2 つ持っている。
        let bgs = body_bgs(DEFAULT_THEME_CSS);
        assert!(bgs.contains(&"#ffffff".to_string()), "light が無い: {:?}", bgs);
        assert!(bgs.contains(&"#0d1117".to_string()), "dark が無い: {:?}", bgs);
        let d = default_theme();
        assert_eq!(window_bg(Some(d), Some(false)), Some([0xff, 0xff, 0xff]));
        assert_eq!(window_bg(Some(d), Some(true)), Some([0x0d, 0x11, 0x17]));
    }

    #[test]
    fn every_fixed_theme_swatch_bg_is_its_body_background() {
        // 外観固定テーマの下地は swatch の bg から作る。ここが body の背景とズレると
        // 起動直後に色が変わって見える。
        for t in BUILTIN.iter().filter(|t| t.appearance != Appearance::Auto) {
            let bgs = body_bgs(t.css);
            assert!(
                bgs.contains(&t.swatch[0].to_string()),
                "{} の swatch bg ({}) が body の背景に無い: {:?}",
                t.name,
                t.swatch[0],
                bgs
            );
        }
    }

    #[test]
    fn only_the_default_theme_follows_the_os() {
        // window_bg の OS 追従側は default の配色（AUTO_LIGHT_BG / AUTO_DARK_BG）を
        // 決め打っている。2 つ目の Auto 同梱テーマを足すと、その窓の下地に黙って
        // default の色が使われる。増やすなら window_bg 側も直すこと。
        let autos: Vec<&str> = BUILTIN
            .iter()
            .filter(|t| t.appearance == Appearance::Auto)
            .map(|t| t.name)
            .collect();
        assert_eq!(autos, ["default"]);
    }

    #[test]
    fn body_bgs_ignores_other_selectors() {
        // ファイル全体の contains に戻さないための番人。`.markdown-body` のような
        // 別セレクタを拾い始めると、body を変えても落ちないテストになる。
        let css = ".markdown-body {\n  background-color: #111111;\n}\nbody {\n  background-color: #222222;\n}\n";
        assert_eq!(body_bgs(css), vec!["#222222".to_string()]);
    }

    #[test]
    fn broken_fixed_theme_swatch_falls_back_to_its_own_appearance() {
        // 読めない swatch を OS 既定へ逃がすと、ダーク固定テーマがライトな OS で
        // 白い下地になる。テーマ自身の外観に寄せる。
        let broken = Theme {
            name: "broken",
            css: "",
            appearance: Appearance::Dark,
            swatch: ["nope", "", "", "", ""],
        };
        assert_eq!(window_bg(Some(&broken), None), Some(AUTO_DARK_BG));
        assert_eq!(window_bg(Some(&broken), Some(false)), Some(AUTO_DARK_BG));
    }

    #[test]
    fn every_builtin_swatch_bg_parses() {
        // 読めない swatch を足しても、窓の下地が黙って OS 既定の 2 択へ落ちるだけで
        // 誰も気づかない。ここで止める。
        for t in BUILTIN {
            assert!(parse_hex_rgb(t.swatch[0]).is_some(), "{} の swatch bg が読めない", t.name);
        }
    }

    #[test]
    fn fixed_theme_window_bg_needs_no_os_setting() {
        // 外観が固定されているなら OS 設定に依らない。判定できなかった（None）
        // ときでも色が決まる。
        for os_dark in [Some(true), Some(false), None] {
            assert_eq!(window_bg(builtin("nord"), os_dark), Some([0x2e, 0x34, 0x40]));
            assert_eq!(window_bg(builtin("minimal"), os_dark), Some([0xff, 0xff, 0xff]));
        }
    }

    #[test]
    fn user_themes_follow_the_os_pair() {
        // ユーザーテーマは BUILTIN に無いので None で渡る。OS 設定で 2 択。
        assert_eq!(window_bg(None, Some(true)), Some(AUTO_DARK_BG));
        assert_eq!(window_bg(None, Some(false)), Some(AUTO_LIGHT_BG));
    }

    #[test]
    fn os_following_themes_give_up_when_the_os_is_unknown() {
        // OS の外観が読めないときに白を決め打つと、ダークな OS でいちばん目立つ形で
        // 外す。色を決めず、呼び出し側から macOS 既定の背景色へ落とす。
        assert_eq!(window_bg(builtin("default"), None), None);
        assert_eq!(window_bg(None, None), None);
    }

    #[test]
    fn unknown_theme_resolves_to_the_default_theme_itself() {
        // フォールバックした先のテーマを返さないと、CSS は default なのに下地色は
        // 元の（存在しない）名前で引くことになり、2 つが別のテーマを指せてしまう。
        let (_, appearance, theme) = resolve("nope-not-real");
        assert_eq!(theme.map(|t| t.name), Some("default"));
        assert_eq!(appearance, Appearance::Auto);
    }

    #[test]
    fn malformed_swatch_is_rejected() {
        assert_eq!(parse_hex_rgb("#0d1117"), Some([0x0d, 0x11, 0x17]));
        assert_eq!(parse_hex_rgb("0d1117"), None);
        assert_eq!(parse_hex_rgb("#0d11"), None);
        assert_eq!(parse_hex_rgb("#gggggg"), None);
        // 6 バイトちょうどのマルチバイト。長さチェックを通り抜けて get(0..2) が
        // 文字境界を割るところまで行く（スライスで切っていれば panic する経路）。
        assert_eq!("ああ".len(), 6);
        assert_eq!(parse_hex_rgb("#ああ"), None);
    }

    #[test]
    fn traversal_names_are_rejected() {
        assert!(!valid_name("../../etc/passwd"));
        assert!(!valid_name("foo/bar"));
        assert!(!valid_name("foo.bar"));
        assert!(valid_name("my-theme_2"));
    }
}
