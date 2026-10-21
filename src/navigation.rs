//! 窓の中の移動と内線（IPC）を通してよいかの判定。
//!
//! WKWebView のデリゲートから渡された事実（URL・移動の種類・行き先や送り元のフレーム）だけを
//! 受けて答えを返す。ObjC の層（バイナリ側の `webview_delegate.rs`）は事実を集めて答えを WebKit へ
//! 返すだけにし、判定はここで単体テストする。
//!
//! 今ここにある規則は、メインフレームは md の画面だけ、既定のブラウザ・外部スキームへ渡すのは
//! リンクのクリックのときだけ、新しい窓は作らない、内線はメインフレームの md の画面からだけ受ける、
//! 内線で開けと言われた URL は http(s) だけ、の 5 つ。サブフレームの移動を送り元で分ける・外部スキームを閉じた許可リストにする、は
//! まだ無い（ここに足す）。

use crate::request::is_index;

/// 移動の種類（`WKNavigationType`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKind {
    /// リンクのクリック（`target=_blank` やキーボードでの決定も含む）。
    LinkActivated,
    FormSubmitted,
    BackForward,
    Reload,
    FormResubmitted,
    /// それ以外。メタリフレッシュ、`location` の書き換え、iframe の読み込み、`window.open` など。
    Other,
}

impl NavKind {
    /// `WKNavigationType` の生の値から。知らない値は [`NavKind::Other`] に寄せる。
    ///
    /// Why not 知らない値をリンクのクリック扱いにする: OS が種類を足したとき、それが
    /// クリックでない移動なら外へ開いてしまう。開かない側へ倒す。
    pub fn from_raw(raw: isize) -> NavKind {
        match raw {
            0 => NavKind::LinkActivated,
            1 => NavKind::FormSubmitted,
            2 => NavKind::BackForward,
            3 => NavKind::Reload,
            4 => NavKind::FormResubmitted,
            _ => NavKind::Other,
        }
    }
}

/// 移動への答え。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavDecision {
    /// 窓の中で移動させる。
    Allow,
    /// 取り消し、何も開かない。
    Cancel,
    /// 取り消して、既定のブラウザで開く。
    OpenInBrowser,
}

/// 移動の行き先のフレーム（`WKNavigationAction.targetFrame`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// メインフレーム。md の画面が載っている文書そのもの。
    Main,
    /// iframe・`<object>`・`<embed>` の中のフレーム。
    Sub,
    /// 行き先のフレームが無い（`target=_blank`・`window.open` など、新しい窓を作ろうとする移動）。
    NewWindow,
}

/// 移動の判定に使う事実。デリゲートが `WKNavigationAction` から集めて渡す。
#[derive(Debug, Clone, Copy)]
pub struct NavFacts<'a> {
    /// 行き先の URL（`request.URL.absoluteString`）。
    pub url: &'a str,
    pub kind: NavKind,
    pub target: Target,
}

/// URL のスキームの大分け。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scheme {
    /// アプリ自身が配るもの（`mdpreview:`）。
    App,
    /// 外へ出ない無名の文書（`about:`・`data:`・`blob:`）。
    Anonymous,
    /// `http:` / `https:`。
    Web,
    /// 窓の中でも外でも、通す理由が無いもの。
    Never,
    /// それ以外（`mailto:`・`vscode:` など、他のアプリへ渡るもの）。
    External,
}

fn scheme_of(url: &str) -> Scheme {
    // スキームの無い URL は WebKit からは来ない（absoluteString は常に絶対 URL）。来たら
    // 何なのか分からないので、外へ渡らない側（取り消し）に倒す。
    let Some((scheme, _)) = url.split_once(':') else { return Scheme::Never };
    match scheme.to_ascii_lowercase().as_str() {
        "mdpreview" => Scheme::App,
        "about" | "data" | "blob" => Scheme::Anonymous,
        "http" | "https" => Scheme::Web,
        // `file:` はアプリが一度も使わない（ファイルは全部 `mdpreview:` で配る）。
        // `javascript:` は WebKit が移動として扱わない見込みだが、来たら通さない。
        "file" | "javascript" | "" => Scheme::Never,
        _ => Scheme::External,
    }
}

