mod definitions;

use super::ErrorStage;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ErrorCodeDefinition {
    pub code: &'static str,
    pub stage: ErrorStage,
    pub retryable: bool,
    pub description: &'static str,
}

pub use definitions::ERROR_CODE_REGISTRY;

pub(super) fn definition(code: &str) -> Option<&'static ErrorCodeDefinition> {
    ERROR_CODE_REGISTRY
        .binary_search_by(|definition| definition.code.cmp(code))
        .ok()
        .map(|index| &ERROR_CODE_REGISTRY[index])
}
