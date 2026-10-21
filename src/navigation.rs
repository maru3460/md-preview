//! 窓の中の移動と内線（IPC）を通してよいかの判定。
//!
//! WKWebView のデリゲートから渡された事実（URL・移動の種類・行き先と送り元のフレーム）だけを
//! 受けて答えを返す。ObjC の層（バイナリ側の `webview_delegate.rs`）は事実を集めて答えを WebKit へ
//! 返すだけにし、判定はここで単体テストする。
//!
//! 規則は許可制で、[`decide`] の doc に全部書く。ほかに、内線はメインフレームの md の画面からだけ
//! 受ける（[`ipc_from_app`]）、内線で開けと言われた URL は http(s) と外部スキームの許可リストだけ
//! （[`open_url_decision`]）。

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
    /// 取り消して、そのスキームを受け持つアプリで開く（[`external_allowed`] を通ったものだけ）。
    OpenExternally,
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

/// 移動を起こしたフレーム（`WKNavigationAction.sourceFrame`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source<'a> {
    /// メインフレーム（md の画面）。
    Main,
    /// サブフレーム。中身は、そのフレームがいま出している文書の URL（`request.URL`）。
    /// 空なら [`decide`] は [`Source::Unknown`] として扱う。
    Sub(&'a str),
    /// 分からない（宣言では nil にならないが、来ることがある）。
    Unknown,
}

/// 移動の判定に使う事実。デリゲートが `WKNavigationAction` から集めて渡す。
#[derive(Debug, Clone, Copy)]
pub struct NavFacts<'a> {
    /// 行き先の URL（`request.URL.absoluteString`）。
    pub url: &'a str,
    pub kind: NavKind,
    pub target: Target,
    /// 行き先のフレームがいま出している文書の URL（`targetFrame.request.URL`）。
    /// 新しい窓など、フレームが無ければ空。
    pub target_url: &'a str,
    pub source: Source<'a>,
    /// メインフレームが最初の読み込みを終えたか（デリゲートが `didCommitNavigation` を見たか）。
    pub main_committed: bool,
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

fn without_fragment(url: &str) -> &str {
    url.split_once('#').map_or(url, |(u, _)| u)
}

/// URL が md の画面（`mdpreview://localhost` ＋ [`is_index`]）か。
///
/// フラグメントは見ない: 目次などページ内の錨（`#sec`）へ飛んでも、同じ文書のまま。
/// スキームとホストは文字どおりに比べる。WebKit は小文字に正規化して渡してくるので、
/// 大文字が混ざった形は来ない。来たら md の画面とは認めない（取り消す側）。
fn is_app_page(url: &str) -> bool {
    let Some(rest) = without_fragment(url).strip_prefix("mdpreview://localhost") else { return false };
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    // `mdpreview://localhost` と `/` を省いた形も同じ画面（URL の正規化で `/` になる）。
    let path = if path.is_empty() { "/" } else { path };
    is_index(path, query)
}

/// `mdpreview:` の URL のパスが `/`（md の画面と、`/?file=` などクエリで引く口）か。
/// ホストは見ない（配る側もホストでは分けない）。
fn is_app_root(url: &str) -> bool {
    let Some((_, rest)) = url.split_once(':') else { return false };
    let rest = without_fragment(rest);
    let rest = rest.split_once('?').map_or(rest, |(r, _)| r);
    let path = rest.strip_prefix("//").map_or(rest, |r| r.find('/').map_or("", |i| &r[i..]));
    path.is_empty() || path == "/"
}

/// まだ何も出していないフレームか（作られたばかりの iframe は `about:blank` から始まる）。
fn is_fresh_frame(target_url: &str) -> bool {
    target_url.is_empty() || target_url == "about:blank"
}

/// 外へ開いてよいクリックの送り元か。メインフレームと、アプリが配った文書のフレームだけ。
///
/// Why not 無名の文書（`about:srcdoc` など）のフレームも入れる: 外部の iframe の中で作られた
/// 無名のフレームと見分けが付かない。
fn trusted_source(source: Source<'_>) -> bool {
    match source {
        Source::Main => true,
        Source::Sub(url) => scheme_of(url) == Scheme::App,
        Source::Unknown => false,
    }
}

