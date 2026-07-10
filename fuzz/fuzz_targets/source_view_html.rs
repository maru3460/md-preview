#![no_main]
//! 非 md ファイルのソースビュー生成。任意テキストを content として与え、HTML
//! エスケープ・巨大判定（バイト数/行数カウント）がパニックしないことを確認する。
//! パスは固定（拡張子による言語判定は別で十分カバーされる）。
use libfuzzer_sys::fuzz_target;
use md_preview::request::source_view_html;
use std::path::Path;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    let _ = source_view_html(Path::new("fuzz_input.rs"), &s);
});
