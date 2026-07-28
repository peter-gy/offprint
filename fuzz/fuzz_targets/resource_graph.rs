#![no_main]

use libfuzzer_sys::fuzz_target;
use pageknot_document::{
    RenderingRole, ResourceGraph, ResourceLocationKind, ResourceReference,
};
use pageknot_model::{FrameId, NodeId};
use url::Url;

fuzz_target!(|data: &[u8]| {
    let source = String::from_utf8_lossy(data);
    let Ok(base) = Url::parse("https://example.test/base/") else {
        return;
    };
    if let Ok(resolved_url) = base.join(&source) {
        let mut graph = ResourceGraph::default();
        let _id = graph.discover(ResourceReference {
            frame_id: FrameId::new(1),
            node_id: NodeId::new(1),
            location: ResourceLocationKind::HtmlAttribute,
            original: source.into_owned(),
            base_url: base,
            resolved_url,
            role: RenderingRole::Other,
        });
        let _complete = graph.validate_complete();
    }
});
