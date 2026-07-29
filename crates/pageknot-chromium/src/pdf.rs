use base64::Engine as _;
use pageknot_model::{ErrorStage, PageKnotError, Result};
use serde_json::{Value, json};
use url::Url;

use crate::CdpClient;

const PDF_READ_BYTES: u64 = 64 * 1024;
const PREPARE_PDF_LINKS: &str = r#"(sourceUrl) => {
    let rewritten = 0;
    let removed = 0;
    const schemes = new Set(["http:", "https:", "mailto:", "tel:"]);
    const visited = new Set();
    const visit = (root) => {
        if (!root || visited.has(root)) return;
        visited.add(root);
        for (const element of root.querySelectorAll("*")) {
            if (
                (element.localName === "a" || element.localName === "area") &&
                element.hasAttribute("href")
            ) {
                try {
                    const target = new URL(element.getAttribute("href"), sourceUrl);
                    if (schemes.has(target.protocol)) {
                        element.setAttribute("href", target.href);
                        rewritten += 1;
                    } else {
                        element.removeAttribute("href");
                        removed += 1;
                    }
                } catch {
                    element.removeAttribute("href");
                    removed += 1;
                }
            }
            visit(element.shadowRoot);
            if (element.localName === "iframe" || element.localName === "frame") {
                try {
                    visit(element.contentDocument);
                } catch {}
            }
        }
    };
    visit(document);
    return { rewritten, removed };
}"#;

pub(crate) fn prepare_pdf_links_expression(source_url: &Url) -> String {
    let source_url = serde_json::Value::String(source_url.as_str().to_owned());
    format!("({PREPARE_PDF_LINKS})({source_url})")
}

pub(crate) async fn print_to_pdf(
    client: CdpClient,
    session_id: String,
    landscape: bool,
    prefer_css_page_size: bool,
    maximum_bytes: u64,
) -> Result<Vec<u8>> {
    tokio::spawn(async move {
        print_to_pdf_owned(
            &client,
            &session_id,
            landscape,
            prefer_css_page_size,
            maximum_bytes,
        )
        .await
    })
    .await
    .map_err(|error| {
        PageKnotError::new(
            "pageknot.export.pdf",
            ErrorStage::Encoding,
            format!("PDF export task failed: {error}"),
        )
    })?
}

async fn print_to_pdf_owned(
    client: &CdpClient,
    session_id: &str,
    landscape: bool,
    prefer_css_page_size: bool,
    maximum_bytes: u64,
) -> Result<Vec<u8>> {
    let response = client
        .command(
            "Page.printToPDF",
            json!({
                "landscape": landscape,
                "printBackground": true,
                "preferCSSPageSize": prefer_css_page_size,
                "generateTaggedPDF": true,
                "generateDocumentOutline": true,
                "transferMode": "ReturnAsStream",
            }),
            Some(session_id),
        )
        .await?;
    let handle = response
        .get("stream")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            PageKnotError::new(
                "pageknot.export.pdf",
                ErrorStage::Encoding,
                "PDF response has no document stream",
            )
        })?
        .to_owned();
    let result = read_pdf_stream(client, session_id, &handle, maximum_bytes).await;
    let close = client
        .command("IO.close", json!({"handle": handle}), Some(session_id))
        .await;
    match result {
        Err(error) => Err(error),
        Ok(bytes) => {
            close.map_err(|error| {
                PageKnotError::new(
                    "pageknot.export.pdf",
                    ErrorStage::Encoding,
                    "failed to close the PDF document stream",
                )
                .with_detail("cause", error.code.as_str())
            })?;
            Ok(bytes)
        }
    }
}

async fn read_pdf_stream(
    client: &CdpClient,
    session_id: &str,
    handle: &str,
    maximum_bytes: u64,
) -> Result<Vec<u8>> {
    let read_limit = maximum_bytes.saturating_add(1);
    let mut bytes = Vec::new();
    loop {
        let received = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let request_bytes = read_limit.saturating_sub(received).clamp(1, PDF_READ_BYTES);
        let response = client
            .command(
                "IO.read",
                json!({"handle": handle, "size": request_bytes}),
                Some(session_id),
            )
            .await?;
        let data = response
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                PageKnotError::new(
                    "pageknot.export.pdf",
                    ErrorStage::Encoding,
                    "PDF stream returned no data field",
                )
            })?;
        let chunk = if response
            .get("base64Encoded")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|error| {
                    PageKnotError::new(
                        "pageknot.export.pdf",
                        ErrorStage::Encoding,
                        format!("PDF stream contains invalid base64: {error}"),
                    )
                })?
        } else {
            data.as_bytes().to_vec()
        };
        let attempted = received.saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        if attempted > maximum_bytes {
            return Err(PageKnotError::new(
                "pageknot.artifact.size",
                ErrorStage::Encoding,
                "PDF output exceeds the configured byte limit",
            )
            .with_detail("attempted", attempted)
            .with_detail("limit", maximum_bytes));
        }
        let eof = response
            .get("eof")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if chunk.is_empty() && !eof {
            return Err(PageKnotError::new(
                "pageknot.export.pdf",
                ErrorStage::Encoding,
                "PDF stream made no progress before end of file",
            ));
        }
        bytes.extend_from_slice(&chunk);
        if eof {
            return Ok(bytes);
        }
    }
}
