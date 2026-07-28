#![no_main]

use libfuzzer_sys::fuzz_target;
use pageknot_document::{Document, serialize_document};

fuzz_target!(|data: &[u8]| {
    let document = Document::parse(data);
    if let Ok(serialized) = serialize_document(&document) {
        let reparsed = Document::parse(&serialized);
        let _serialized_again = serialize_document(&reparsed);
    }
});
