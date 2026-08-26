//! ファイルの実パスと、WebView が引く URL / `?file=` の識別子との変換をまとめる。
//!
//! 3 つの表現が行き来する。
//!
//! - **実パス** — 正規化済みの絶対パス。ファイルを読むときだけ使う。
//! - **URL** — `mdpreview://localhost` からの絶対 URL パス。root 配下なら `/docs/fig.png`、
//!   root の外なら [`ABS_PREFIX`] を冠した `/__abs/Users/me/fig.png`。`/__abs/` 配下は
//!   実パスの階層をそのまま写しているので、iframe 内の相対参照も素直に解決される。
//! - **識別子** — `?file=` / `?raw=` / タブ・サイドバーが持つ文字列。root 配下なら
//!   root 相対パス（先頭 `/` 無し）、root の外なら絶対パス（先頭 `/` あり）。
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
pub fn file_id(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => path.to_string_lossy().into_owned(),
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
    fn file_id_switches_between_relative_and_absolute() {
        let root = PathBuf::from("/proj");
        assert_eq!(file_id(&root, Path::new("/proj/docs/a.md")), "docs/a.md");
        assert_eq!(file_id(&root, Path::new("/other/x.md")), "/other/x.md");
    }
}
