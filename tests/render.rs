//! レンダリング出力のスナップショットテスト。
//!
//! `tests/fixtures/` を描画した結果を `tests/snapshots/` の期待値と丸ごと突き合わせる。
//! 個別の assert では拾えない「意図しない構造の変化」（属性の増減・要素の入れ子・
//! 描画経路ごとのラッパの違い）を捕まえるのが目的で、リファクタの安全網として使う。
//!
//! 期待値の更新:
//!   UPDATE_SNAPSHOTS=1 cargo test --test render
//! 更新したら必ず差分を目で確認すること（意図した変更かどうかはここでは判定できない）。
//!
//! inline された CSS / JS の中身は `…` に畳んでから比較する。base.css や hljs の
//! 中身まで固定すると、無関係な変更でスナップショットが落ちて役に立たなくなるため。

use std::path::{Path, PathBuf};

use md_preview::html::{build_folder_html, build_html, render_full_document};
use md_preview::request::{handle_request, render_html_iframe, source_view_html, RequestContext};

/// テーマ / ユーザー CSS は中身を固定しておく（テーマ側の変更で落ちないように）。
const THEME_CSS: &str = "/* theme */";
const CUSTOM_CSS: &str = "/* custom */";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .canonicalize()
        .expect("tests/fixtures が無い")
}

fn read_fixture(rel: &str) -> String {
    std::fs::read_to_string(fixtures().join(rel)).expect("フィクスチャを読めない")
}

// ── 正規化 ──────────────────────────────────────────────────────────

/// inline の `<style>` / `<script>` の中身を `…` に畳む。開きタグ（属性込み）と
/// 閉じタグは残すので、読み込み順や nonce の有無は引き続き固定される。
///
/// 中身に閉じタグ文字列が現れることは無い（現れたらブラウザ側でページが壊れるので、
/// そもそも成立しない）。よって単純な前方探索で十分。
fn fold_inline_assets(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 50);
    let mut rest = html;
    loop {
        let next = ["<style>", "<script"]
            .iter()
            .filter_map(|t| rest.find(t).map(|i| (i, *t)))
            .min_by_key(|(i, _)| *i);
        let Some((pos, tag)) = next else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];

        let close = if tag == "<style>" { "</style>" } else { "</script>" };
        let (Some(gt), Some(end)) = (rest.find('>'), rest.find(close)) else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..=gt]);
        if end > gt + 1 {
            out.push('…');
        }
        out.push_str(close);
        rest = &rest[end + close.len()..];
    }
}

/// nonce は起動時刻由来で毎回変わるので固定値に潰す。`nonce="…"`（script 属性）と
/// `'nonce-…'`（CSP ヘッダ）の両方に現れるので、続く 16 進の連なりをまとめて潰す。
fn mask_nonce(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("nonce") {
        out.push_str(&rest[..i]);
        rest = &rest[i + "nonce".len()..];
        // 区切り（`="` か `-`）を挟んで 16 進が続く形だけを対象にする。
        let sep_len = if rest.starts_with("=\"") { 2 } else if rest.starts_with('-') { 1 } else { 0 };
        if sep_len == 0 {
            out.push_str("nonce");
            continue;
        }
        let body = &rest[sep_len..];
        let hex_len = body.find(|c: char| !c.is_ascii_hexdigit()).unwrap_or(body.len());
        if hex_len == 0 {
            out.push_str("nonce");
            continue;
        }
        out.push_str("nonce");
        out.push_str(&rest[..sep_len]);
        out.push_str("NONCE");
        rest = &body[hex_len..];
    }
    out.push_str(rest);
    out
}

fn normalize(html: &str) -> String {
    let mut s = mask_nonce(&fold_inline_assets(html));
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

// ── 突き合わせ ──────────────────────────────────────────────────────

fn snapshot_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("{name}.txt"))
}

/// 最初に食い違った行を、前後の文脈つきで返す（落ちたときに原因を読めるように）。
fn first_diff(expected: &str, actual: &str) -> String {
    let exp: Vec<&str> = expected.lines().collect();
    let act: Vec<&str> = actual.lines().collect();
    let at = (0..exp.len().max(act.len()))
        .find(|&i| exp.get(i) != act.get(i))
        .unwrap_or(0);
    let from = at.saturating_sub(2);
    let mut s = format!("\n{} 行目から食い違っている:\n", at + 1);
    for i in from..(at + 3).min(exp.len().max(act.len())) {
        let mark = if i == at { ">>" } else { "  " };
        s.push_str(&format!("{mark} 期待 {:>4}| {}\n", i + 1, exp.get(i).unwrap_or(&"<無>")));
        s.push_str(&format!("{mark} 実際 {:>4}| {}\n", i + 1, act.get(i).unwrap_or(&"<無>")));
    }
    s
}

