use std::error::Error;
use std::io;

use lopdf::{Dictionary, Document as PdfDocument, Object, Stream, dictionary, text_string};
use offprint_model::{ArtifactManifest, ContentDigest, ErrorStage};

use super::source::SourceMetadata;
use super::{embed_pdf_metadata, verify_offprint_pdf};
use crate::pdf::semantics::load_pdf;

type TestResult<T = ()> = std::result::Result<T, Box<dyn Error>>;

fn manifest() -> TestResult<ArtifactManifest> {
    Ok(serde_json::from_str(include_str!(
        "../../../../../schemas/examples/artifact-manifest.json"
    ))?)
}

fn tagged_pdf() -> TestResult<Vec<u8>> {
    let mut document = PdfDocument::with_version("1.7");
    let pages_id = document.new_object_id();
    let page_id = document.new_object_id();
    let content_id = document.add_object(Stream::new(Dictionary::new(), b"BT ET".to_vec()));
    let structure_element_id = document.add_object(dictionary! {
        "Type" => "StructElem",
        "S" => "P",
        "Pg" => page_id,
    });
    let structure_id = document.add_object(dictionary! {
        "Type" => "StructTreeRoot",
        "K" => vec![structure_element_id.into()],
    });
    let outlines_id = document.add_object(dictionary! {
        "Type" => "Outlines",
        "Count" => 0,
    });
    document.objects.insert(
        page_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Contents" => content_id,
            "StructParents" => 0,
        }),
    );
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Count" => 1,
            "Kids" => vec![page_id.into()],
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "StructTreeRoot" => structure_id,
        "MarkInfo" => dictionary! { "Marked" => true },
        "Outlines" => outlines_id,
    });
    document.trailer.set("Root", catalog_id);
    save(document)
}

fn enriched_pdf() -> TestResult<Vec<u8>> {
    let raw = tagged_pdf()?;
    let html = br#"<!doctype html><html lang="en"><head>
        <title>Semantic &amp; capture</title>
        <meta name="author" content="Grace Hopper">
        <meta name="description" content="A tagged PDF fixture">
        <meta name="keywords" content="metadata, semantics">
        </head><body><h1>Semantic capture</h1></body></html>"#;
    let digest = ContentDigest::sha256(html);
    Ok(embed_pdf_metadata(
        &raw,
        html,
        &manifest()?,
        digest,
        4 * 1024 * 1024,
    )?)
}

fn save(mut document: PdfDocument) -> TestResult<Vec<u8>> {
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    Ok(bytes)
}

fn mutate_info(bytes: &[u8], key: &str, value: &str) -> TestResult<Vec<u8>> {
    let mut document = load_pdf(bytes, ErrorStage::Verification)?;
    let info_id = document.trailer.get(b"Info")?.as_reference()?;
    document
        .get_object_mut(info_id)?
        .as_dict_mut()?
        .set(key, text_string(value));
    save(document)
}

fn mutate_catalog_language(bytes: &[u8], language: &str) -> TestResult<Vec<u8>> {
    let mut document = load_pdf(bytes, ErrorStage::Verification)?;
    document.catalog_mut()?.set("Lang", text_string(language));
    save(document)
}

fn mutate_xmp(
    bytes: &[u8],
    mutation: impl FnOnce(String) -> TestResult<String>,
) -> TestResult<Vec<u8>> {
    let mut document = load_pdf(bytes, ErrorStage::Verification)?;
    let metadata_id = document.catalog()?.get(b"Metadata")?.as_reference()?;
    let xmp = document
        .get_object(metadata_id)?
        .as_stream()?
        .get_plain_content()?;
    let xmp = mutation(String::from_utf8(xmp)?)?;
    document
        .get_object_mut(metadata_id)?
        .as_stream_mut()?
        .set_plain_content(xmp.into_bytes());
    save(document)
}

fn replace_xmp_text(xmp: String, name: &str, value: &str) -> TestResult<String> {
    let start = format!("<{name}>");
    let end = format!("</{name}>");
    let start_index = xmp
        .find(&start)
        .ok_or_else(|| io::Error::other(format!("XMP field {name} is missing")))?;
    let value_start = start_index + start.len();
    let value_end = xmp[value_start..]
        .find(&end)
        .map(|offset| value_start + offset)
        .ok_or_else(|| io::Error::other(format!("XMP field {name} is incomplete")))?;
    Ok(format!(
        "{}{}{}",
        &xmp[..value_start],
        value,
        &xmp[value_end..]
    ))
}