/// 移動を通すか。
///
/// `http(s)`:
///
/// - リンクのクリックは、送り元がメインフレームか `mdpreview:` のフレームなら既定のブラウザへ
///   渡し、それ以外（外部の iframe の中のリンクなど）は何も開かずに取り消す。行き先のフレームは
///   問わない（`target=_blank` も `_top` も同じ）
/// - クリックでない移動は、まだ何も出していないサブフレームの最初の読み込み（種類が other）
///   だけを窓の中で通す。設定で外部の読み込みを許したときの外部の iframe がこれ。許していない
///   ときは、シェルの CSP（`frame-src`）がここより手前で止める。表示中のフレームが移る
///   （メタリフレッシュ・`location` の書き換え・フォームの送信）のは取り消す
///
/// 外部スキーム（`mailto:` など）: リンクのクリックで、送り元が上と同じで、URL が閉じた許可リスト
/// （[`external_allowed`]）に入るものだけ、受け持つアプリで開いて取り消す。ほかは取り消す。
///
/// `mdpreview:`:
///
/// - メインフレーム: md の画面（`is_app_page`）だけ。ほかのパスはどれも取り消す（script が
///   止まっていない素の文書や、守りの無い md の 1 枚ページへ移られないように）。最初の読み込み
///   （種類は other で来る）を終えた後は、種類が reload のものと、いま出している画面の中の
///   錨へのジャンプだけを通す。reload は人の ⌘R のほか、本文の `<meta http-equiv=refresh>`
///   （URL 無し）も実機ではこの種類で届くので、ここでは見分けずに通す。それを止めているのは、
///   docguard が md の本文から `<meta>` を落とすこと（`.html` は iframe の中なので、
///   メインフレームを読み込み直させられない）
/// - サブフレーム: パスが `/`（md の画面と、クエリで引く口）は常に取り消す。ほかは、送り元が
///   `mdpreview:` のフレームなら通す（`.html` の中の相対リンク・サブリソース）。送り元が
///   メインフレーム・無名の文書（`about:srcdoc` など）のフレーム・分からないときは、種類が
///   other（iframe を作ったときの読み込み）だけを通し、リンクの `target` で既存のフレームを
///   差し替えるのは取り消す
/// - 新しい窓: 取り消す
///
/// 無名の文書（`about:`・`data:`・`blob:`）: サブフレームだけ通す（`srcdoc` の iframe など）。
///
/// `file:`・`javascript:`・スキームの無いもの: 常に取り消す。
///
/// Why not サブフレームの送り元が「行き先のフレーム自身か、その中のフレーム」かまで確かめる:
/// 公開の `WKFrameInfo` にはフレームの親子を辿る口が無い。同じ文書の中の兄弟のフレームからの
/// 移動も通るが、送り元が `mdpreview:` の文書であることは確かめている。
pub fn decide(facts: NavFacts<'_>) -> NavDecision {
    let NavFacts { url, kind, target, target_url, source, main_committed } = facts;
    // 送り元のフレームの URL が取れない（`request.URL` が nil など）ときは、分からないものとして
    // 扱う。Why not 空の URL のまま判定する: スキームの無いものは取り消す側なので、`.html` の
    // プレビューを作る読み込みまで止まる。
    let source = match source {
        Source::Sub("") => Source::Unknown,
        other => other,
    };
    let clicked = kind == NavKind::LinkActivated;
    match scheme_of(url) {
        Scheme::Web if clicked => {
            if trusted_source(source) { NavDecision::OpenInBrowser } else { NavDecision::Cancel }
        }
        Scheme::Web => {
            if target == Target::Sub && kind == NavKind::Other && is_fresh_frame(target_url) {
                NavDecision::Allow
            } else {
                NavDecision::Cancel
            }
        }
        Scheme::External => {
            if clicked && trusted_source(source) && external_allowed(url) {
                NavDecision::OpenExternally
            } else {
                NavDecision::Cancel
            }
        }
        Scheme::App => match target {
            Target::Main => decide_main_app(url, kind, target_url, main_committed),
            Target::Sub => decide_sub_app(url, kind, source),
            Target::NewWindow => NavDecision::Cancel,
        },
        Scheme::Anonymous if target == Target::Sub => NavDecision::Allow,
        Scheme::Anonymous | Scheme::Never => NavDecision::Cancel,
    }
}

