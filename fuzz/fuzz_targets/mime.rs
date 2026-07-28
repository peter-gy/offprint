#![no_main]

use data_url::DataUrl;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let media_type = String::from_utf8_lossy(data);
    let candidate = format!("data:{media_type},x");
    if let Ok(url) = DataUrl::process(&candidate) {
        let _mime = url.mime_type();
    }
});
