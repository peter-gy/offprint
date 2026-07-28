use serde::Serialize;
use serde::de::DeserializeOwned;

pub(crate) trait CdpCommand {
    type Params: Serialize;
    type Response: DeserializeOwned;

    const METHOD: &'static str;
}

pub(crate) trait CdpEventMessage: DeserializeOwned {
    const METHOD: &'static str;
}

pub(crate) mod generated {
    include!("cdp_generated.rs");
}