fn decide_main_app(url: &str, kind: NavKind, target_url: &str, main_committed: bool) -> NavDecision {
    if !is_app_page(url) {
        return NavDecision::Cancel;
    }
    // 錨へのジャンプは、フラグメントを除いて今の文書と同じ URL のときだけ。`/` から `/?#x` へは
    // 錨に見えても読み込み直しになる。
    let same_document_anchor = url.contains('#') && without_fragment(url) == without_fragment(target_url);
    if !main_committed || kind == NavKind::Reload || same_document_anchor {
        NavDecision::Allow
    } else {
        NavDecision::Cancel
    }
}

fn decide_sub_app(url: &str, kind: NavKind, source: Source<'_>) -> NavDecision {
    if is_app_root(url) {
        return NavDecision::Cancel;
    }
    let allowed = match source {
        Source::Sub(from) => match scheme_of(from) {
            Scheme::App => true,
            // 無名の文書のフレームは、`.html` の中の `srcdoc` の iframe と、外部の iframe の中で
            // 作られたものとの見分けが付かない（[`trusted_source`] と同じ）。種類を問わず
            // 通すと、外部の iframe の中から既存のフレームをアプリのファイルへ移らせられる。
            Scheme::Anonymous => kind == NavKind::Other,
            _ => false,
        },
        // Why not 行き先がまだ何も出していないフレームかで見る: 作ったばかりの iframe で
        // `targetFrame.request` に何が入るかを当てにすると、外れたときに `.html` の
        // プレビューが全部止まる。メインフレームの文書（md の本文）が既存のフレームを
        // 差し替えるのは、script を止めていればリンクとフォームの `target` だけ。
        Source::Main | Source::Unknown => kind == NavKind::Other,
    };
    if allowed { NavDecision::Allow } else { NavDecision::Cancel }
}

/// 外部スキームの閉じた許可リスト。ここに無いスキームと形は、押しても何も開かない。
///
/// - `mailto:` は宛先だけ。`?subject=`・`?body=`・`?cc=` などは付けられない（本文を仕込んだ
///   メールの下書きを開かせない）。宛先はカンマ区切りの `local@domain` で、`%` による
///   エスケープも受けない
/// - `vscode://file/<パス>` だけ（ファイルを開く）。`vscode://<拡張機能>/…` は拡張機能の
///   コマンドに届くので開かない。クエリとフラグメントも付けられない
///
/// Why not `obsidian:` などほかのエディタ・ノートアプリも入れる: 開くだけの形とプラグインの
/// コマンドに届く形がスキームの中で混ざっていて、形で絞り切れない。要るときは、ここに形ごと足す。
pub fn external_allowed(url: &str) -> bool {
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let Some((scheme, rest)) = url.split_once(':') else { return false };
    match scheme.to_ascii_lowercase().as_str() {
        "mailto" => is_mail_addresses(rest),
        "vscode" => is_vscode_file(rest),
        _ => false,
    }
}

fn is_mail_addresses(rest: &str) -> bool {
    let local_ok = |c: char| c.is_ascii_alphanumeric() || ".!$'*+-/=^_`{|}~".contains(c);
    let domain_ok = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
    !rest.is_empty()
        && rest.split(',').all(|addr| {
            let Some((local, domain)) = addr.split_once('@') else { return false };
            !local.is_empty()
                && !domain.is_empty()
                && local.chars().all(local_ok)
                && domain.chars().all(domain_ok)
        })
}

fn is_vscode_file(rest: &str) -> bool {
    let Some(head) = rest.get(..7) else { return false };
    head.eq_ignore_ascii_case("//file/")
        && rest.len() > 7
        && !rest.contains(['?', '#'])
}

