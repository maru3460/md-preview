//! 相対パスの基準がドキュメントの場所であること、および root の外を開けることの回帰テスト。
//!
//! 元の壊れ方（issue #10 / #11）:
//!   - `docs/a.md` の `![](fig.png)` が root 直下の `/fig.png` を引きに行って 404 になる
//!     （フォルダモードのページ URL は常に root なので、ブラウザが root 基準で解決する）
//!   - root の外を指すリンクが開けない（`safe_join` が `..` と root 外を二重で弾く）
//!
//! ここでは実際に一時ディレクトリを掘り、`handle_request` を叩いて確かめる。

use std::path::{Path, PathBuf};

use md_preview::request::{handle_request, RequestContext};

/// テスト用のツリーを掘る。
///
/// ```text
/// <tmp>/proj/README.md
/// <tmp>/proj/docs/a.md      ← 相対参照はここが基準
/// <tmp>/proj/docs/fig.png
/// <tmp>/proj/docs/b.md
/// <tmp>/proj/docs/page.html
/// <tmp>/outside/x.md        ← root の外
/// <tmp>/outside/out.png
/// ```
fn fixture(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("md-relpath-{}", name));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("proj");
    let outside = base.join("outside");
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::create_dir_all(&outside).unwrap();

    std::fs::write(root.join("README.md"), "# root\n").unwrap();
    std::fs::write(root.join("docs/fig.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    std::fs::write(root.join("docs/b.md"), "# b\n").unwrap();
    std::fs::write(root.join("docs/page.html"), "<p>page</p>").unwrap();
    std::fs::write(outside.join("out.png"), b"\x89PNG\r\n\x1a\n").unwrap();
    std::fs::write(outside.join("x.md"), "# x\n\n![out](out.png)\n").unwrap();
    std::fs::write(
        root.join("docs/a.md"),
        concat!(
            "# doc\n\n",
            "![fig](fig.png)\n\n",
            "[b](./b.md)\n\n",
            "![out](../../outside/out.png)\n\n",
            "<img src=\"fig.png\" width=\"300\">\n\n",
            "[外](../../outside/x.md)\n",
        ),
    )
    .unwrap();

    (root.canonicalize().unwrap(), outside.canonicalize().unwrap())
}

struct Resp {
    status: u16,
    body: String,
}

fn get(root: &Path, url_path: &str, query: &str) -> Resp {
    let ctx = RequestContext {
        root_dir: root.to_path_buf(),
        index_html: b"<!-- index -->".to_vec(),
        theme_css: String::new(),
        custom_css: String::new(),
        single_file: None,
    };
    let resp = handle_request(&ctx, url_path, query);
    Resp {
        status: resp.status().as_u16(),
        body: String::from_utf8_lossy(resp.body()).into_owned(),
    }
}

fn view(root: &Path, id: &str) -> Resp {
    get(root, "/", &format!("file={}", id))
}

#[test]
fn relative_image_resolves_against_the_document_dir() {
    let (root, _) = fixture("image");
    let body = view(&root, "docs/a.md").body;
    // 素の相対 src がドキュメント基準の URL に畳まれていること。
    assert!(body.contains(r#"src="/docs/fig.png""#), "{body}");
    // root 直下を引きに行っていないこと（これが元の壊れ方）。
    assert!(!body.contains(r#"src="fig.png""#), "書き換えられていない: {body}");
    // 畳んだ URL が実際に配信できること。
    assert_eq!(get(&root, "/docs/fig.png", "").status, 200);
}

#[test]
fn relative_link_resolves_against_the_document_dir() {
    let (root, _) = fixture("link");
    let body = view(&root, "docs/a.md").body;
    assert!(body.contains(r#"href="/docs/b.md""#), "{body}");
}

#[test]
fn raw_html_img_is_rewritten_too() {
    let (root, _) = fixture("rawhtml");
    let body = view(&root, "docs/a.md").body;
    // md 中に直書きした <img> も同じ基準で畳む（幅指定などでよく使われる）。
    assert!(body.contains(r#"<img src="/docs/fig.png" width="300">"#), "{body}");
}

#[test]
fn out_of_root_image_goes_through_the_abs_route() {
    let (root, outside) = fixture("absimage");
    let body = view(&root, "docs/a.md").body;
    let url = format!("/__abs{}/out.png", outside.to_string_lossy());
    assert!(body.contains(&format!(r#"src="{}""#, url)), "{body}\nexpected {url}");
    // その URL で実際に画像が返ること。
    let img = get(&root, &url, "");
    assert_eq!(img.status, 200, "root の外の画像が配信できない");
}

#[test]
fn out_of_root_file_opens_by_absolute_id() {
    let (root, outside) = fixture("absfile");
    let x = outside.join("x.md");
    // 本文のリンクは /__abs/ 付きの URL に畳まれている。
    let body = view(&root, "docs/a.md").body;
    assert!(body.contains(&format!(r#"href="/__abs{}""#, x.to_string_lossy())), "{body}");

    // JS はそれを絶対パスの識別子に戻して ?file= に載せる。
    let opened = view(&root, &x.to_string_lossy());
    assert_eq!(opened.status, 200, "root の外のファイルが開けない");
    assert!(opened.body.contains("<h1"), "{}", opened.body);

    // raw / diff も同じ識別子で通ること。
    assert_eq!(get(&root, "/", &format!("raw={}", x.to_string_lossy())).status, 200);
    assert_eq!(get(&root, "/", &format!("diffstat={}", x.to_string_lossy())).status, 200);
}

#[test]
fn out_of_root_document_resolves_its_own_relative_images() {
    let (root, outside) = fixture("absown");
    let x = outside.join("x.md");
    let body = view(&root, &x.to_string_lossy()).body;
    let url = format!("/__abs{}/out.png", outside.to_string_lossy());
    assert!(body.contains(&format!(r#"src="{}""#, url)), "{body}\nexpected {url}");
}

#[test]
fn html_iframe_src_points_at_the_document_dir() {
    let (root, _) = fixture("iframe");
    let body = view(&root, "docs/page.html").body;
    assert!(body.contains(r#"src="/docs/page.html""#), "{body}");
}

#[test]
fn the_sidebar_tree_still_stops_at_the_root() {
    let (root, outside) = fixture("tree");
    // ツリー（?dir= / ?has_md=）は root の中だけ。root の外は開く経路を持たせない。
    assert_eq!(get(&root, "/", "dir=../outside").status, 404);
    assert_eq!(
        get(&root, "/", &format!("dir={}", outside.to_string_lossy())).status,
        404
    );
    // アセットの素の traversal も従来どおり弾く。
    assert_eq!(get(&root, "/../outside/out.png", "").status, 404);
}
