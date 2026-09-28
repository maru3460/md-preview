//! ファイルの実パスと、WebView が引く URL / `?file=` の識別子との変換をまとめる。
//!
//! 3 つの表現が行き来する。
//!
//! - **実パス** — 正規化済みの絶対パス。ファイルを読むときだけ使う。
//! - **URL** — `mdpreview://localhost` からの絶対 URL パス。root 配下なら `/docs/fig.png`、
//!   root の外なら [`ABS_PREFIX`] を冠した `/__abs/Users/me/fig.png`。`/__abs/` 配下は
//!   実パスの階層をそのまま写しているので、iframe 内の相対参照も素直に解決される。
//! - **識別子** — `?file=` / `?raw=` / `?dir=` / タブ・サイドバーが持つ文字列。
//!   **常に絶対パス**で、root の内も外も同じ形。
//!
//! 相対パスの基準は**描画中のファイルがある場所**であって root ではない。
//! この基準を持つのが [`DocBase`] で、md 中の `src` / `href` はここを通して URL へ畳む。

use std::path::{Component, Path, PathBuf};

/// root の外にあるファイルを配信する URL の接頭辞。
pub const ABS_PREFIX: &str = "/__abs/";

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = from_hex(bytes[i + 1]);
            let lo = from_hex(bytes[i + 2]);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// パスを URL パスとしてエンコードする（`/` 区切りは残す）。