/// 内線の `open-url:` で渡された URL をどう開くか。答えは [`NavDecision::OpenInBrowser`]・
/// [`NavDecision::OpenExternally`]・[`NavDecision::Cancel`] のどれか。
///
/// `.html` のプレビューの中の外部リンク（`http(s)` も `mailto:` なども）は、シェルの CSP が
/// 移動の判定より手前で止めるので、親の script（common.js の bindFrame）がクリックを受けて
/// ここへ回してくる。移動の判定でリンクのクリックを外へ渡すのと同じ線で、`http(s)` は
/// [`open_url_allowed`] を、外部スキームは [`external_allowed`] を通ったものだけを開く。
///
/// Why not クリックだったかをここで確かめる: 内線は事実（移動の種類）を運ばない。人が押した
/// かどうかは送る側（isTrusted）が見ている。文書の script を許していれば `top.ipc` を直に
/// 呼べるので、ここで何を見ても防げない（[`ipc_from_app`] の doc と同じ）。
pub fn open_url_decision(url: &str) -> NavDecision {
    if open_url_allowed(url) {
        NavDecision::OpenInBrowser
    } else if scheme_of(url) == Scheme::External && external_allowed(url) {
        NavDecision::OpenExternally
    } else {
        NavDecision::Cancel
    }
}

