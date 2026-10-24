//! docguard.js（md の本文の `<style>` を本文の枠の中に閉じ込める）が置いている前提。
//!
//! 文書の `@keyframes` と `@counter-style` は `@scope` の中でも名前を document 全体に
//! 定義するが、docguard.js はそれを残している。アプリが同じ種類の名前を 1 つも定義して
//! いなければ、文書の定義はアプリの見た目を変えられない、という前提に乗っている。
//! アプリの CSS（`src/` の CSS と、CSS を文字列で持つ JS・Rust）にどちらかが現れたら
//! ここで落ちる。足すなら、docguard.js で文書の側の名前を分けるか、規則ごと落とすこと。
//!
//! 同梱のライブラリ（`src/assets/vendor/`）は数えない。mermaid は図の `<style>` に
//! `@keyframes` を書くが、それは本文の中の図の見た目で、アプリの UI ではない。

use std::fs;
use std::path::{Path, PathBuf};

const AT_RULES: [&str; 2] = ["@keyframes", "@counter-style"];

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read_dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "vendor") {
                continue;
            }
            walk(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e == "css" || e == "js" || e == "rs" || e == "html")
        {
            out.push(path);
        }
    }
}

#[test]
fn app_sources_define_no_keyframes_or_counter_styles() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&src, &mut files);
    // 数えた範囲が空になっていないこと（置き場所が変わって素通りするのを防ぐ）。
    assert!(
        files.iter().any(|p| p.ends_with("assets/css/base.css")),
        "src/assets/css/base.css が見つからない: {files:?}"
    );
    assert!(
        files.iter().any(|p| p.starts_with(src.join("assets/themes"))),
        "src/assets/themes/ の CSS が見つからない"
    );

    let mut hits = Vec::new();
    for path in &files {
        // docguard.js 自身は説明のために名前を書いている。
        if path.ends_with("assets/js/docguard.js") {
            continue;
        }
        let text = fs::read_to_string(path).expect("read");
        let lower = text.to_ascii_lowercase();
        for at in AT_RULES {
            if lower.contains(at) {
                hits.push(format!("{} に {at}", path.strip_prefix(&src).unwrap().display()));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "アプリの CSS が {hits:?} を定義している。docguard.js は文書の @keyframes / \
         @counter-style を名前のまま残しているので、同じ名前で上書きされうる。\
         docguard.js の LEAVES を見直すこと"
    );
}