/// 空白・非ASCII・記号を percent-encode する。
pub fn encode_path(rel: &str) -> String {
    let mut out = String::with_capacity(rel.len());
    for b in rel.bytes() {
        match b {
            b'/' | b'-' | b'_' | b'.' | b'~'
            | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// `.` / `..` を語彙的に畳む。`canonicalize` と違いファイルの存在を要求しないので、
/// まだ無い画像を指す `src` でも URL を組める。ルートより上へは出ない。
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 実パス → URL。root 配下かどうかで `/rel` と `/__abs/abs` を出し分ける。
pub fn asset_url(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel) => format!("/{}", encode_path(&rel.to_string_lossy())),
        Err(_) => format!(
            "{}{}",
            ABS_PREFIX,
            encode_path(path.to_string_lossy().trim_start_matches('/'))
        ),
    }
}

/// 実パス → 識別子（`?file=` に載せる文字列）。
///
/// **識別子は絶対パスそのもの**。root 相対にしないのは、root が動いた瞬間に
/// 同じ文字列が別のファイルを指してしまうため（#33）。タブの同一判定は文字列一致
/// なので、1 経路でも別の形を混ぜると同じファイルがタブ 2 枚になる。
///
/// ここで `canonicalize` は呼ばない。**正規化するのは外から入ってきたパスだけ**で、
/// それは入口（`resolve_arg_path` / [`crate::request::id_to_path`] / 監視の
/// `ev.path`）が済ませている。一覧（ツリー・⌘P・git）が渡してくるのは正規化済みの
/// root から `join` で降りたパスなので、もう一度解決し直す必要が無い。
///
/// この割り切りは root の中のシンボリックリンクで 2 つ取りこぼす。**どちらも
/// #33 以前から同じ形で起きていたもので、新しい退行ではない。**
///
/// 1. **ファイルへのリンク** — 識別子はリンク側のパスのままになり、監視が返す
///    実体側のパスと一致しないのでホットリロードが効かない。ここで `canonicalize`
///    して実体側に寄せると、今度はツリーの行（リンク側）とタブの識別子（実体側）が
///    食い違ってハイライトが外れる。どちらも取れないので、ツリーの見た目に合う方を
///    残してある。
/// 2. **ディレクトリへのリンク** — `resolve_tree_dir` が `id_to_path` で
///    canonicalize してから中を並べるので、`link/` を展開して出てくる子の識別子は
///    実体側になる。一方で本文リンク `[x](link/y.md)` は JS の `urlToId` が
///    リンク側で組むため、同じファイルがタブ 2 枚になる。
///
/// 直すなら「一覧を作るときも `canonicalize` する」ではなく「ツリーの展開でも
/// リンク側のパスを保つ」側に寄せること。前者は 1 の見え方を壊す。
pub fn file_id(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// 識別子 → 画面に出す名前。root 配下なら root 相対、外なら絶対パスのまま。
///
/// 識別子をそのまま出すと ⌘P の全行に `/Users/<name>/…` が並ぶ。表示だけをここで
/// 畳み、識別子そのものには手を付けない。JS 側の対（`MdCommon.idToDisplay`）と
/// **同じ結果を返すこと**。片方だけ直すと、サーバが出したラベルと画面が組んだ
/// ラベルが食い違う。
///
/// `strip_prefix` に任せず前方一致で書いているのは、揃えたい端が 2 つあるため。
/// root 自身を渡されたら空文字ではなく識別子をそのまま返す（名前の無いラベルを
/// 画面に出さない）。root が `/` のときも配下を剥ぐ。
pub fn display_id(root: &Path, id: &str) -> String {
    let root = root.to_string_lossy();
    // 空の root を「`/` が root」と読まない。読むと、あらゆる絶対パスの先頭 `/` を
    // 剥いでしまう（JS 側は空の root を「剥ぐものが無い」と扱うので、そこでも食い違う）。
    if root.is_empty() {
        return id.to_string();
    }
    let prefix = if root.ends_with('/') { root.to_string() } else { format!("{}/", root) };
    match id.strip_prefix(&prefix) {
        Some(rel) if !rel.is_empty() => rel.to_string(),
        _ => id.to_string(),
    }
}

/// `http:` や `mailto:` のようにスキームを持つ URL か。
/// 先頭が英字で、`:` までが英数字 `+ - .` だけで出来ているものをスキーム付きとみなす。
fn has_scheme(url: &str) -> bool {
    let bytes = url.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    for (i, b) in bytes.iter().enumerate() {
        match b {
            b':' => return i > 0,
            b if b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.') => {}
            _ => return false,
        }
    }
    false
}

/// 相対 URL を解決する基準。`dir` は描画中のファイルがあるディレクトリ、
/// `root` は配信ルート（URL をどちらの形に畳むかの判定に使う）。
pub struct DocBase<'a> {
    pub dir: &'a Path,
    pub root: &'a Path,
}

impl<'a> DocBase<'a> {
    pub fn new(dir: &'a Path, root: &'a Path) -> Self {
        DocBase { dir, root }
    }

    /// md 中の `src` / `href` を WebView が引ける URL へ書き換える。
    /// 書き換える必要が無いもの（スキーム付き・ページ内アンカー・既に絶対 URL）は None。
    ///
    /// 値は percent-decode してから実パスに畳み、改めて encode し直す。
    /// 生の空白や日本語で書かれた `src` もこの一往復で正しい URL になる。
    pub fn resolve_url(&self, url: &str) -> Option<String> {
        if url.is_empty() || url.starts_with('#') || url.starts_with('/') || has_scheme(url) {
            return None;
        }
        let (path_part, frag) = match url.find('#') {
            Some(i) => (&url[..i], &url[i..]),
            None => (url, ""),
        };
        if path_part.is_empty() {
            return None;
        }
        let abs = normalize(&self.dir.join(percent_decode(path_part)));
        Some(format!("{}{}", asset_url(self.root, &abs), frag))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base<'a>(dir: &'a str, root: &'a str) -> (PathBuf, PathBuf) {
        (PathBuf::from(dir), PathBuf::from(root))
    }

    #[test]
    fn relative_src_resolves_against_the_document_not_the_root() {
        let (dir, root) = base("/proj/docs", "/proj");
        let b = DocBase::new(&dir, &root);
        assert_eq!(b.resolve_url("fig.png").unwrap(), "/docs/fig.png");
        assert_eq!(b.resolve_url("./fig.png").unwrap(), "/docs/fig.png");
        assert_eq!(b.resolve_url("../fig.png").unwrap(), "/fig.png");
        assert_eq!(b.resolve_url("sub/fig.png").unwrap(), "/docs/sub/fig.png");
    }

    #[test]
    fn out_of_root_gets_the_abs_prefix() {
        let (dir, root) = base("/proj/docs", "/proj");
        let b = DocBase::new(&dir, &root);
        assert_eq!(b.resolve_url("../../assets/fig.png").unwrap(), "/__abs/assets/fig.png");
    }

    #[test]
    fn absolute_and_scheme_urls_are_left_alone() {
        let (dir, root) = base("/proj/docs", "/proj");
        let b = DocBase::new(&dir, &root);
        for url in ["https://example.com/a.png", "mailto:a@b.c", "data:image/png;base64,AA", "#sec", "/abs.png", ""] {
            assert!(b.resolve_url(url).is_none(), "{url} を書き換えてはいけない");
        }
    }

    #[test]
    fn fragment_survives_the_rewrite() {
        let (dir, root) = base("/proj/docs", "/proj");
        let b = DocBase::new(&dir, &root);
        assert_eq!(b.resolve_url("./b.md#sec").unwrap(), "/docs/b.md#sec");
    }

    #[test]
    fn spaces_and_non_ascii_are_encoded_once() {
        let (dir, root) = base("/proj/docs", "/proj");
        let b = DocBase::new(&dir, &root);
        assert_eq!(b.resolve_url("my fig.png").unwrap(), "/docs/my%20fig.png");
        // 既に encode 済みの値を二重にエンコードしない。
        assert_eq!(b.resolve_url("my%20fig.png").unwrap(), "/docs/my%20fig.png");
    }

    #[test]
    fn file_id_is_the_absolute_path_inside_or_outside_the_root() {
        assert_eq!(file_id(Path::new("/proj/docs/a.md")), "/proj/docs/a.md");
        assert_eq!(file_id(Path::new("/other/x.md")), "/other/x.md");
    }

    #[test]
    fn display_id_strips_the_root_but_leaves_the_outside_alone() {
        let root = PathBuf::from("/proj");
        assert_eq!(display_id(&root, "/proj/docs/a.md"), "docs/a.md");
        assert_eq!(display_id(&root, "/other/x.md"), "/other/x.md");
        // root 自身は名前が無くなるので、そのまま返す。
        assert_eq!(display_id(&root, "/proj"), "/proj");
        // 名前の一部が root に一致するだけのものを剥がない。
        assert_eq!(display_id(&root, "/project/x.md"), "/project/x.md");
    }

    /// `md /` のときも配下を剥ぐこと。root + "/" を素朴に組むと "//" になり、
    /// 1 つも一致しなくなる（JS 側 `MdCommon.idToDisplay` と揃えるための回帰）。
    #[test]
    fn display_id_handles_the_filesystem_root() {
        let root = PathBuf::from("/");
        assert_eq!(display_id(&root, "/Applications/x.md"), "Applications/x.md");
        assert_eq!(display_id(&root, "/"), "/");
        // 空の root を `/` と読まない（JS 側は「剥ぐものが無い」と扱う）。
        assert_eq!(display_id(Path::new(""), "/Applications/x.md"), "/Applications/x.md");
    }
}