/// 内線の `open-url:` で渡された URL を、既定のブラウザで開いてよいか。
///
/// 移動の判定でブラウザへ渡すのと同じく `http(s)` だけで、ホストの無いもの・空白や制御文字の
/// 混ざったもの（script が組めば作れる。`URL.href` はそうした形を出さない）は開かない。
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

    const APP: &str = "mdpreview://localhost/";
    const HTML: &str = "mdpreview://localhost/docs/page.html";
    const EXT_FRAME: &str = "https://www.youtube.com/embed/x";

    /// 送り元の候補。外部の iframe と、その中に作られたものと見分けの付かない無名のフレーム、
    /// URL の取れないフレームは信用しない側。
    const TRUSTED_SOURCES: [Source<'static>; 3] = [Source::Main, Source::Sub(HTML), Source::Sub(APP)];
    const UNTRUSTED_SOURCES: [Source<'static>; 7] = [
        Source::Sub(EXT_FRAME),
        Source::Sub("about:blank"),
        Source::Sub("about:srcdoc"),
        Source::Sub("data:text/html,hi"),
        Source::Sub("blob:https://www.youtube.com/1234"),
        Source::Sub(""),
        Source::Unknown,
    ];

    /// 無名の文書のフレーム。`.html` の中の `srcdoc` の iframe なのか、外部の iframe の中で
    /// 作られたものなのかは見分けが付かない。
    const ANONYMOUS_SOURCES: [Source<'static>; 4] = [
        Source::Sub("about:blank"),
        Source::Sub("about:srcdoc"),
        Source::Sub("data:text/html,hi"),
        Source::Sub("blob:mdpreview://localhost/1234"),
    ];

    /// 読み込みを終えたメインフレームの中で起きた移動。行き先のフレームが今出している文書は、
    /// メインなら md の画面、サブなら `.html` のプレビュー、新しい窓なら無し。
    fn facts<'a>(url: &'a str, kind: NavKind, target: Target, source: Source<'a>) -> NavFacts<'a> {
        let target_url = match target {
            Target::Main => APP,
            Target::Sub => HTML,
            Target::NewWindow => "",
        };
        NavFacts { url, kind, target, target_url, source, main_committed: true }
    }

    fn at(url: &str, kind: NavKind, target: Target) -> NavDecision {
        decide(facts(url, kind, target, Source::Main))
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

    /// 本文の普通のリンク（メインフレーム）・`.html` の中のリンクと `_top`（送り元は `.html` の
    /// フレーム）・`target=_blank`（行き先のフレーム無し）は、どれも種類がリンクのクリックで届く。
    #[test]
    fn clicked_web_links_from_the_app_open_in_the_browser_whatever_the_target() {
        for url in ["https://example.com/", "http://example.com/a?b#c", "HTTPS://EXAMPLE.COM/", "HtTp://x/"] {
            for target in ALL_TARGETS {
                for source in TRUSTED_SOURCES {
                    let f = facts(url, NavKind::LinkActivated, target, source);
                    assert_eq!(decide(f), NavDecision::OpenInBrowser, "{url} {target:?} {source:?}");
                }
            }
        }
    }

    /// 外部の iframe（設定で外部の読み込みを許したとき）の中のリンクや広告は、ブラウザを開かない。
    #[test]
    fn clicked_web_links_from_external_or_unknown_frames_open_nothing() {
        for target in ALL_TARGETS {
            for source in UNTRUSTED_SOURCES {
                let f = facts("https://example.com/", NavKind::LinkActivated, target, source);
                assert_eq!(decide(f), NavDecision::Cancel, "{target:?} {source:?}");
            }
        }
    }

    /// メタリフレッシュ・`location` の書き換え・フォームの送信・`window.open` は、取り消され、
    /// ブラウザも開かない。サブフレームでも、表示中のフレームが移るのは同じ。
    #[test]
    fn web_navigations_without_a_click_are_cancelled_and_open_nothing() {
        for kind in not_clicked() {
            for target in ALL_TARGETS {
                for source in TRUSTED_SOURCES.into_iter().chain(UNTRUSTED_SOURCES) {
                    for url in ["https://example.com/", "http://127.0.0.1:8000/"] {
                        let f = facts(url, kind, target, source);
                        assert_eq!(decide(f), NavDecision::Cancel, "{url} {kind:?} {target:?} {source:?}");
                    }
                }
            }
        }
        // 外部の iframe が自分の中でメタリフレッシュする。
        let f = NavFacts { target_url: EXT_FRAME, ..facts("https://evil.example/", NavKind::Other, Target::Sub, Source::Sub(EXT_FRAME)) };
        assert_eq!(decide(f), NavDecision::Cancel);
    }

    /// 設定で外部の読み込みを許したときの、YouTube などの iframe。許していなければ CSP が先に止める。
    #[test]
    fn the_first_load_of_a_fresh_subframe_may_be_web() {
        for target_url in ["", "about:blank"] {
            for source in [Source::Main, Source::Sub(HTML), Source::Unknown] {
                let f = NavFacts { target_url, ..facts(EXT_FRAME, NavKind::Other, Target::Sub, source) };
                assert_eq!(decide(f), NavDecision::Allow, "{target_url:?} {source:?}");
            }
            // 最初の読み込みでも、種類が other でなければ（フォームの送信先のフレームなど）通さない。
            for kind in not_clicked().filter(|k| *k != NavKind::Other) {
                let f = NavFacts { target_url, ..facts(EXT_FRAME, kind, Target::Sub, Source::Main) };
                assert_eq!(decide(f), NavDecision::Cancel, "{target_url:?} {kind:?}");
            }
            // メインフレームと新しい窓には「最初の読み込み」の例外が無い。
            for target in [Target::Main, Target::NewWindow] {
                let f = NavFacts { target_url, ..facts(EXT_FRAME, NavKind::Other, target, Source::Main) };
                assert_eq!(decide(f), NavDecision::Cancel, "{target_url:?} {target:?}");
            }
        }
    }

    /// 閉じた許可リスト。ここに足すときは、そのスキームで他のアプリの操作に届く形が無いかを見る。
    #[test]
    fn the_external_scheme_allowlist_is_mailto_addresses_and_vscode_files() {
        for url in [
            "mailto:a@example.com",
            "MailTo:a@example.com",
            "mailto:first.last+tag@sub.example.co.jp",
            "mailto:a@example.com,b@example.org",
            "vscode://file/Users/me/proj/README.md",
            "vscode://file/Users/me/proj/src/main.rs:12:3",
            "VSCODE://FILE/Users/me/a.md",
            "vscode://file/Users/me/My%20Docs/a.md",
        ] {
            assert!(external_allowed(url), "{url}");
        }
    }

    #[test]
    fn external_schemes_outside_the_allowlist_or_its_shapes_are_refused() {
        for url in [
            // 宛先のほかを付けた mailto:
            "mailto:a@example.com?subject=hi",
            "mailto:a@example.com?body=run%20this",
            "mailto:?to=a@example.com",
            "mailto:a@example.com#x",
            "mailto:a%40example.com",
            "mailto:",
            "mailto:nobody",
            "mailto:a@",
            "mailto:@example.com",
            "mailto:a@b@example.com",
            "mailto:a@example.com,",
            "mailto:a@example.com b@example.com",
            "mailto:a@example.com\n",
            // ファイルを開く以外の vscode:
            "vscode://vscode.git/clone?url=https://evil.example/r.git",
            "vscode://ms-python.python/x",
            "vscode://file",
            "vscode://file/",
            "vscode://fileX/a",
            "vscode:file/a",
            "vscode://file/a.md?windowId=_blank",
            "vscode://file/a.md#L1",
            "vscode-insiders://file/a.md",
            // 許可リストに無いスキーム
            "obsidian://open?vault=v&file=f",
            "slack://open",
            "tel:+81-3-0000-0000",
            "x-apple.systempreferences:com.apple.preference.security",
            "ssh://host",
            "",
        ] {
            assert!(!external_allowed(url), "{url:?}");
        }
    }

    /// 許可リストに入るものだけを、押したときに、信用する送り元からだけ開く。
    #[test]
    fn external_schemes_open_only_when_clicked_from_the_app_and_allowed() {
        for url in ["mailto:a@example.com", "vscode://file/Users/me/a.md"] {
            for target in ALL_TARGETS {
                for source in TRUSTED_SOURCES {
                    let f = facts(url, NavKind::LinkActivated, target, source);
                    assert_eq!(decide(f), NavDecision::OpenExternally, "{url} {target:?} {source:?}");
                }
                for source in UNTRUSTED_SOURCES {
                    let f = facts(url, NavKind::LinkActivated, target, source);
                    assert_eq!(decide(f), NavDecision::Cancel, "{url} {target:?} {source:?}");
                }
                for kind in not_clicked() {
                    assert_eq!(at(url, kind, target), NavDecision::Cancel, "{url} {kind:?} {target:?}");
                }
            }
        }
        for url in ["obsidian://open?vault=x", "vscode://ms-python.python/x", "mailto:a@example.com?subject=x"] {
            for target in ALL_TARGETS {
                assert_eq!(at(url, NavKind::LinkActivated, target), NavDecision::Cancel, "{url} {target:?}");
            }
        }
    }

    /// 最初の読み込みは種類が other で来る。読み込み前のメインフレームは何も出していない。
    #[test]
    fn the_main_frame_may_load_the_app_page_first_for_every_kind() {
        for url in [APP, "mdpreview://localhost", "mdpreview://localhost/?", "mdpreview://localhost/#sec"] {
            for kind in ALL_KINDS {
                for target_url in ["", "about:blank"] {
                    let f = NavFacts { target_url, main_committed: false, ..facts(url, kind, Target::Main, Source::Unknown) };
                    assert_eq!(decide(f), NavDecision::Allow, "{url} {kind:?} {target_url:?}");
                }
            }
        }
    }

    /// 読み込んだ後は、再読み込みと、いま出している画面の中の錨へのジャンプだけ。
    #[test]
    fn after_the_first_load_the_main_frame_only_reloads_or_jumps_to_anchors() {
        for url in [APP, "mdpreview://localhost", "mdpreview://localhost/?"] {
            assert_eq!(at(url, NavKind::Reload, Target::Main), NavDecision::Allow, "{url}");
        }
        for kind in ALL_KINDS {
            assert_eq!(at("mdpreview://localhost/#sec", kind, Target::Main), NavDecision::Allow, "{kind:?}");
            let f = NavFacts { target_url: "mdpreview://localhost/#old", ..facts("mdpreview://localhost/#sec", kind, Target::Main, Source::Main) };
            assert_eq!(decide(f), NavDecision::Allow, "{kind:?}");
        }
    }

    /// `location` の書き換えなど、種類が reload でない移動では md の画面を読み込み直させられない。
    /// 本文の `<meta http-equiv=refresh>`（URL 無し）は実機では種類 reload で届くので、ここでは
    /// 止まらない（止めているのは docguard が md の本文から `<meta>` を落とすこと）。
    #[test]
    fn after_the_first_load_the_main_frame_reloads_only_by_reload_navigations() {
        for kind in ALL_KINDS.into_iter().filter(|k| *k != NavKind::Reload) {
            for url in [APP, "mdpreview://localhost", "mdpreview://localhost/?"] {
                assert_eq!(at(url, kind, Target::Main), NavDecision::Cancel, "{url} {kind:?}");
            }
            // `/` から `/?#x` は錨に見えても読み込み直し。
            assert_eq!(at("mdpreview://localhost/?#x", kind, Target::Main), NavDecision::Cancel, "{kind:?}");
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
                for main_committed in [false, true] {
                    let f = NavFacts { main_committed, ..facts(url, kind, Target::Main, Source::Main) };
                    assert_eq!(decide(f), NavDecision::Cancel, "{url} {kind:?} {main_committed}");
                }
            }
        }
    }

    const APP_FILES: [&str; 4] = [
        HTML,
        "mdpreview://localhost/docs/other.html?q=1#x",
        "mdpreview://localhost/__abs/Users/me/page.html#x",
        "MDPREVIEW://localhost/docs/page.html",
    ];

    const ANONYMOUS_URLS: [&str; 5] =
        ["about:blank", "about:srcdoc", "data:text/html,hi", "Data:text/html,hi", "blob:mdpreview://localhost/1234"];

    /// アプリ自身の `.html` プレビューの中の相対リンクとサブリソースは、その文書から起きたもの
    /// なら種類を問わず通す。
    #[test]
    fn subframes_may_load_app_files_from_app_documents_for_every_kind() {
        for url in APP_FILES {
            for kind in ALL_KINDS {
                let f = facts(url, kind, Target::Sub, Source::Sub(HTML));
                assert_eq!(decide(f), NavDecision::Allow, "{url} {kind:?}");
            }
        }
    }

    /// 無名の文書（`srcdoc` の iframe など）は、送り元を問わずサブフレームに出してよい。
    #[test]
    fn subframes_may_load_anonymous_documents_from_any_source() {
        for url in ANONYMOUS_URLS {
            for kind in ALL_KINDS {
                for source in TRUSTED_SOURCES.into_iter().chain(UNTRUSTED_SOURCES) {
                    let f = facts(url, kind, Target::Sub, source);
                    assert_eq!(decide(f), NavDecision::Allow, "{url} {kind:?} {source:?}");
                }
            }
        }
    }

    /// 無名のフレームからアプリのファイルへは、iframe を作ったときの読み込み（種類が other）
    /// だけ。外部の iframe の中で作られた無名のフレームから、既存のフレームをアプリのファイルへ
    /// 移らせられない。
    #[test]
    fn subframes_driven_by_anonymous_frames_only_take_their_initial_load_of_app_files() {
        for url in APP_FILES {
            for source in ANONYMOUS_SOURCES {
                assert_eq!(decide(facts(url, NavKind::Other, Target::Sub, source)), NavDecision::Allow, "{url} {source:?}");
                for kind in ALL_KINDS.into_iter().filter(|k| *k != NavKind::Other) {
                    let f = facts(url, kind, Target::Sub, source);
                    assert_eq!(decide(f), NavDecision::Cancel, "{url} {kind:?} {source:?}");
                }
            }
        }
    }

    /// 送り元のフレームの URL が空（`request.URL` が nil など）なら、分からないときと同じ答え。
    /// `.html` のプレビューを作る読み込みは止まらない。
    #[test]
    fn a_source_frame_without_a_url_is_treated_as_unknown() {
        for url in APP_FILES.into_iter().chain(ANONYMOUS_URLS).chain([APP, EXT_FRAME, "https://example.com/", "mailto:a@example.com"]) {
            for kind in ALL_KINDS {
                for target in ALL_TARGETS {
                    for target_url in ["", "about:blank", HTML] {
                        let at_source = |source| NavFacts { target_url, ..facts(url, kind, target, source) };
                        assert_eq!(
                            decide(at_source(Source::Sub(""))),
                            decide(at_source(Source::Unknown)),
                            "{url} {kind:?} {target:?} {target_url:?}"
                        );
                    }
                }
            }
        }
        assert_eq!(decide(facts(HTML, NavKind::Other, Target::Sub, Source::Sub(""))), NavDecision::Allow);
    }

    /// md の画面が iframe を作ったときの読み込み（`.html` のプレビュー）は通す。本文のリンクの
    /// `target` で既存のフレームを別のファイルへ差し替えるのは取り消す。
    #[test]
    fn subframes_driven_by_the_main_frame_only_take_their_initial_load() {
        for source in [Source::Main, Source::Unknown] {
            assert_eq!(decide(facts(HTML, NavKind::Other, Target::Sub, source)), NavDecision::Allow, "{source:?}");
            for kind in ALL_KINDS.into_iter().filter(|k| *k != NavKind::Other) {
                let f = facts("mdpreview://localhost/__abs/Users/me/.ssh/config", kind, Target::Sub, source);
                assert_eq!(decide(f), NavDecision::Cancel, "{kind:?} {source:?}");
            }
        }
        // 外部の iframe の中から、アプリのファイルへ移らせることはできない。
        for kind in ALL_KINDS {
            let f = facts(HTML, kind, Target::Sub, Source::Sub(EXT_FRAME));
            assert_eq!(decide(f), NavDecision::Cancel, "{kind:?}");
        }
    }

    /// `<iframe src="/">` で md の画面をもう 1 枚作ったり、`/?file=` などの口をフレームに出したり
    /// させない。送り元・種類を問わない。
    #[test]
    fn subframes_never_load_the_app_root_or_its_query_routes() {
        for url in [
            APP,
            "mdpreview://localhost",
            "mdpreview://localhost/?",
            "mdpreview://localhost/#x",
            "mdpreview://localhost/?file=/Users/me/a.md",
            "mdpreview://localhost/?raw=/Users/me/.ssh/config",
            "mdpreview://localhost/?dir=/",
            "mdpreview://other/?file=a",
            "MDPREVIEW://localhost/?file=a",
        ] {
            for kind in ALL_KINDS {
                for source in TRUSTED_SOURCES.into_iter().chain(UNTRUSTED_SOURCES).chain([Source::Sub("about:srcdoc")]) {
                    let f = facts(url, kind, Target::Sub, source);
                    assert_eq!(decide(f), NavDecision::Cancel, "{url} {kind:?} {source:?}");
                }
            }
        }
    }

    /// 行き先のフレームが無い移動は、押したリンクを外へ渡すほかは全部取り消す。
    #[test]
    fn new_windows_are_never_created() {
        for url in [
            APP,
            HTML,
            "about:blank",
            "data:text/html,hi",
            "blob:mdpreview://localhost/1234",
            "obsidian://open?vault=x",
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
                    for source in TRUSTED_SOURCES {
                        let f = facts(url, kind, target, source);
                        assert_eq!(decide(f), NavDecision::Cancel, "{url} {kind:?} {target:?} {source:?}");
                    }
                }
            }
        }
    }

    /// `.html` の中の外部リンクを内線で受けたとき。移動の判定でリンクのクリックを外へ渡すのと同じ線。
    #[test]
    fn open_url_opens_web_urls_in_the_browser_and_allowlisted_schemes_externally() {
        for url in ["https://example.com/", "http://127.0.0.1:8000/", "HTTPS://EXAMPLE.COM/"] {
            assert_eq!(open_url_decision(url), NavDecision::OpenInBrowser, "{url}");
        }
        for url in ["mailto:a@example.com", "mailto:a@example.com,b@example.org", "vscode://file/Users/me/a.md", "VSCODE://FILE/Users/me/a.md"] {
            assert_eq!(open_url_decision(url), NavDecision::OpenExternally, "{url}");
        }
    }

    #[test]
    fn open_url_ignores_everything_else() {
        for url in [
            "https://",
            "https://example.com/a b",
            "mailto:a@example.com?subject=hi",
            "vscode://vscode.git/clone?url=https://evil.example/r.git",
            "vscode://file/a.md#L1",
            "obsidian://open?vault=x",
            "mdpreview://localhost/docs/page.html",
            "mdpreview://localhost/",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "about:blank",
            "blob:mdpreview://localhost/1234",
            "",
        ] {
            assert_eq!(open_url_decision(url), NavDecision::Cancel, "{url:?}");
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
