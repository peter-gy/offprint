use offprint::{CaptureOutput, CaptureRequest, ErrorStage, OffprintError, Result};

use crate::command::{CaptureArguments, ConflictMode};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::runner) struct CapturePlan {
    output: String,
    conflict: offprint::ConflictPolicy,
}

impl CapturePlan {
    pub(in crate::runner) fn from_arguments(arguments: &CaptureArguments) -> Result<Self> {
        if arguments.output == "-" && arguments.output_options.json {
            return Err(OffprintError::new(
                "offprint.input.output",
                ErrorStage::Validation,
                "`--json` cannot be combined with `--output -`",
            ));
        }
        Ok(Self {
            output: arguments.output.clone(),
            conflict: match arguments.on_exists {
                ConflictMode::Fail => offprint::ConflictPolicy::Fail,
                ConflictMode::Replace => offprint::ConflictPolicy::Replace,
                ConflictMode::Uniquify => offprint::ConflictPolicy::Uniquify,
            },
        })
    }

    pub(in crate::runner) fn apply_to_request(&self, request: &mut CaptureRequest) {
        request.output = if self.output == "-" {
            CaptureOutput::memory(request.limits.artifact_bytes)
        } else {
            CaptureOutput::File {
                path: self.output.as_str().into(),
                conflict: self.conflict,
            }
        };
    }

    pub(super) fn requested_output(&self) -> &str {
        &self.output
    }
}
