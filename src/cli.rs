//! 端末向けのサブコマンド（`--help` / `--sample` / `md theme` / `--html` ダンプ）と、
//! 窓を開く経路の引数処理（[`split_open_flags`]）。どちらも GUI に一切依存しないので
//! ライブラリ側に置き、ユニットテストから叩けるようにしてある。
//!
//! 窓を開くかどうかで分けていないのは、`md` の引数の**意味**を決める場所を 1 つに
//! しておきたいため（散らすと `-n` のようなフラグが「どこで剥がされるか」を追えなくなる）。

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
                      （ウィンドウが開いていれば、そこにタブとして届きます）
  md <a.md> <b.md>…   複数のファイルをタブで開きます（先頭が最初に見えるタブ）
  cat file.md | md    標準入力（パイプ）からMarkdownを読みます
  md theme [<name>]   テーマ一覧を表示、または <name> に切り替えます
  md uninstall        md が置いた設定・データを消し、本体の消し方も案内します

オプション:
  --new-window, -n    既存のウィンドウへ送らず、新しいウィンドウで開きます
  --                  これ以降をフラグとして解釈しません（md -- -n.md）
  --sample            サンプルのMarkdownを標準出力に出します
  --help, -h          このヘルプを表示します
  --version, -V       バージョンを表示します";

/// 窓を開く経路のフラグ。
pub struct OpenFlags {
    /// `--new-window` / `-n`。既存の窓へ転送せず、2 枚目を開く（#31）。
    pub new_window: bool,
}

/// 窓を開く経路の引数からフラグを剥がし、残りをパスとして返す。
/// `run_terminal_command` が処理しなかったときだけここへ来るので、`theme` /
/// `uninstall` / `--html` はここには現れない。
///
/// 短縮形を置く基準は「**繰り返し叩く × 人が叩く**」の両方を満たすもの（#56）。
/// `--new-window` はこれを満たす唯一のオプションで、いま置いてある短縮形も `-n`
/// だけである。`--sample` も `uninstall --dry-run` も人が叩くが実質 1 回しか叩かない。
/// `--notify`（#36）は繰り返し叩くがエージェントが叩くので、短くしても誰も得をしない。
///
/// Why not `--dry-run` に `-n` を足す: 多くの道具（`make -n` / `rsync -n`）がそうしている
/// が、**この道具では `-n` は新しい窓に取った**。後から慣習に従って足そうとしないよう、
/// 理由ごとここに残す。`-V` が大文字なのも同じ理由で残す——`-v` を verbose に空けておく
/// 慣習に従っている。
///
/// Why not 引数処理そのものを作り直す（#56）: `run_terminal_command` の個数一致の分岐も
/// `uninstall::run` の完全一致も、ここからは独立して動いている。#31 が必要とするのは
/// 「窓を開く経路でフラグとパスを混ぜられること」だけなので、その 1 点に絞ってある。
pub fn split_open_flags(args: &[String]) -> Result<(OpenFlags, Vec<String>), String> {
    let mut flags = OpenFlags { new_window: false };
    let mut paths = Vec::new();
    let mut only_paths = false;
    for arg in args {
        if only_paths {
            paths.push(arg.clone());
            continue;
        }
        match arg.as_str() {
            // `md -- -n.md` のように、フラグに見える名前のファイルを開く逃げ道。
            "--" => only_paths = true,
            "-n" | "--new-window" => flags.new_window = true,
            // 素の `-` はフラグではない。md は標準入力をパス引数で受けないので、
            // ここを通して「開けませんでした」の普通のエラーに落とす。
            s if s.len() > 1 && s.starts_with('-') => {
                return Err(format!("md: 不明なオプション '{}'", s));
            }
            _ => paths.push(arg.clone()),
        }
    }
    Ok((flags, paths))
}

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
    let (theme_paint, appearance, _) = theme::resolve(&theme_name);
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
        for flag in ["--sample", "--help", "--version", "theme", "uninstall", "--new-window"] {
            assert!(USAGE.contains(flag), "{flag} が使い方に無い");
        }
    }

    #[test]
    fn open_flags_are_stripped_and_the_rest_stays_a_path() {
        let args = own(&["-n", "a.md", "b.md"]);
        let (flags, paths) = split_open_flags(&args).unwrap();
        assert!(flags.new_window);
        assert_eq!(paths, vec!["a.md", "b.md"]);
    }

    #[test]
    fn a_flag_after_the_paths_is_still_a_flag() {
        let (flags, paths) = split_open_flags(&own(&["a.md", "--new-window"])).unwrap();
        assert!(flags.new_window);
        assert_eq!(paths, vec!["a.md"]);
    }

    #[test]
    fn everything_after_the_terminator_is_a_path() {
        let (flags, paths) = split_open_flags(&own(&["--", "-n", "--weird.md"])).unwrap();
        assert!(!flags.new_window);
        assert_eq!(paths, vec!["-n", "--weird.md"]);
    }

    #[test]
    fn an_unknown_option_is_refused_instead_of_being_opened_as_a_file() {
        assert!(split_open_flags(&own(&["--nope", "a.md"])).is_err());
        assert!(split_open_flags(&own(&["-x"])).is_err());
    }

    #[test]
    fn a_bare_dash_is_a_path_not_a_flag() {
        // md は標準入力をパス引数で受けない。ここで弾くと「不明なオプション」になり、
        // 実際の理由（そんなファイルは無い）から遠いメッセージが出る。
        let (_, paths) = split_open_flags(&own(&["-"])).unwrap();
        assert_eq!(paths, vec!["-"]);
    }

    fn own(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
}
