//! 文書の JavaScript と外部への読み込みを止める CSP を、1 か所で組み立てる。
//!
//! 付ける場所は 2 つある。md の画面（`/`）は `<head>` の meta で、それ以外の応答は
//! ヘッダで（[`crate::request::handle_request`] が付ける）。両方が同じ [`Policy`] を
//! 見るよう、Policy は起動時に 1 回だけ作り、`AppConfig` と `RequestContext` に同じ値を
//! 渡す。設定の切り替えは ⌘Q で終了して開き直したときから効く（プロセスの間は変えない）。
//!
//! 既定（OFF）で塞ぎ、設定「文書を信頼する」（`trusted`）で戻す。戻るのは次の 2 つ。
//! - 文書の JavaScript（md のイベント属性・`javascript:`、`.html` の script）
//! - 外部（http / https）からの画像・動画・音声・CSS・フォント・iframe の読み込み
//!
//! Why not 2 つを別のスイッチにする: 安全の軸として独立していない。JS を動かすと、外部を
//! 止めたままでも、文書の JS がアプリの内線でブラウザに URL を開かせて手元のファイルの中身を
//! 外へ出せる。「外部を OFF にしておけば送れない」と誤解させるより、「この文書を信じるか」の
//! 1 点にする（2026-10-23 に利用者が決めた）。失うのは「バッジだけ出したい」の使い方だけ。

/// 本番の md の画面の origin。meta の `script-src` で `/__lib/` を許す住所の頭に使う。
/// 開発用サーバ（`examples/serve.rs`）は自分の `http://127.0.0.1:<port>` を渡す。
pub const APP_ORIGIN: &str = "mdpreview://localhost";

/// `/__lib/` で配る同梱ライブラリの名前。meta はこれだけをファイル単位で許す。
/// 表は配る側と共有している（[`crate::html::BUILTIN_LIBS`]）。
pub fn lib_files() -> impl Iterator<Item = &'static str> {
    crate::html::BUILTIN_LIBS.iter().map(|(name, _)| *name)
}

/// 起動時に設定から 1 回だけ作る。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    /// 文書を信頼する（文書の JavaScript を動かし、外部からの読み込みを許す）。
    pub trusted: bool,
}

/// 応答の種類。ヘッダの中身はこれで決まる。
///
/// 決めるのは配る側のルート（`request.rs` の `Route`）で、パスではない。既定は
/// [`Kind::Other`]。`Index` と `Doc` だけを明示で決めるので、ルートを足して付け忘れても
/// いちばん閉じた指定に倒れる。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// md の画面そのもの（クエリの無い `/`）。ヘッダは付けず、meta だけで守る。
    Index,
    /// 文書のページ。`.html` / `.htm`（URL と実体の両方が html のもの）と、md を
    /// 直接開いた 1 枚ページ。文書を信頼する設定で `script-src` が緩むのはここだけ。
    Doc,
    /// それ以外すべて（本文の断片・JSON・画像・CSS・`/__lib/`・404 など）。
    Other,
}

const EXTERNAL: &str = "https: http:";

/// `script-src` 以外の指定。meta とヘッダで同じものを使う。
///
/// 外部を許すときも指定は消さずに `https: http:` を足す。消すと `data:` / `blob:` の
/// iframe なども開いてしまう。
fn base_directives(policy: &Policy) -> Vec<String> {
    let ext = |v: &str| {
        if policy.trusted {
            format!("{v} {EXTERNAL}")
        } else {
            v.to_string()
        }
    };
    vec![
        format!("default-src {}", ext("'self' data: blob:")),
        format!("img-src {}", ext("'self' data: blob:")),
        format!("media-src {}", ext("'self' data: blob:")),
        format!("font-src {}", ext("'self' data:")),
        format!("style-src {}", ext("'self' 'unsafe-inline'")),
        format!("frame-src {}", ext("'self'")),
        format!("connect-src {}", ext("'self'")),
        format!("object-src {}", ext("'self'")),
        "base-uri 'none'".to_string(),
        "form-action 'none'".to_string(),
    ]
}

/// 文書を信頼するときの `script-src`。
///
/// Why not `script-src` を書かない: 書かないと `default-src` に落ちて、inline の script が
/// アプリ自身のものまで全部止まる。
/// Why not ハッシュを残して `'unsafe-inline'` を足す: ハッシュがあると `'unsafe-inline'` は
/// 無視され、イベント属性と `javascript:` が動かない。
/// `'unsafe-eval'` は drawio の拡張図形が図形の取り込みに eval を使うため。ON の間は文書の
/// JS を信じる前提なので、足しても守りは変わらない。
fn open_script_src() -> String {
    format!("script-src 'self' 'unsafe-inline' 'unsafe-eval' data: blob: {EXTERNAL}")
}