/// URL が md の画面（`mdpreview://localhost` ＋ [`is_index`]）か。
///
/// フラグメントは見ない: 目次などページ内の錨（`#sec`）へ飛んでも、同じ文書のまま。
/// スキームとホストは文字どおりに比べる。WebKit は小文字に正規化して渡してくるので、
/// 大文字が混ざった形は来ない。来たら md の画面とは認めない（取り消す側）。
fn is_app_page(url: &str) -> bool {
    let url = url.split_once('#').map_or(url, |(u, _)| u);
    let Some(rest) = url.strip_prefix("mdpreview://localhost") else { return false };
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    // `mdpreview://localhost` と `/` を省いた形も同じ画面（URL の正規化で `/` になる）。
    let path = if path.is_empty() { "/" } else { path };
    is_index(path, query)
}

/// 移動を通すか。規則は行き先のフレームで分かれる。
///
/// - メインフレーム: md の画面（`is_app_page`）だけを通す。種類は問わない（最初の読み込みは
///   種類が other で来る。再読み込みもここ）。ほかの `mdpreview:` のパス・`about:`・`data:`・
///   `blob:` も取り消す。本文の `<meta http-equiv=refresh>` や `location` の書き換えで、
///   md の画面の外（script が止まっていない素の文書）へ移られないように
/// - サブフレーム: `mdpreview:` と無名の文書は通す（`.html` のプレビュー、その中の相対リンクと
///   サブリソース、`srcdoc` の iframe）
/// - 新しい窓（行き先のフレームが無い）: 作らない。wry は `new_window_req_handler` を
///   渡さなければ窓を作らないが、作る手前で取り消しておく
///
/// どのフレームでも共通:
///
/// - `http(s)` は窓の中では開かない。リンクのクリックなら既定のブラウザへ渡し、それ以外
///   （メタリフレッシュなど、CSP では止まらない移動）は何も開かずに取り消す
/// - 外部スキームは、メインフレームとサブフレームでリンクのクリックのときだけ今までどおり
///   WebKit に任せる。それ以外は取り消す。メインフレームで任せても、WebKit はそのスキームの
///   文書を読めないので、メインフレームが md の画面から動くことは無い
///
/// Why not 外部スキームをここで開く: どのスキームをどんな形なら開くかの一覧（閉じた許可リスト）
/// がまだ無い。それまでは、クリックのときの扱いを今までと変えない。
pub fn decide(facts: NavFacts<'_>) -> NavDecision {
    let NavFacts { url, kind, target } = facts;
    let clicked = kind == NavKind::LinkActivated;
    match (scheme_of(url), target) {
        (Scheme::Web, _) if clicked => NavDecision::OpenInBrowser,
        (Scheme::App, Target::Main) if is_app_page(url) => NavDecision::Allow,
        (Scheme::App | Scheme::Anonymous, Target::Sub) => NavDecision::Allow,
        (Scheme::External, Target::Main | Target::Sub) if clicked => NavDecision::Allow,
        _ => NavDecision::Cancel,
    }
}

