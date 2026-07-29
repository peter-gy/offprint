use std::io::{Read, Write};

use pageknot::{
    ArtifactExportRequest, ErrorStage, MarkdownOptions, PageKnot, PageKnotError, PdfOptions,
    PortablePath, Result,
};

use super::arguments::{conflict_policy, export_base_name, export_variant, require_local_browser};
use super::input::read_artifact;
use super::scheduler::{drive_scheduler, finish_runtime};
use crate::command::{ArtifactVariant, ExportArguments};
use crate::config::ResolvedConfig;
use crate::output::{output_error, write_json};

pub(super) async fn execute_export(
    arguments: ExportArguments,
    input: &mut dyn Read,
    output: &mut dyn Write,
) -> Result<()> {
    let includes_pdf = arguments.variant.contains(&ArtifactVariant::Pdf);
    let includes_markdown = arguments.variant.contains(&ArtifactVariant::Markdown);
    if (arguments.landscape || arguments.prefer_css_page_size) && !includes_pdf {
        return Err(PageKnotError::new(
            "pageknot.input.export_option",
            ErrorStage::Validation,
            "PDF options require the pdf artifact variant",
        ));
    }
    if arguments.no_front_matter && !includes_markdown {
        return Err(PageKnotError::new(
            "pageknot.input.export_option",
            ErrorStage::Validation,
            "Markdown options require the markdown artifact variant",
        ));
    }
    let base_name = arguments
        .base_name
        .unwrap_or_else(|| export_base_name(&arguments.artifact.0));
    let pdf_options = PdfOptions {
        landscape: arguments.landscape,
        prefer_css_page_size: arguments.prefer_css_page_size,
    };
    let markdown_options = MarkdownOptions {
        front_matter: !arguments.no_front_matter,
    };
    let request = ArtifactExportRequest {
        output_directory: PortablePath::new(arguments.output),
        base_name,
        variants: arguments
            .variant
            .into_iter()
            .map(|variant| export_variant(variant, pdf_options, markdown_options))
            .collect(),
        conflict: conflict_policy(arguments.conflict),
    };
    let mut resolved = ResolvedConfig::load(arguments.config.as_deref(), None)?;
    if let Some(path) = arguments.browser_path {
        resolved.apply_browser_path_flag(path);
    }
    require_local_browser(&resolved, "artifact export")?;
    let builder = resolved.apply_to_builder(PageKnot::builder());
    let artifact = read_artifact(arguments.artifact.0, input)?;
    let pageknot = builder.build()?;
    let operation = async {
        let result = drive_scheduler(
            pageknot.artifacts().export(artifact, request),
            tokio::signal::ctrl_c(),
            "artifact export",
        )
        .await?;
        if arguments.output_options.json {
            write_json(&result, output)?;
        } else {
            for variant in &result.variants {
                writeln!(output, "{}", variant.entrypoint).map_err(output_error)?;
            }
        }
        Ok(())
    }
    .await;
    finish_runtime(&pageknot, operation).await
}
