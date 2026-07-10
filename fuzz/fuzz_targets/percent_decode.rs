#![no_main]
//! パーセントデコード。`%xx` の途中打ち切りや不正 16 進、非 UTF-8 バイト列で
//! パニックしないことを確認する。URL クエリ経由で外部入力が直接届く経路。
use libfuzzer_sys::fuzz_target;
use md_preview::request::percent_decode;

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    let _ = percent_decode(&s);
});
