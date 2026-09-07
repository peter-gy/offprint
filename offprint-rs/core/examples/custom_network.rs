use std::collections::BTreeSet;

use offprint::{NetworkPolicy, NetworkRules, Offprint};

#[tokio::main]
async fn main() -> offprint::Result<()> {
    let network = NetworkPolicy::Custom(NetworkRules {
        allowed_hosts: BTreeSet::from(["capture.internal".to_owned()]),
        allowed_cidrs: BTreeSet::from(["10.20.0.0/16".to_owned()]),
        allow_loopback: false,
        allow_private: false,
        allow_link_local: false,
    });

    let offprint = Offprint::new()?;
    let capture = offprint.capture("https://example.com")?.network(network);
    drop(capture);
    offprint.close().await?;
    Ok(())
}
