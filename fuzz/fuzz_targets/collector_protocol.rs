#![no_main]

use libfuzzer_sys::fuzz_target;
use pageknot_protocol::{CollectorCommand, CollectorMessage};

fuzz_target!(|data: &[u8]| {
    let _message = serde_json::from_slice::<CollectorMessage>(data);
    let _command = serde_json::from_slice::<CollectorCommand>(data);
});
