#![no_main]
//! パス結合の安全判定。相対パス文字列に対する `..`（ParentDir）検出と正規化後の
//! ルート内チェックが、任意入力（NUL・非 UTF-8・エンコード済み区切り等）で
//! パニックしないことを確認する。ディレクトリトラバーサル防御の要。
use libfuzzer_sys::fuzz_target;
use md_preview::request::safe_join;

fuzz_target!(|data: &[u8]| {
    let rel = String::from_utf8_lossy(data);
    // canonicalize が実在パスに触れるので、固定の一時ディレクトリを root にする。
    let root = std::env::temp_dir();
    let _ = safe_join(&root, &rel);
});