/// 内線の `open-url:` で渡された URL を、既定のブラウザで開いてよいか。
///
/// `.html` のプレビューの中の外部リンクは、シェルの CSP が移動の判定より手前で止めるので、
/// 親の script（common.js の bindFrame）がクリックを受けてここへ回してくる。移動の判定で
/// ブラウザへ渡すのと同じく `http(s)` だけで、ホストの無いもの・空白や制御文字の混ざったもの
/// （script が組めば作れる。`URL.href` はそうした形を出さない）は開かない。
///
/// Why not クリックだったかをここで確かめる: 内線は事実（移動の種類）を運ばない。人が押した
/// かどうかは送る側（isTrusted）が見ている。文書の script を許していれば `top.ipc` を直に
/// 呼べるので、ここで何を見ても防げない（[`ipc_from_app`] の doc と同じ）。
pub fn open_url_allowed(url: &str) -> bool {
    if scheme_of(url) != Scheme::Web {
        return false;
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let Some((_, rest)) = url.split_once(':') else { return false };
    let Some(rest) = rest.strip_prefix("//") else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    // `user@` を外した残りがホスト（とポート）。
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    // ポートを外す。IPv6 は `[::1]:8080` の形なので、`]` までをホストとして見る。
    let host = match host.strip_prefix('[') {
        Some(v6) => v6.split_once(']').map_or("", |(h, _)| h),
        None => host.split_once(':').map_or(host, |(h, _)| h),
    };
    !host.is_empty()
}

/// 内線のメッセージを受けてよいか。`is_main_frame` と `frame_url` は、送ってきたフレームの
/// `WKFrameInfo`（`isMainFrame` と `request.URL`）。
///
/// 正規の内線は全部、メインフレームに読まれた md の画面（クエリの無い `mdpreview://localhost/`）の
/// 自前の script から来る。URL だけでは足りない: 本文や `.html` が `<iframe src="/">` を作ると、
/// そのフレームにも md の画面が読まれて URL が同じになる。メインフレームかどうかで見分ける。
///
/// 判定は必ず許可制で書く。「iframe だと分かる形だけ弾く」と、`about:blank` / `about:srcdoc`
/// の無名のフレームを通してしまう。
///
/// これで塞げない経路が残る: 同じ origin のフレーム（`.html` のプレビューなど）が
/// `top.ipc.postMessage` を呼ぶと、メインフレームの `window.ipc` を通るので、メインフレーム
/// から来たものとして届く。そこを止めているのは CSP（文書の JS を既定で止める）で、ここでは
/// ない。ここが止めるのは、フレームが自分の `window.webkit.messageHandlers` から送る道。
pub fn ipc_from_app(is_main_frame: bool, frame_url: &str) -> bool {
    is_main_frame && is_app_page(frame_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_KINDS: [NavKind; 6] = [
        NavKind::LinkActivated,
        NavKind::FormSubmitted,
        NavKind::BackForward,
        NavKind::Reload,
        NavKind::FormResubmitted,
        NavKind::Other,
    ];

    const ALL_TARGETS: [Target; 3] = [Target::Main, Target::Sub, Target::NewWindow];

    fn at(url: &str, kind: NavKind, target: Target) -> NavDecision {
        decide(NavFacts { url, kind, target })
    }

    fn not_clicked() -> impl Iterator<Item = NavKind> {
        ALL_KINDS.into_iter().filter(|k| *k != NavKind::LinkActivated)
    }

    #[test]
    fn webkit_navigation_types_map_to_kinds_and_unknown_values_are_other() {
        assert_eq!(NavKind::from_raw(0), NavKind::LinkActivated);
        assert_eq!(NavKind::from_raw(1), NavKind::FormSubmitted);
        assert_eq!(NavKind::from_raw(2), NavKind::BackForward);
        assert_eq!(NavKind::from_raw(3), NavKind::Reload);
        assert_eq!(NavKind::from_raw(4), NavKind::FormResubmitted);
        assert_eq!(NavKind::from_raw(-1), NavKind::Other);
        assert_eq!(NavKind::from_raw(5), NavKind::Other);
    }

    /// 本文の普通のリンク（メインフレーム）・`.html` の中のリンクと `_top`（サブフレーム）・
    /// `target=_blank`（行き先のフレーム無し）は、どれも種類がリンクのクリックで届く。
    #[test]
    fn clicked_web_links_open_in_the_browser_from_every_frame() {
        for url in ["https://example.com/", "http://example.com/a?b#c", "HTTPS://EXAMPLE.COM/", "HtTp://x/"] {
            for target in ALL_TARGETS {
                assert_eq!(at(url, NavKind::LinkActivated, target), NavDecision::OpenInBrowser, "{url} {target:?}");
            }
        }
    }

    /// メタリフレッシュ・`location` の書き換え・iframe の読み込み・フォームの送信・
    /// `window.open` は、取り消され、ブラウザも開かない。
    #[test]
    fn web_navigations_without_a_click_are_cancelled_and_open_nothing() {
        for kind in not_clicked() {
            for target in ALL_TARGETS {
                for url in ["https://example.com/", "http://127.0.0.1:8000/"] {
                    assert_eq!(at(url, kind, target), NavDecision::Cancel, "{url} {kind:?} {target:?}");
                }
            }
        }
    }

    #[test]
    fn external_schemes_are_left_to_webkit_only_when_clicked_in_an_existing_frame() {
        for url in ["mailto:a@example.com", "vscode://file/x", "obsidian://open?vault=x", "MailTo:a@example.com"] {
            for target in [Target::Main, Target::Sub] {
                assert_eq!(at(url, NavKind::LinkActivated, target), NavDecision::Allow, "{url} {target:?}");
                for kind in not_clicked() {
                    assert_eq!(at(url, kind, target), NavDecision::Cancel, "{url} {kind:?} {target:?}");
                }
            }
        }
    }

    /// 最初の読み込み（種類は other）・再読み込み・目次の錨へのジャンプ。
    #[test]
    fn the_main_frame_may_load_the_app_page_for_every_kind() {
        for url in [
            "mdpreview://localhost/",
            "mdpreview://localhost",
            "mdpreview://localhost/?",
            "mdpreview://localhost/#sec",
        ] {
            for kind in ALL_KINDS {
                assert_eq!(at(url, kind, Target::Main), NavDecision::Allow, "{url} {kind:?}");
            }
        }
    }

    /// md の画面の外へ移ると、自前の script も CSP の守りも無い素の文書がメインフレームに出る。
    #[test]
    fn the_main_frame_cannot_leave_the_app_page() {
        for url in [
            // クエリの付いたルートは md の画面ではない（配る側も 404 を返す）。
            "mdpreview://localhost/?file=a.md",
            "mdpreview://localhost/docs/page.html",
            "mdpreview://localhost/docs/page.md",
            "mdpreview://localhost/__abs/Users/me/page.html#x",
            "mdpreview://localhost.evil/",
            "mdpreview://evil/",
            "mdpreview://localhost:80/",
            "MDPREVIEW://localhost/",
            "mdpreview://LOCALHOST/",
            "about:blank",
            "about:srcdoc",
            "data:text/html,hi",
            "DATA:text/html,hi",
            "blob:mdpreview://localhost/1234",
        ] {
            for kind in ALL_KINDS {
                assert_eq!(at(url, kind, Target::Main), NavDecision::Cancel, "{url} {kind:?}");
            }
        }
    }

    /// アプリ自身の `.html` プレビューの中の相対リンクとサブリソース・無名のフレームは、
    /// サブフレームでは通す。
    #[test]
    fn subframes_may_load_app_files_and_anonymous_documents_for_every_kind() {
        for url in [
            "mdpreview://localhost/",
            "mdpreview://localhost/docs/page.html",
            "mdpreview://localhost/__abs/Users/me/page.html#x",
            "MDPREVIEW://localhost/docs/page.html",
            "about:blank",
            "about:srcdoc",
            "data:text/html,hi",
            "Data:text/html,hi",
            "blob:mdpreview://localhost/1234",
        ] {
            for kind in ALL_KINDS {
                assert_eq!(at(url, kind, Target::Sub), NavDecision::Allow, "{url} {kind:?}");
            }
        }
    }

    /// 行き先のフレームが無い移動は、クリックした http(s) を既定のブラウザへ渡すほかは全部取り消す。
    #[test]
    fn new_windows_are_never_created() {
        for url in [
            "mdpreview://localhost/",
            "mdpreview://localhost/docs/page.html",
            "about:blank",
            "data:text/html,hi",
            "blob:mdpreview://localhost/1234",
            "mailto:a@example.com",
            "vscode://file/x",
        ] {
            for kind in ALL_KINDS {
                assert_eq!(at(url, kind, Target::NewWindow), NavDecision::Cancel, "{url} {kind:?}");
            }
        }
    }

    #[test]
    fn file_and_javascript_urls_and_scheme_less_strings_are_always_cancelled() {
        for url in ["file:///etc/passwd", "FILE:///tmp/x.html", "javascript:alert(1)", "no-scheme", ":x", ""] {
            for kind in ALL_KINDS {
                for target in ALL_TARGETS {
                    assert_eq!(at(url, kind, target), NavDecision::Cancel, "{url} {kind:?} {target:?}");
                }
            }
        }
    }

    #[test]
    fn open_url_accepts_web_urls_with_a_host() {
        for url in [
            "https://example.com/",
            "http://example.com/a?b#c",
            "https://example.com",
            "HTTPS://EXAMPLE.COM/",
            "https://user:pw@example.com:8443/x",
            "http://127.0.0.1:8000/",
            "http://[::1]:8080/",
            "https://ext.invalid/fig.png",
        ] {
            assert!(open_url_allowed(url), "{url}");
        }
    }

    #[test]
    fn open_url_rejects_other_schemes_and_urls_without_a_host() {
        for url in [
            "mdpreview://localhost/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "mailto:a@example.com",
            "vscode://file/x",
            "data:text/html,hi",
            "about:blank",
            "https:example.com",
            "https://",
            "https:///path",
            "https://user@/x",
            "https://:80/",
            "http://[]/",
            "-a https://example.com/",
            "",
        ] {
            assert!(!open_url_allowed(url), "{url}");
        }
    }

    /// `open` の引数は 1 つでも、URL として読む前に空白や改行で別のものに化けないように。
    #[test]
    fn open_url_rejects_whitespace_and_control_characters() {
        for url in ["https://example.com/a b", "https://example.com/\n", "https://example.com/\t", "https://exa\u{0}mple.com/"] {
            assert!(!open_url_allowed(url), "{url:?}");
        }
    }

    #[test]
    fn ipc_is_accepted_from_the_app_page_in_the_main_frame() {
        assert!(ipc_from_app(true, "mdpreview://localhost/"));
        assert!(ipc_from_app(true, "mdpreview://localhost"));
        assert!(ipc_from_app(true, "mdpreview://localhost/?"));
        assert!(ipc_from_app(true, "mdpreview://localhost/#sec"));
    }

    /// `<iframe src="/">` で md の画面を読み込んだフレームは、URL では見分けが付かない。
    #[test]
    fn ipc_from_a_subframe_is_rejected_even_at_the_app_url() {
        for url in ["mdpreview://localhost/", "mdpreview://localhost/docs/page.html", "about:blank", "https://example.com/"] {
            assert!(!ipc_from_app(false, url), "{url}");
        }
    }

    #[test]
    fn ipc_from_the_main_frame_elsewhere_is_rejected() {
        for url in [
            // クエリの付いたルートは md の画面ではない（配る側も 404 を返す）。
            "mdpreview://localhost/?file=a.md",
            "mdpreview://localhost/docs/page.html",
            "mdpreview://localhost/__abs/Users/me/page.md",
            "mdpreview://localhost.evil/",
            "mdpreview://evil/",
            "mdpreview://localhost:80/",
            "MDPREVIEW://localhost/",
            "https://localhost/",
            "about:blank",
            "about:srcdoc",
            "",
        ] {
            assert!(!ipc_from_app(true, url), "{url}");
        }
    }
}
