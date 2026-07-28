use pageknot_model::{CaptureRequest, Result};

#[derive(Clone, Debug)]
pub struct ValidatedCaptureRequest(CaptureRequest);

impl ValidatedCaptureRequest {
    pub fn new(request: CaptureRequest) -> Result<Self> {
        request.validate()?;
        Ok(Self(request))
    }

    #[must_use]
    pub const fn get(&self) -> &CaptureRequest {
        &self.0
    }

    #[must_use]
    pub fn into_inner(self) -> CaptureRequest {
        self.0
    }
}
