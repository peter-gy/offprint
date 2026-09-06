use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub struct CaptureCancellation {
    token: CancellationToken,
}

impl CaptureCancellation {
    #[must_use]
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
        }
    }

    pub fn cancel(&self) {
        self.token.cancel();
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }

    #[must_use]
    pub fn child_token(&self) -> CancellationToken {
        self.token.child_token()
    }

    pub async fn cancelled(&self) {
        self.token.cancelled().await;
    }
}

impl Default for CaptureCancellation {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::CaptureCancellation;

    #[test]
    fn cancellation_is_idempotent_and_propagates_to_children() {
        let cancellation = CaptureCancellation::new();
        let child = cancellation.child_token();

        cancellation.cancel();
        cancellation.cancel();

        assert!(cancellation.is_cancelled());
        assert!(child.is_cancelled());
    }

    proptest! {
        #![proptest_config({
            let mut config = ProptestConfig::default();
            if std::env::var_os("PROPTEST_CASES").is_none() {
                config.cases = 128;
            }
            config
        })]

        #[test]
        fn arbitrary_repeated_cancellation_stays_terminal(
            child_count in 0_usize..32,
            cancellation_count in 1_usize..64,
        ) {
            let cancellation = CaptureCancellation::new();
            let children = (0..child_count)
                .map(|_| cancellation.child_token())
                .collect::<Vec<_>>();

            for _ in 0..cancellation_count {
                cancellation.cancel();
            }

            prop_assert!(cancellation.is_cancelled());
            prop_assert!(children.iter().all(|child| child.is_cancelled()));
            prop_assert!(cancellation.child_token().is_cancelled());
        }
    }
}
