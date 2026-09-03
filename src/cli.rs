//! ウィンドウを開かない、端末向けのサブコマンド（`--help` / `--sample` /
//! `md theme` / `--html` ダンプ）。GUI に一切依存しないのでライブラリ側に置き、
//! ユニットテストから叩けるようにしてある。

use std::io::IsTerminal;
use std::path::Path;

use crate::html::build_html;
use crate::request::{self, ViewMode};
use crate::theme;

pub const SAMPLE_MD: &str = include_str!("assets/sample.md");

/// `--help` とエラー時のどちらでも使い回す使い方テキスト。
pub const USAGE: &str = "\
md - 高速Markdownプレビュー

使い方:
  md <file.md|dir>    ファイルかディレクトリをプレビュー表示します
  md <a.md> <b.md>…   複数のファイルをタブで開きます（先頭が最初に見えるタブ）
  cat file.md | md    標準入力（パイプ）からMarkdownを読みます
  md theme [<name>]   テーマ一覧を表示、または <name> に切り替えます
  md uninstall        md が置いた設定・データを消します（本体は cargo に任せます）
  md --sample         サンプルのMarkdownを標準出力に出します
  md --help, -h       このヘルプを表示します
  md --version, -V    バージョンを表示します

オプション:
  --detach            ウィンドウを開いたら即座にコマンドを終了します
  --no-detach         ウィンドウを閉じるまでコマンドを終了しません
                      （既定: 端末から起動したときは --no-detach、
                       それ以外は --detach）";

fn hex_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()?;
    let g = u8::from_str_radix(&h[2..4], 16).ok()?;
    let b = u8::from_str_radix(&h[4..6], 16).ok()?;
    Some((r, g, b))
}

/// テーマのパレットを見せる、truecolor ブロックを隙間なく並べた帯。
fn swatch_strip(hexes: &[&str]) -> String {
    let mut s = String::new();
    for hex in hexes {
        if let Some((r, g, b)) = hex_rgb(hex) {
            s.push_str(&format!("\x1b[48;2;{};{};{}m  \x1b[0m", r, g, b));
        }
    }
    s
}

/// テーマをグループ分けして一覧表示する。TTY ではテーマごとの色見本と、使用中の
/// ものにアクセント色のドットを付ける。パイプ時は grep しやすいよう素の名前だけ。
pub fn theme_list_text(active: &str, rich: bool) -> String {
    use theme::Appearance::{Auto, Dark, Light};
    let user = theme::user_theme_names();
    let mut s = String::new();

    if rich {
        s.push_str(&format!("\n  \x1b[1mテーマ\x1b[0m  \x1b[2m· 使用中: {}\x1b[0m\n", active));
    } else {
        s.push_str(&format!("テーマ（使用中: {}）\n", active));
    }

    let group = |s: &mut String, label: &str, names: Vec<&theme::Theme>| {
        if names.is_empty() {
            return;
        }
        if rich {
            s.push_str(&format!("\n  \x1b[1;2m{}\x1b[0m\n", label));
        } else {
            s.push_str(&format!("\n{}\n", label));
        }
        for t in names {
            let is_active = t.name == active;
            let overridden = user.iter().any(|u| u == t.name);
            if rich {
                let marker = if is_active {
                    let (r, g, b) = hex_rgb(t.swatch[2]).unwrap_or((255, 255, 255));
                    format!("\x1b[38;2;{};{};{}m●\x1b[0m", r, g, b)
                } else {
                    " ".to_string()
                };
                let pad = " ".repeat(16usize.saturating_sub(t.name.chars().count()));
                let name = if is_active { format!("\x1b[1m{}\x1b[0m", t.name) } else { t.name.to_string() };
                let over = if overridden { "  \x1b[2m（ユーザー定義で上書き）\x1b[0m" } else { "" };
                s.push_str(&format!("  {} {}{}  {}{}\n", marker, name, pad, swatch_strip(&t.swatch), over));
            } else {
                let marker = if is_active { "*" } else { " " };
                let over = if overridden { "  （ユーザー定義で上書き）" } else { "" };
                s.push_str(&format!("  {} {}{}\n", marker, t.name, over));
            }
        }
    };

    group(&mut s, "ライト", theme::BUILTIN.iter().filter(|t| t.appearance == Light).collect());
    group(&mut s, "ダーク", theme::BUILTIN.iter().filter(|t| t.appearance == Dark).collect());
    group(&mut s, "auto · OS設定に追従", theme::BUILTIN.iter().filter(|t| t.appearance == Auto).collect());

    let user_only: Vec<&String> = user
        .iter()
        .filter(|u| !theme::BUILTIN.iter().any(|t| t.name == u.as_str()))
        .collect();
    if !user_only.is_empty() {
        let header = if rich { "\n  \x1b[1;2mユーザー\x1b[0m\n" } else { "\nユーザー\n" };
        s.push_str(header);
        for name in user_only {
            let marker = if rich {
                if name.as_str() == active { "\x1b[1m●\x1b[0m" } else { " " }
            } else if name.as_str() == active {
                "*"
            } else {
                " "
            };
            s.push_str(&format!("  {} {}\n", marker, name));
        }
    }
    s
}

