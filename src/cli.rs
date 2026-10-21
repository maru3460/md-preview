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
  md <file.md>        ファイルをプレビュー表示します
                      （ウィンドウが開いていれば、そこにタブとして届きます）
  md <dir>            ディレクトリを開きます
                      （ウィンドウが開いていれば、そのサイドバーが移ります）
  md <a.md> <b.md>…   複数のファイルをタブで開きます（先頭が最初に見えるタブ）
  cat file.md | md    標準入力（パイプ）からMarkdownを読みます
  md theme [<name>]   テーマ一覧を表示、または <name> に切り替えます
  md uninstall        md が置いた設定・データを消し、本体の消し方も案内します

オプション:
  --new-window, -n    既存のウィンドウへ送らず、新しいウィンドウで開きます
  --notify <file>…    開かずにベルへ積みます（ウィンドウは前に出ません）
                      ウィンドウが無ければ、普通に開きます
                      パイプ入力も積めますが、md を終了するか、ベルから
                      開いたタブを閉じると開けなくなります
  --title <名前>      パイプ入力のタブ名と、ベルの項目名にします
  --description <文>  ベルの項目に添える 1 行です（ベルにだけ出ます）
  --note <文>         タブの下の帯に出す補足です
                      この 3 つが効くのはパイプ入力か --notify のときだけです
  --                  これ以降をフラグとして解釈しません（md -- -n.md）
  --sample            サンプルのMarkdownを標準出力に出します
  --help, -h          このヘルプを表示します
  --version, -V       バージョンを表示します";

/// 窓を開く経路のフラグ。
pub struct OpenFlags {
    /// `--new-window` / `-n`。既存の窓へ転送せず、2 枚目を開く（#31）。
    pub new_window: bool,
    /// `--notify`。開かずにベルへ積む（#36）。窓を持っている md が居なければ
    /// 普通に開くので、「窓を開く経路のフラグ」のままここに居る。
    ///
    /// ⚠️ `MD_NO_IPC=1` を併せると転送そのものが切れるので、**窓があっても
    /// 2 枚目が開いて前に出る**（積まれない）。`-n` との併用は下で断っているが、
    /// あちらは逃げ道（前景デバッグ）なので断らない——切るために在るものを
    /// 「使い方が矛盾している」と拒むと、逃げ道が逃げ道でなくなる。
    pub notify: bool,
    /// `--title` / `--description` / `--note`（#68）。パイプ入力と `--notify` のときだけ
    /// 意味を持つ。それ以外では [`OpenFlags::drop_origin_unless_shown`] で捨てる。
    pub origin: crate::origin::Origin,
}

impl OpenFlags {
    /// 名乗りを出す場所が無い開き方（実ファイルを `--notify` 無しで開く）なら、名乗りを
    /// 捨てる。パイプ入力かどうかは標準入力を見ないと決まらないので、呼ぶのは `main.rs`。
    ///
    /// Why not 断る: 名乗りはおまけなので、それで本体が開かないのは逆。断ると、エージェントは
    /// `cat a.md | md --title …` へ逃げやすく、そうなると保存しても再読み込みされない。
    /// Why not 受けてタブ名にする: ファイルは自分の名前を持っている。タブ名だけ差し替えても、
    /// ツリーから開き直せば元の名前に戻る（#68 の「やらないこと」）。
    pub fn drop_origin_unless_shown(&mut self, stdin_mode: bool) {
        if !stdin_mode && !self.notify {
            self.origin = Default::default();
        }
    }
}

