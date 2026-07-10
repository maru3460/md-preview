#![no_main]
//! Markdown 本文→HTML 変換。任意のバイト列（UTF-8 に補正）を食わせ、パニック・
//! 無限ループ・過大メモリが起きないことを確認する。pulldown-cmark と自前の
//! transform_events（alert / mermaid / drawio / filename コードブロック）が対象。
use libfuzzer_sys::fuzz_target;
use md_preview::html::render_body;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    let _ = render_body(&s);
});
