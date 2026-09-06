#![no_main]

use std::collections::BTreeMap;

use libfuzzer_sys::fuzz_target;
use offprint_document::discover_css_resources;
use url::Url;

fuzz_target!(|data: &[u8]| {
    let source = String::from_utf8_lossy(data);
    let Ok(base) = Url::parse("https://example.test/base/") else {
        return;
    };
    if let Ok(resources) = discover_css_resources(&source, &base) {
        let replacements = resources
            .resources()
            .iter()
            .map(|resource| {
                (
                    resource.id,
                    "data:image/gif;base64,R0lGODlhAQABAAAAACw=".to_owned(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let _rewritten = resources.rewrite(&replacements);
    }
});