/// 窓を開く経路の引数からフラグを剥がし、残りをパスとして返す。
/// `run_terminal_command` が処理しなかったときだけここへ来る。`theme` と `uninstall` は
/// 必ずあちらが持っていくので現れない。`--html` は `debug_assertions` ゲートなので、
/// **リリースビルドではここへ落ちてくる**（`md: 不明なオプション '--html'`）。
/// 個数が合わない `md --help extra` のようなものも同じ。
///
/// 短縮形を置く基準は「**繰り返し叩く × 人が叩く**」の両方を満たすもの（#56）。
/// `--new-window` はこれを満たす唯一のオプションで、いま置いてある短縮形も `-n`
/// だけである。`--sample` も `uninstall --dry-run` も人が叩くが実質 1 回しか叩かない。
/// `--notify`（#36）は繰り返し叩くがエージェントが叩くので、短くしても誰も得をしない
/// （足していない理由をここに残す。後から慣習で `-N` を生やさないため）。
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
    let mut flags = OpenFlags { new_window: false, notify: false, origin: Default::default() };
    let mut paths = Vec::new();
    let mut only_paths = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if only_paths {
            paths.push(arg.clone());
            continue;
        }
        // 値を取るフラグは `--title x` と `--title=x` のどちらでも受ける。
        // 前者の値は `-` で始まっていても値として取る（`--title -draft-` を断らない）。
        if let Some((name, inline)) = value_flag(arg) {
            let value = match inline {
                Some(v) => v.to_string(),
                None => args
                    .next()
                    .ok_or_else(|| format!("md: {} に値がありません", name))?
                    .clone(),
            };
            // 空の値は「名乗らなかった」として通す。`--note "$NOTE"` の変数が空だった
            // だけで文書ごと届かなくなるのは、おまけのために本体を落とすことになる。
            // 同じフラグが 2 回あれば後ろが勝つ（空の後ろも、前の値を消す）。
            let v = crate::origin::clean(&value);
            let o = &mut flags.origin;
            match name {
                "--title" => o.title = v,
                "--description" => o.description = v,
                _ => o.note = v,
            }
            continue;
        }
        match arg.as_str() {
            // `md -- -n.md` のように、フラグに見える名前のファイルを開く逃げ道。
            "--" => only_paths = true,
            "-n" | "--new-window" => flags.new_window = true,
            "--notify" => flags.notify = true,
            // 素の `-` はフラグではない。md は標準入力をパス引数で受けないので、
            // ここを通して「開けませんでした」の普通のエラーに落とす。
            s if s.len() > 1 && s.starts_with('-') => {
                return Err(format!("md: 不明なオプション '{}'", s));
            }
            _ => paths.push(arg.clone()),
        }
    }
    // 「窓を開くな」と「新しい窓を開け」が同時に立っている。黙って片方を勝たせると、
    // 負けた方の意図（`--notify` なら「邪魔したくない」）が何の痕跡も残さずに消える。
    if flags.notify && flags.new_window {
        return Err("md: --notify と --new-window は一緒に使えません".to_string());
    }
    Ok((flags, paths))
}