pub fn run_theme_command(rest: &[String]) {
    match rest {
        [] => {
            let rich = std::io::stdout().is_terminal();
            print!("{}", theme_list_text(&theme::read_active_name(), rich));
        }
        [name] => {
            if !theme::theme_exists(name) {
                eprintln!("md: '{}' というテーマはありません", name);
                eprint!("{}", theme_list_text(&theme::read_active_name(), std::io::stderr().is_terminal()));
                std::process::exit(2);
            }
            if let Err(e) = theme::write_active_name(name) {
                eprintln!("md: テーマを保存できませんでした: {}", e);
                std::process::exit(1);
            }
            println!("テーマを '{}' に切り替えました", name);
        }
        _ => {
            eprintln!("使い方: md theme [<name>]");
            std::process::exit(1);
        }
    }
}

/// `md --html <file> [theme]` — ウィンドウを開かず、完全に描画したページを stdout へ
/// 出力する。ライブプレビューと同じ `render_file` / `build_html` を通るので、
/// 出力は WebView の表示に忠実。
pub fn run_html_dump(arg: &str, theme_override: Option<&str>) {
    let path = Path::new(arg);
    let title = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Markdown Preview")
        .to_string();

    let custom_css = crate::user_style_css();
    let theme_name = theme_override
        .map(String::from)
        .unwrap_or_else(theme::read_active_name);
    let (theme_paint, appearance) = theme::resolve(&theme_name);
    let theme_css = theme::style_layer(appearance, &theme_paint);

    // 単体のファイルなので root はその親ディレクトリ。相対 src / href は
    // ライブプレビューでそのファイルを開いたときと同じ URL に畳まれる。
    let root = path.parent().unwrap_or(Path::new("."));
    let Some(rendered) = request::render_file(path, root, ViewMode::Normal) else {
        eprintln!("md: '{}' を読み込めませんでした", arg);
        std::process::exit(1);
    };
    print!(
        "{}",
        build_html(&rendered.html, &title, &theme_css, &custom_css, rendered.body_class)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_list_marks_the_active_theme_in_both_styles() {
        // 素の一覧（パイプ時）は grep しやすいプレーンテキスト。
        let plain = theme_list_text("nord", false);
        assert!(plain.contains("使用中: nord"), "{plain}");
        assert!(plain.contains("* nord"), "{plain}");
        assert!(!plain.contains('\x1b'), "パイプ時に ANSI が混ざっている: {plain:?}");

        // 装飾つき（TTY 時）は色見本とドットが入る。
        let rich = theme_list_text("nord", true);
        assert!(rich.contains('\x1b'), "装飾が無い: {rich:?}");
        assert!(rich.contains("●"), "使用中マーカーが無い: {rich:?}");
    }

    #[test]
    fn theme_list_covers_every_builtin_theme() {
        // 一覧から漏れるテーマが無いこと（グループ分けの条件漏れ検出）。
        let plain = theme_list_text("default", false);
        for t in theme::BUILTIN {
            assert!(plain.contains(t.name), "{} が一覧に無い", t.name);
        }
    }

    #[test]
    fn usage_lists_every_subcommand() {
        for flag in ["--sample", "--help", "--version", "theme", "uninstall", "--detach", "--no-detach"] {
            assert!(USAGE.contains(flag), "{flag} が使い方に無い");
        }
    }
}