fn join(mut directives: Vec<String>, script_src: String) -> String {
    directives.insert(0, script_src);
    directives.join("; ")
}

/// md の画面の `<meta>`。`<meta charset>` の直後、最初の `<script>` より前に置く
/// （meta の CSP は、それより後ろにしか効かない）。
///
/// `lib_origin` は `/__lib/` を許す住所。Policy とは別の引数にしているのは、設定ではなく
/// 「どこで配っているか」で決まる値だから。
/// Why not 本番にも `http://127.0.0.1:*` を足して 1 本にする: 本文の srcdoc から、別のポートで
/// 動く手元のサーバの同じパスの script が動いた（実測）。
pub fn meta(policy: &Policy, lib_origin: &str) -> String {
    let script_src = if policy.trusted {
        open_script_src()
    } else {
        let mut v = String::from("script-src");
        for h in script_hashes() {
            v.push_str(" 'sha256-");
            v.push_str(h);
            v.push('\'');
        }
        for f in lib_files() {
            v.push_str(&format!(" {lib_origin}/__lib/{f}"));
        }
        v
    };
    format!(
        r#"<meta http-equiv="Content-Security-Policy" content="{}">"#,
        crate::html::attr_escape(&join(base_directives(policy), script_src))
    )
}

/// ファイルの応答のヘッダ。`Index` には付けない（meta が守る。ヘッダを付けると、
/// 開発用サーバが `<head>` の頭に足す起動用の script がハッシュに無く、止まる）。
pub fn header(policy: &Policy, kind: Kind) -> Option<String> {
    let script_src = match kind {
        Kind::Index => return None,
        Kind::Doc if policy.trusted => open_script_src(),
        Kind::Doc | Kind::Other => "script-src 'none'".to_string(),
    };
    Some(join(base_directives(policy), script_src))
}

/// 自前の inline script のハッシュ（base64）。タグを出す列（`html::inline_script_sources`）
/// から作るので、script を足してもハッシュの付け忘れは起きない。中身は固定なので、
/// 初回に 1 回だけ計算する。
pub fn script_hashes() -> &'static [String] {
    static HASHES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    HASHES.get_or_init(|| {
        crate::html::inline_script_sources()
            .map(|src| base64(&sha256(src.as_bytes())))
            .collect()
    })
}

/// 1 つの inline script の中身のハッシュ（`'sha256-…'` の `…` の部分）。
pub fn hash_of(src: &str) -> String {
    base64(&sha256(src.as_bytes()))
}

/// Why CommonCrypto: 起動のたびに約 600KB を通すので、ハードウェアの SHA を使う OS の実装に
/// 任せる（md は起動の速さを最優先にしている）。libSystem に入っているので依存は増えない。
#[cfg(target_os = "macos")]
fn sha256(data: &[u8]) -> [u8; 32] {
    extern "C" {
        fn CC_SHA256(data: *const std::ffi::c_void, len: u32, md: *mut u8) -> *mut u8;
    }
    let mut out = [0u8; 32];
    // CC_LONG は 32 bit。inline の script はその上限よりずっと小さい（超えたら起動の時点で
    // 落として気づかせる。黙って切り詰めると、合わないハッシュでアプリが白紙になる）。
    let len = u32::try_from(data.len()).expect("inline script が 4GB を超えている");
    // SAFETY: data は len バイト読める。out は CC_SHA256_DIGEST_LENGTH（32）バイト書ける。
    unsafe { CC_SHA256(data.as_ptr().cast(), len, out.as_mut_ptr()) };
    out
}

