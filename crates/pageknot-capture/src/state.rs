use pageknot_model::{CaptureStatus, ErrorStage, PageKnotError, Result};

#[derive(Clone, Debug)]
pub struct CaptureStateMachine {
    status: CaptureStatus,
}

impl CaptureStateMachine {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            status: CaptureStatus::Created,
        }
    }

    #[must_use]
    pub const fn status(&self) -> CaptureStatus {
        self.status
    }

    pub fn transition(&mut self, next: CaptureStatus) -> Result<()> {
        if !can_transition(self.status, next) {
            return Err(PageKnotError::new(
                "pageknot.runtime.state_transition",
                ErrorStage::Internal,
                format!(
                    "capture cannot transition from {:?} to {next:?}",
                    self.status
                ),
            ));
        }
        self.status = next;
        Ok(())
    }
}

impl Default for CaptureStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

fn can_transition(current: CaptureStatus, next: CaptureStatus) -> bool {
    use CaptureStatus::{
        Cancelled, Cancelling, Collecting, Committing, Created, Encoding, Failed, Navigating,
        ResolvingResources, Settling, Succeeded, Transforming, Validating, Verifying,
        WaitingForBrowser,
    };
    matches!(
        (current, next),
        (Created, Validating)
            | (Validating, WaitingForBrowser)
            | (WaitingForBrowser, Navigating)
            | (Navigating, Settling)
            | (Settling, Collecting)
            | (Collecting, ResolvingResources)
            | (ResolvingResources, Transforming)
            | (Transforming, Encoding)
            | (Encoding, Verifying)
            | (Verifying, Committing)
            | (Committing, Succeeded)
            | (Cancelling, Cancelled)
            | (Cancelling, Failed)
    ) || (!current.is_terminal() && current != Cancelling && matches!(next, Cancelling | Failed))
}

#[cfg(test)]
mod tests {
    use pageknot_model::CaptureStatus;
    use proptest::prelude::*;

    use super::CaptureStateMachine;

    #[test]
    fn success_path_preserves_the_normative_stage_order() {
        let mut state = CaptureStateMachine::new();
        for status in [
            CaptureStatus::Validating,
            CaptureStatus::WaitingForBrowser,
            CaptureStatus::Navigating,
            CaptureStatus::Settling,
            CaptureStatus::Collecting,
            CaptureStatus::ResolvingResources,
            CaptureStatus::Transforming,
            CaptureStatus::Encoding,
            CaptureStatus::Verifying,
            CaptureStatus::Committing,
            CaptureStatus::Succeeded,
        ] {
            assert!(state.transition(status).is_ok());
        }
        assert_eq!(state.status(), CaptureStatus::Succeeded);
    }

    #[test]
    fn terminal_state_rejects_further_progress() {
        let mut state = CaptureStateMachine::new();
        assert!(state.transition(CaptureStatus::Failed).is_ok());

        let result = state.transition(CaptureStatus::Validating);

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("pageknot.runtime.state_transition")
        );
        assert_eq!(state.status(), CaptureStatus::Failed);
    }

    #[test]
    fn cancellation_has_one_named_terminal_path() {
        let mut state = CaptureStateMachine::new();
        assert!(state.transition(CaptureStatus::Validating).is_ok());
        assert!(state.transition(CaptureStatus::Cancelling).is_ok());
        assert!(state.transition(CaptureStatus::Cancelled).is_ok());
        assert_eq!(state.status(), CaptureStatus::Cancelled);
    }

    fn status_strategy() -> impl Strategy<Value = CaptureStatus> {
        prop_oneof![
            Just(CaptureStatus::Created),
            Just(CaptureStatus::Validating),
            Just(CaptureStatus::WaitingForBrowser),
            Just(CaptureStatus::Navigating),
            Just(CaptureStatus::Settling),
            Just(CaptureStatus::Collecting),
            Just(CaptureStatus::ResolvingResources),
            Just(CaptureStatus::Transforming),
            Just(CaptureStatus::Encoding),
            Just(CaptureStatus::Verifying),
            Just(CaptureStatus::Committing),
            Just(CaptureStatus::Cancelling),
            Just(CaptureStatus::Succeeded),
            Just(CaptureStatus::Cancelled),
            Just(CaptureStatus::Failed),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::default())]

        #[test]
        fn arbitrary_transition_sequences_accept_one_terminal_state_at_most(
            candidates in proptest::collection::vec(status_strategy(), 0..64),
        ) {
            let mut state = CaptureStateMachine::new();
            let mut accepted_terminal_states = 0_u8;

            for candidate in candidates {
                if state.transition(candidate).is_ok() && candidate.is_terminal() {
                    accepted_terminal_states += 1;
                }
            }

            prop_assert!(accepted_terminal_states <= 1);
        }
    }
}
