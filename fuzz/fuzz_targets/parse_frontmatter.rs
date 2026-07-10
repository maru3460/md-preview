#![no_main]
//! frontmatter 分離。`---` の開始/終了検出とバイト境界のスライス処理が、任意入力で
//! パニック（非文字境界スライス等）しないことを確認する。
use libfuzzer_sys::fuzz_target;
use md_preview::html::parse_frontmatter;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    let _ = parse_frontmatter(&s);
});