/// macOS 以外ではアプリは動かない（窓は WKWebView 前提）。ライブラリだけを他の OS で
/// ビルドしたときに通るよう、ハッシュの無い（アプリの script も止まる）側に倒しておく。
#[cfg(not(target_os = "macos"))]
fn sha256(_data: &[u8]) -> [u8; 32] {
    [0u8; 32]
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFF: Policy = Policy { trusted: false };
    const ON: Policy = Policy { trusted: true };
    const ALL: [Policy; 2] = [OFF, ON];

    fn directive<'a>(csp: &'a str, name: &str) -> &'a str {
        csp.split("; ")
            .find(|d| d.split(' ').next() == Some(name))
            .unwrap_or_else(|| panic!("{name} が無い: {csp}"))
    }

    fn meta_content(p: &Policy) -> String {
        let m = meta(p, APP_ORIGIN);
        let start = m.find("content=\"").unwrap() + 9;
        m[start..m.len() - 2].replace("&#39;", "'").replace("&quot;", "\"")
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn sha256_matches_the_known_digest() {
        assert_eq!(hash_of("abc"), "ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=");
        assert_eq!(hash_of(""), "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=");
    }

    #[test]
    fn base64_pads_short_tails() {
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
    }

    #[test]
    fn the_index_gets_no_header_under_any_policy() {
        for p in ALL {
            assert_eq!(header(&p, Kind::Index), None, "{p:?}");
        }
    }

    #[test]
    fn other_responses_never_run_scripts() {
        for p in ALL {
            let h = header(&p, Kind::Other).unwrap();
            assert_eq!(directive(&h, "script-src"), "script-src 'none'", "{p:?}");
        }
    }

    #[test]
    fn documents_run_scripts_only_when_trusted() {
        assert_eq!(directive(&header(&OFF, Kind::Doc).unwrap(), "script-src"), "script-src 'none'");
        assert_eq!(
            directive(&header(&ON, Kind::Doc).unwrap(), "script-src"),
            "script-src 'self' 'unsafe-inline' 'unsafe-eval' data: blob: https: http:"
        );
    }

    #[test]
    fn the_page_meta_allows_only_its_own_scripts_and_the_two_libraries_by_default() {
        let c = meta_content(&OFF);
        let s = directive(&c, "script-src");
        assert!(!s.contains("'unsafe-inline'") && !s.contains("'self'"), "{s}");
        assert!(s.contains("'sha256-"), "{s}");
        for f in lib_files() {
            assert!(s.contains(&format!(" mdpreview://localhost/__lib/{f}")), "{s}");
        }
        let c = meta_content(&ON);
        let s = directive(&c, "script-src");
        assert!(s.starts_with("script-src 'self' 'unsafe-inline' 'unsafe-eval'"), "{s}");
        // ハッシュがあると 'unsafe-inline' が無視される。
        assert!(!s.contains("'sha256-"), "{s}");
    }

    #[test]
    fn production_meta_has_no_loopback_or_wildcard_by_default() {
        let c = meta_content(&OFF);
        assert!(!c.contains("http:") && !c.contains('*') && !c.contains("127.0.0.1"), "{c}");
    }

    #[test]
    fn trusting_adds_the_web_without_dropping_the_rest() {
        for (name, base) in [
            ("default-src", "'self' data: blob:"),
            ("img-src", "'self' data: blob:"),
            ("media-src", "'self' data: blob:"),
            ("font-src", "'self' data:"),
            ("style-src", "'self' 'unsafe-inline'"),
            ("frame-src", "'self'"),
            ("connect-src", "'self'"),
            ("object-src", "'self'"),
        ] {
            for csp in [meta_content(&OFF), header(&OFF, Kind::Doc).unwrap(), header(&OFF, Kind::Other).unwrap()] {
                assert_eq!(directive(&csp, name), format!("{name} {base}"));
            }
            for csp in [meta_content(&ON), header(&ON, Kind::Doc).unwrap(), header(&ON, Kind::Other).unwrap()] {
                assert_eq!(directive(&csp, name), format!("{name} {base} https: http:"));
            }
        }
    }

    #[test]
    fn base_and_form_targets_are_always_closed() {
        for p in ALL {
            for csp in [meta_content(&p), header(&p, Kind::Doc).unwrap(), header(&p, Kind::Other).unwrap()] {
                assert_eq!(directive(&csp, "base-uri"), "base-uri 'none'");
                assert_eq!(directive(&csp, "form-action"), "form-action 'none'");
            }
        }
    }

    #[test]
    fn the_meta_and_the_header_differ_only_in_script_src() {
        for p in ALL {
            let strip = |c: &str| c.split("; ").filter(|d| !d.starts_with("script-src")).collect::<Vec<_>>().join("; ");
            assert_eq!(strip(&meta_content(&p)), strip(&header(&p, Kind::Doc).unwrap()), "{p:?}");
            assert_eq!(strip(&meta_content(&p)), strip(&header(&p, Kind::Other).unwrap()), "{p:?}");
        }
    }
}
