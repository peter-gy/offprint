use pageknot_model::{ErrorStage, PageKnotError};

use super::journal::JournalPhase;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FaultPoint {
    AfterJournal(JournalPhase),
    AfterBackup(usize),
    AfterCommit(usize),
    BeforeCleanup,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FaultAction {
    Error,
    Crash,
}

pub(super) trait FaultInjector {
    fn action(&mut self, point: FaultPoint) -> Option<FaultAction>;
}

#[derive(Debug, Default)]
pub(super) struct NoFault;

impl FaultInjector for NoFault {
    fn action(&mut self, _point: FaultPoint) -> Option<FaultAction> {
        None
    }
}

pub(super) fn injected_fault(point: FaultPoint, action: FaultAction) -> PageKnotError {
    PageKnotError::new(
        "pageknot.export.output",
        ErrorStage::Commit,
        match action {
            FaultAction::Error => "injected export transaction failure",
            FaultAction::Crash => "injected export transaction crash",
        },
    )
    .with_detail("faultPoint", format!("{point:?}"))
    .with_detail("injectedCrash", action == FaultAction::Crash)
}

#[cfg(test)]
#[derive(Debug)]
pub(super) struct OneFault {
    point: FaultPoint,
    action: FaultAction,
    fired: bool,
}

#[cfg(test)]
impl OneFault {
    pub(super) const fn crash(point: FaultPoint) -> Self {
        Self {
            point,
            action: FaultAction::Crash,
            fired: false,
        }
    }

    pub(super) const fn error(point: FaultPoint) -> Self {
        Self {
            point,
            action: FaultAction::Error,
            fired: false,
        }
    }
}

#[cfg(test)]
impl FaultInjector for OneFault {
    fn action(&mut self, point: FaultPoint) -> Option<FaultAction> {
        if !self.fired && point == self.point {
            self.fired = true;
            Some(self.action)
        } else {
            None
        }
    }
}
