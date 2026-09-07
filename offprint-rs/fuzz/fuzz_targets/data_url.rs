#![no_main]

use data_url::DataUrl;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = std::str::from_utf8(data)
        && let Ok(url) = DataUrl::process(value)
    {
        let _decoded = url.decode_to_vec();
    }
});