fn assert_snapshot(name: &str, actual: &str) {
    let actual = normalize(actual);
    let path = snapshot_path(name);

    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
        return;
    }

    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "スナップショットがありません: {}\n作成するには UPDATE_SNAPSHOTS=1 cargo test --test render",
            path.display()
        )
    });
    assert!(
        expected == actual,
        "スナップショット '{name}' が一致しません。{}\n\
         意図した変更なら UPDATE_SNAPSHOTS=1 cargo test --test render で更新すること。",
        first_diff(&expected, &actual)
    );
}

/// `handle_request` を叩いて本文を取り出す。ステータスも一緒に固定する。
fn request(query: &str, single_file: Option<&Path>) -> String {
    let (url_path, query) = match query.strip_prefix('/') {
        Some(_) => (query, ""),
        None => ("/", query),
    };
    let ctx = RequestContext {
        root_dir: fixtures(),
        index_html: b"<!-- index -->".to_vec(),
        theme_css: THEME_CSS.to_string(),
        custom_css: CUSTOM_CSS.to_string(),
        single_file: single_file.map(|p| p.to_path_buf()),
    };
    let resp = handle_request(&ctx, url_path, query);
    let status = resp.status().as_u16();
    let ctype = resp
        .headers()
        .get("Content-Type")
        .map(|v| v.to_str().unwrap_or("").to_string())
        .unwrap_or_default();
    format!(
        "status: {status}\ncontent-type: {ctype}\n----\n{}",
        String::from_utf8_lossy(resp.body())
    )
}

// ── ページ全体 ──────────────────────────────────────────────────────

#[test]
fn markdown_full_document() {
    let md = read_fixture("kitchen-sink.md");
    let dir = fixtures();
    let html = render_full_document(&md, "kitchen-sink.md", THEME_CSS, CUSTOM_CSS, Some(&dir));
    assert_snapshot("page-markdown", &html);
}

#[test]
fn source_view_page() {
    let path = fixtures().join("code/sample.rs");
    let text = read_fixture("code/sample.rs");
    let html = build_html(
        &source_view_html(&path, &text),
        "sample.rs",
        THEME_CSS,
        CUSTOM_CSS,
        "source-page",
    );
    assert_snapshot("page-source", &html);
}

#[test]
fn html_iframe_page() {
    let html = build_html(
        &render_html_iframe("page.html"),
        "page.html",
        THEME_CSS,
        CUSTOM_CSS,
        "html-page",
    );
    assert_snapshot("page-html-iframe", &html);
}

#[test]
fn folder_shell_page() {
    let html = build_folder_html("fixtures", THEME_CSS, CUSTOM_CSS, Some("kitchen-sink.md"));
    assert_snapshot("page-folder-shell", &html);
}

// ── フラグメント（フォルダモードの ?file= 経路） ────────────────────

#[test]
fn fragment_markdown() {
    assert_snapshot("fragment-markdown", &request("file=kitchen-sink.md", None));
}

#[test]
fn fragment_html_is_iframe() {
    assert_snapshot("fragment-html", &request("file=page.html", None));
}

#[test]
fn fragment_source() {
    assert_snapshot("fragment-source", &request("file=code%2Fsample.rs", None));
}

#[test]
fn fragment_binary() {
    assert_snapshot("fragment-binary", &request("file=blob.bin", None));
}

#[test]
fn fragment_raw() {
    assert_snapshot("fragment-raw", &request("raw=kitchen-sink.md", None));
}

#[test]
fn fragment_not_found() {
    assert_snapshot("fragment-not-found", &request("file=nope.md", None));
}

// ── フラグメント（単一ファイルモードの番兵経路） ────────────────────

#[test]
fn single_file_body_markdown() {
    let f = fixtures().join("kitchen-sink.md");
    assert_snapshot("single-body-markdown", &request("body=1", Some(&f)));
}

#[test]
fn single_file_body_source() {
    let f = fixtures().join("code/sample.rs");
    assert_snapshot("single-body-source", &request("body=1", Some(&f)));
}

#[test]
fn single_file_body_html() {
    let f = fixtures().join("page.html");
    assert_snapshot("single-body-html", &request("body=1", Some(&f)));
}

#[test]
fn single_file_raw() {
    let f = fixtures().join("kitchen-sink.md");
    assert_snapshot("single-raw", &request("raw=1", Some(&f)));
}

// ── JSON エンドポイント ─────────────────────────────────────────────

#[test]
fn dir_listing_json() {
    assert_snapshot("json-dir", &request("dir=", None));
}

#[test]
fn file_list_json() {
    assert_snapshot("json-files", &request("files=1", None));
}

// ── アセット配信 ────────────────────────────────────────────────────

#[test]
fn asset_html_gets_style_gate() {
    assert_snapshot("asset-html", &request("/page.html", None));
}

#[test]
fn asset_plain_text() {
    assert_snapshot("asset-text", &request("/embed.txt", None));
}

#[test]
fn index_is_served_verbatim() {
    assert_snapshot("asset-index", &request("/", None));
}