/// 値を取るフラグ（#68）なら、その名前と `=` の後ろ（あれば）を返す。
fn value_flag(arg: &str) -> Option<(&'static str, Option<&str>)> {
    for name in ["--title", "--description", "--note"] {
        if arg == name {
            return Some((name, None));
        }
        if let Some(v) = arg.strip_prefix(name).and_then(|r| r.strip_prefix('=')) {
            return Some((name, Some(v)));
        }
    }
    None
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
            // 動いている md の窓にも効かせる（#38）。窓はプロセスごと ⌘Q まで生き続ける
            // （#49）ので、知らせないと次に起動し直すまで古いテーマのまま残る。
            // 届かなくても（誰も居ない）保存は済んでいるので、黙って続ける。
            //
            // ⚠️ `theme=` を知らない古い md が相手の穴は、`main.rs` の
            // `message_to_forward` に並べてある（⌘Q すれば消える）。
            if let Some(ep) = crate::instance::Endpoint::user_default() {
                let _ = crate::instance::try_send(&ep, &crate::instance::Message::theme_changed());
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
    let Some(rendered) = request::render_file(path, root, ViewMode::Normal, crate::csp::Policy::default()) else {
        eprintln!("md: '{}' を読み込めませんでした", arg);
        std::process::exit(1);
    };
    print!(
        "{}",
        // ダンプは設定を読まず、既定の CSP（両方 OFF）で描く。
        build_html(
            &rendered.html,
            &title,
            &theme_css,
            &custom_css,
            rendered.body_class,
            &crate::csp::Policy::default(),
            crate::csp::APP_ORIGIN,
        )
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
        for flag in [
            "--sample", "--help", "--version", "theme", "uninstall", "--new-window", "--notify", "--title",
            "--description", "--note",
        ] {
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
    fn notify_is_a_flag_and_the_rest_stays_a_path() {
        let (flags, paths) = split_open_flags(&own(&["--notify", "a.md", "b.md"])).unwrap();
        assert!(flags.notify);
        assert!(!flags.new_window);
        assert_eq!(paths, vec!["a.md", "b.md"]);
    }

    #[test]
    fn notify_and_new_window_together_are_refused() {
        // 片方が黙って勝つのを避ける。順序は関係しない。
        assert!(split_open_flags(&own(&["--notify", "-n", "a.md"])).is_err());
        assert!(split_open_flags(&own(&["-n", "--notify", "a.md"])).is_err());
    }

    #[test]
    fn a_file_called_notify_is_reachable_through_the_terminator() {
        let (flags, paths) = split_open_flags(&own(&["--", "--notify"])).unwrap();
        assert!(!flags.notify);
        assert_eq!(paths, vec!["--notify"]);
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

    #[test]
    fn origin_flags_take_a_value_in_either_spelling() {
        let args = own(&["--title", "PR #12", "--description=md-preview: 差分", "--note", "a=b"]);
        let (flags, paths) = split_open_flags(&args).unwrap();
        assert!(paths.is_empty());
        assert_eq!(flags.origin.title.as_deref(), Some("PR #12"));
        assert_eq!(flags.origin.description.as_deref(), Some("md-preview: 差分"));
        assert_eq!(flags.origin.note.as_deref(), Some("a=b"));
    }

    #[test]
    fn the_value_of_an_origin_flag_is_never_read_as_a_flag_or_a_path() {
        let (flags, paths) = split_open_flags(&own(&["--title", "-n", "a.md"])).unwrap();
        assert!(!flags.new_window);
        assert_eq!(flags.origin.title.as_deref(), Some("-n"));
        assert_eq!(paths, vec!["a.md"]);
    }

    #[test]
    fn an_origin_flag_without_a_value_is_refused() {
        assert!(split_open_flags(&own(&["--title"])).is_err());
        assert!(split_open_flags(&own(&["--note"])).is_err());
    }

    #[test]
    fn an_empty_origin_value_means_no_name() {
        let (flags, _) = split_open_flags(&own(&["--title=", "--note", " "])).unwrap();
        assert!(flags.origin.is_empty());
    }

    #[test]
    fn a_name_given_to_a_plain_file_open_is_dropped_but_kept_for_a_pipe_or_a_notification() {
        let named = |extra: &[&str]| {
            let mut args = vec!["--title", "x", "--note", "n"];
            args.extend_from_slice(extra);
            split_open_flags(&own(&args)).unwrap().0
        };
        let mut plain = named(&["a.md"]);
        plain.drop_origin_unless_shown(false);
        assert!(plain.origin.is_empty(), "実ファイルをそのまま開く");

        let mut pipe = named(&[]);
        pipe.drop_origin_unless_shown(true);
        assert_eq!(pipe.origin.title.as_deref(), Some("x"), "パイプ入力");

        let mut notify = named(&["--notify", "a.md"]);
        notify.drop_origin_unless_shown(false);
        assert_eq!(notify.origin.note.as_deref(), Some("n"), "--notify");
    }

    #[test]
    fn the_last_of_a_repeated_origin_flag_wins() {
        let (flags, _) = split_open_flags(&own(&["--note=a", "--note=b", "--title=x", "--title="])).unwrap();
        assert_eq!(flags.origin.note.as_deref(), Some("b"));
        assert_eq!(flags.origin.title, None);
    }

    #[test]
    fn a_file_named_like_an_origin_flag_is_reachable_through_the_terminator() {
        let (flags, paths) = split_open_flags(&own(&["--", "--title"])).unwrap();
        assert!(flags.origin.is_empty());
        assert_eq!(paths, vec!["--title"]);
    }

    fn own(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
}
