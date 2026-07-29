use std::path::Path;

use pageknot::{
    ArtifactSpec, ArtifactTarget, ArtifactVariant, CaptureRequest, ConflictPolicy, ErrorStage,
    PageKnotError, PdfOptions, Result, VerificationPolicy,
};

use crate::command::{CaptureArguments, CaptureFormat};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum CaptureDestination {
    Default,
    Stdout,
    File(String),
}

impl CaptureDestination {
    pub(super) fn from_output(output: Option<&str>) -> Self {
        match output {
            None => Self::Default,
            Some("-") => Self::Stdout,
            Some(path) => Self::File(path.to_owned()),
        }
    }

    pub(super) fn requested_output(&self) -> Option<&str> {
        match self {
            Self::Default => None,
            Self::Stdout => Some("-"),
            Self::File(path) => Some(path),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::runner) struct HtmlCapturePlan {
    pub(super) destination: CaptureDestination,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::runner) struct DerivedCapturePlan {
    pub(super) destination: CaptureDestination,
    pub(super) variant: ArtifactVariant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::runner) enum CapturePlan {
    Html(HtmlCapturePlan),
    Derived(DerivedCapturePlan),
}

impl CapturePlan {
    pub(in crate::runner) fn from_arguments(arguments: &CaptureArguments) -> Result<Self> {
        if arguments.output.as_deref() == Some("-") && arguments.output_options.json {
            return Err(PageKnotError::new(
                "pageknot.input.output",
                ErrorStage::Validation,
                "`--json` cannot be combined with `--output -`",
            ));
        }

        let destination = CaptureDestination::from_output(arguments.output.as_deref());
        match arguments.format {
            CaptureFormat::Html => {
                if arguments.landscape || arguments.prefer_css_page_size {
                    return Err(PageKnotError::new(
                        "pageknot.input.export_option",
                        ErrorStage::Validation,
                        "PDF print options require `--format pdf`",
                    ));
                }
                Ok(Self::Html(HtmlCapturePlan { destination }))
            }
            CaptureFormat::Pdf => {
                if let CaptureDestination::File(path) = &destination
                    && Path::new(path)
                        .extension()
                        .and_then(std::ffi::OsStr::to_str)
                        .is_none_or(|extension| extension != "pdf")
                {
                    return Err(PageKnotError::new(
                        "pageknot.input.output",
                        ErrorStage::Validation,
                        "PDF capture output must use a .pdf extension",
                    ));
                }
                Ok(Self::Derived(DerivedCapturePlan {
                    destination,
                    variant: ArtifactVariant::Pdf(PdfOptions {
                        landscape: arguments.landscape,
                        prefer_css_page_size: arguments.prefer_css_page_size,
                    }),
                }))
            }
        }
    }

    pub(in crate::runner) fn apply_to_request(&self, request: &mut CaptureRequest) {
        match self {
            Self::Html(plan) => match &plan.destination {
                CaptureDestination::File(path) => {
                    let ArtifactSpec::Html(spec) = &mut request.artifact;
                    spec.target = ArtifactTarget::File(path.as_str().into());
                    spec.conflict = ConflictPolicy::Replace;
                }
                CaptureDestination::Default | CaptureDestination::Stdout => {
                    request.artifact = ArtifactSpec::html_bytes(request.limits.artifact_bytes);
                }
            },
            Self::Derived(_) => {
                request.artifact = ArtifactSpec::html_bytes(request.limits.artifact_bytes);
            }
        }
    }

    pub(super) fn validate_request(&self, request: &CaptureRequest) -> Result<()> {
        if matches!(self, Self::Derived(_)) && request.verification != VerificationPolicy::Offline {
            return Err(PageKnotError::new(
                "pageknot.input.export_option",
                ErrorStage::Validation,
                "PDF capture requires `--verify offline`",
            ));
        }
        Ok(())
    }

    pub(super) const fn requires_local_browser(&self) -> bool {
        matches!(self, Self::Derived(_))
    }

    pub(super) fn requested_output(&self) -> Option<&str> {
        match self {
            Self::Html(plan) => plan.destination.requested_output(),
            Self::Derived(plan) => plan.destination.requested_output(),
        }
    }
}
