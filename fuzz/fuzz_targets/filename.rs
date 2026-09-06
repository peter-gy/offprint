#![no_main]

use libfuzzer_sys::fuzz_target;
use offprint_artifact::portable_file_stem;

fuzz_target!(|data: &[u8]| {
    let title = String::from_utf8_lossy(data);
    let stem = portable_file_stem(&title);
    assert!(!stem.is_empty());
    assert!(stem.len() <= 96);
});