#[test]
fn source_metadata_uses_html_semantics_and_manifest_fallbacks() -> TestResult {
    let manifest = manifest()?;
    let source = SourceMetadata::from_html(
        r#"<!doctype html><html lang="de-AT"><head>
        <title>Die Übersicht &amp; Daten</title>
        <meta name="author" content="Ada Lovelace">
        <meta name="description" content="  Eine   Zusammenfassung. ">
        <meta name="keywords" content="charts, data; accessibility">
        <meta property="article:tag" content="PDF">
        <meta property="og:site_name" content="Example Research">
        <meta name="generator" content="StaticPress 2">
        <meta property="article:published_time" content="2026-07-29">
        </head><body><h1>Übersicht</h1></body></html>"#
            .as_bytes(),
        &manifest,
    );

    assert_eq!(source.title, "Die Übersicht & Daten");
    assert_eq!(source.authors, ["Ada Lovelace"]);
    assert_eq!(source.description.as_deref(), Some("Eine Zusammenfassung."));
    assert_eq!(source.keywords, ["charts", "data", "accessibility", "PDF"]);
    assert_eq!(source.language, "de-AT");
    assert_eq!(source.site_name.as_deref(), Some("Example Research"));
    assert_eq!(source.source_generator.as_deref(), Some("StaticPress 2"));
    assert_eq!(source.published_at.as_deref(), Some("2026-07-29"));
    Ok(())
}

#[test]
fn source_metadata_uses_the_first_html_head_title_and_ignores_svg_titles() -> TestResult {
    let source = SourceMetadata::from_html(
        br#"<!doctype html><html><head>
        <title>Document title</title>
        <title>Later document title</title>
        </head><body><svg><title>Chart title</title></svg></body></html>"#,
        &manifest()?,
    );

    assert_eq!(source.title, "Document title");
    Ok(())
}

#[test]
fn enriched_pdf_has_cross_checked_offprint_metadata() -> TestResult {
    let enriched = enriched_pdf()?;

    verify_offprint_pdf(&enriched)?;
    let document = load_pdf(&enriched, ErrorStage::Verification)?;
    let metadata = document
        .catalog()?
        .get(b"Metadata")
        .and_then(Object::as_reference)
        .and_then(|id| document.get_object(id))
        .and_then(Object::as_stream)
        .and_then(Stream::get_plain_content)?;
    let metadata = String::from_utf8(metadata)?;
    assert!(metadata.contains("<dc:title>"));
    assert!(metadata.contains("Semantic &amp; capture"));
    assert!(metadata.contains("Grace Hopper"));
    assert!(metadata.contains("<offprint:SourceArtifactSHA256>"));
    assert!(metadata.contains("urn:offprint:source-artifact:sha256:"));
    Ok(())
}

#[test]
fn verifier_rejects_independent_duplicate_field_mutations() -> TestResult {
    let enriched = enriched_pdf()?;
    let mutations = [
        (
            "Info source",
            mutate_info(&enriched, "Source", "https://other.example/")?,
        ),
        (
            "Info title",
            mutate_info(&enriched, "Title", "Different title")?,
        ),
        (
            "catalog language",
            mutate_catalog_language(&enriched, "fr")?,
        ),
        (
            "Dublin Core source",
            mutate_xmp(&enriched, |xmp| {
                replace_xmp_text(xmp, "dc:source", "https://other.example/")
            })?,
        ),
        (
            "source artifact digest",
            mutate_xmp(&enriched, |xmp| {
                replace_xmp_text(
                    xmp,
                    "offprint:SourceArtifactSHA256",
                    &"0".repeat(ContentDigest::HEX_LENGTH),
                )
            })?,
        ),
        (
            "semantic page count",
            mutate_xmp(&enriched, |xmp| {
                replace_xmp_text(xmp, "offprint:PageCount", "2")
            })?,
        ),
    ];

    for (name, mutated) in mutations {
        assert!(
            verify_offprint_pdf(&mutated).is_err(),
            "{name} mutation was accepted"
        );
    }
    Ok(())
}

#[test]
fn verifier_rejects_nested_semantic_values_and_wrong_namespaces() -> TestResult {
    let enriched = enriched_pdf()?;
    let nested = mutate_xmp(&enriched, |xmp| {
        replace_xmp_text(
            xmp,
            "offprint:PageCount",
            "<rdf:Bag><rdf:li>1</rdf:li></rdf:Bag>",
        )
    })?;
    let wrong_namespace = mutate_xmp(&enriched, |xmp| {
        Ok(xmp.replacen(
            "xmlns:dc=\"http://purl.org/dc/elements/1.1/\"",
            "xmlns:dc=\"https://invalid.example/dc\"",
            1,
        ))
    })?;

    assert!(verify_offprint_pdf(&nested).is_err());
    assert!(verify_offprint_pdf(&wrong_namespace).is_err());
    Ok(())
}

#[test]
fn offprint_pdf_verifier_requires_the_xmp_packet() -> TestResult {
    let enriched = enriched_pdf()?;
    let mut document = load_pdf(&enriched, ErrorStage::Verification)?;
    document.catalog_mut()?.remove(b"Metadata");
    let stripped = save(document)?;

    assert!(verify_offprint_pdf(&stripped).is_err());
    Ok(())
}
