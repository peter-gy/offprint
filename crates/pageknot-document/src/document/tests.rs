use crate::serialize_document;

use super::{Document, NodeData};

#[test]
fn parser_builds_stable_arena_ids_and_browser_corrected_structure() {
    let document = Document::parse(b"<!doctype html><title>x</title><p>one<p>two");
    let paragraphs = document
        .walk()
        .filter(|id| {
            matches!(
                document.node(*id).map(|node| &node.data),
                Some(NodeData::Element { name, .. }) if name.local.as_ref() == "p"
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(paragraphs.len(), 2);
    assert!(paragraphs[0].get() < paragraphs[1].get());
}

#[test]
fn frame_embedding_uses_the_parent_documents_light_dom_order() {
    let mut document = Document::parse(
        br#"<template shadowrootmode="open"><iframe src="shadow"></iframe></template>
            <iframe src="top"></iframe>"#,
    );

    assert!(document.embed_frame(0, "<h1>child</h1>").is_ok());
    let html = serialize_document(&document)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());

    assert!(
        html.as_ref()
            .is_some_and(|html| html.contains("src=\"shadow\""))
    );
    assert!(
        html.as_ref()
            .is_some_and(|html| html.contains("srcdoc=\"&lt;h1&gt;child&lt;/h1&gt;\""))
    );
    assert!(
        html.as_ref()
            .is_some_and(|html| !html.contains("src=\"top\""))
    );
}

#[test]
fn frame_embedding_follows_nested_owner_paths() {
    let mut document = Document::parse(
        br#"<iframe srcdoc="<p>root decoy</p>"></iframe>
            <iframe srcdoc="<iframe srcdoc='<p>nested decoy</p>'></iframe><iframe src='target'></iframe>"></iframe>"#,
    );

    let result = document.embed_captured_frame_at_path(
        &[1, 1],
        "<main>captured target</main>",
        pageknot_model::FrameId::new(9),
    );
    let outer_frames = document.inline_frames();
    let nested_frames = outer_frames
        .get(1)
        .map(|frame| Document::parse(frame.html.as_bytes()).inline_frames())
        .unwrap_or_default();

    assert!(result.is_ok());
    assert_eq!(nested_frames.len(), 2);
    assert!(nested_frames[0].html.contains("nested decoy"));
    assert!(!nested_frames[0].captured);
    assert!(nested_frames[1].html.contains("captured target"));
    assert!(nested_frames[1].captured);
}

#[test]
fn visual_fallback_resolution_reaches_nested_inline_documents() {
    let mut document = Document::parse(
        br#"<iframe srcdoc="<html><body><img data-pageknot-visual-fallback='inline-0'></body></html>"></iframe>"#,
    );

    let resolved =
        document.resolve_visual_fallback("inline-0", "data:image/png;base64,cGFnZWtub3Q=");
    let html = serialize_document(&document)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());

    assert!(resolved.is_ok());
    assert!(
        html.as_ref()
            .is_some_and(|html| html.contains("data:image/png;base64,cGFnZWtub3Q="))
    );
    assert!(
        html.as_ref()
            .is_some_and(|html| !html.contains("data-pageknot-visual-fallback"))
    );
}
