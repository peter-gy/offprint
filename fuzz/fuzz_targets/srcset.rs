#![no_main]

use libfuzzer_sys::fuzz_target;
use pageknot_document::{Document, discover_document_resources};
use url::Url;

fuzz_target!(|data: &[u8]| {
    let value = String::from_utf8_lossy(data).replace('"', "&quot;");
    let document = Document::parse(
        format!("<!doctype html><img srcset=\"{value}\">").as_bytes(),
    );
    let Ok(base) = Url::parse("https://example.test/base/") else {
        return;
    };
    let _resources = discover_document_resources(&document, &base);
});
