use offprint_model::{CaptureLimits, ErrorStage, OffprintError, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetCounter {
    Frames,
    Nodes,
    Resources,
    ResourceBytes,
    TotalResourceBytes,
}

#[derive(Clone, Debug)]
pub struct CaptureBudget {
    limits: CaptureLimits,
    frames: u32,
    nodes: u64,
    resources: u32,
    observation_bytes: u64,
    total_resource_bytes: u64,
}

impl CaptureBudget {
    #[must_use]
    pub fn new(limits: CaptureLimits) -> Self {
        Self {
            limits,
            frames: 0,
            nodes: 0,
            resources: 0,
            observation_bytes: 0,
            total_resource_bytes: 0,
        }
    }

    pub fn add_frame(&mut self) -> Result<()> {
        self.add_frames(1)
    }

    pub fn add_frames(&mut self, count: u32) -> Result<()> {
        let next = self.frames.checked_add(count).ok_or_else(|| {
            budget_limit_error(
                BudgetCounter::Frames,
                u64::from(self.frames) + u64::from(count),
                u64::from(self.limits.frames),
            )
        })?;
        enforce(
            BudgetCounter::Frames,
            u64::from(next),
            u64::from(self.limits.frames),
        )?;
        self.frames = next;
        Ok(())
    }

    pub fn add_nodes(&mut self, count: u64) -> Result<()> {
        let next = self
            .nodes
            .checked_add(count)
            .ok_or_else(|| budget_limit_error(BudgetCounter::Nodes, u64::MAX, self.limits.nodes))?;
        enforce(BudgetCounter::Nodes, next, self.limits.nodes)?;
        self.nodes = next;
        Ok(())
    }

    pub fn add_resource(&mut self) -> Result<()> {
        let next = self.resources.checked_add(1).ok_or_else(|| {
            budget_limit_error(
                BudgetCounter::Resources,
                u64::from(self.resources) + 1,
                u64::from(self.limits.resources),
            )
        })?;
        enforce(
            BudgetCounter::Resources,
            u64::from(next),
            u64::from(self.limits.resources),
        )?;
        self.resources = next;
        Ok(())
    }

    pub fn reserve_resource_bytes(&mut self, bytes: u64) -> Result<()> {
        enforce(
            BudgetCounter::ResourceBytes,
            bytes,
            self.limits.resource_bytes,
        )?;
        let next = self
            .total_resource_bytes
            .checked_add(bytes)
            .ok_or_else(|| {
                budget_limit_error(
                    BudgetCounter::TotalResourceBytes,
                    u64::MAX,
                    self.limits.total_resource_bytes,
                )
            })?;
        enforce(
            BudgetCounter::TotalResourceBytes,
            next,
            self.limits.total_resource_bytes,
        )?;
        self.total_resource_bytes = next;
        Ok(())
    }

    pub fn reserve_observation_bytes(&mut self, bytes: u64) -> Result<()> {
        let next = self
            .observation_bytes
            .checked_add(bytes)
            .ok_or_else(|| observation_payload_limit_error(u64::MAX, self.limits.artifact_bytes))?;
        if next > self.limits.artifact_bytes {
            return Err(observation_payload_limit_error(
                next,
                self.limits.artifact_bytes,
            ));
        }
        self.observation_bytes = next;
        Ok(())
    }

    #[must_use]
    pub const fn frames(&self) -> u32 {
        self.frames
    }

    #[must_use]
    pub const fn nodes(&self) -> u64 {
        self.nodes
    }

    #[must_use]
    pub const fn resources(&self) -> u32 {
        self.resources
    }

    #[must_use]
    pub const fn observation_bytes(&self) -> u64 {
        self.observation_bytes
    }

    #[must_use]
    pub const fn remaining_observation_bytes(&self) -> u64 {
        self.limits
            .artifact_bytes
            .saturating_sub(self.observation_bytes)
    }

    #[must_use]
    pub const fn total_resource_bytes(&self) -> u64 {
        self.total_resource_bytes
    }
}

fn observation_payload_limit_error(attempted: u64, limit: u64) -> OffprintError {
    OffprintError::new(
        "offprint.collector.payload_limit",
        ErrorStage::Collection,
        "collector payloads exceed the configured capture limit",
    )
    .with_detail("attempted", attempted)
    .with_detail("limit", limit)
}

fn enforce(counter: BudgetCounter, attempted: u64, limit: u64) -> Result<()> {
    if attempted <= limit {
        Ok(())
    } else {
        Err(budget_limit_error(counter, attempted, limit))
    }
}

fn budget_limit_error(counter: BudgetCounter, attempted: u64, limit: u64) -> OffprintError {
    OffprintError::new(
        "offprint.resource.limit",
        ErrorStage::Resource,
        format!("{counter:?} budget exceeded"),
    )
    .with_detail("attempted", attempted)
    .with_detail("limit", limit)
}

#[cfg(test)]
mod tests {
    use offprint_model::CaptureLimits;

    use super::CaptureBudget;

    #[test]
    fn byte_budget_rejects_work_before_mutating_the_counter() {
        let limits = CaptureLimits {
            resource_bytes: 4,
            total_resource_bytes: 6,
            ..CaptureLimits::default()
        };
        let mut budget = CaptureBudget::new(limits);

        assert!(budget.reserve_resource_bytes(4).is_ok());
        let result = budget.reserve_resource_bytes(3);

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.resource.limit")
        );
        assert_eq!(budget.total_resource_bytes(), 4);
    }

    #[test]
    fn observation_payload_budget_is_shared_across_frames() {
        let limits = CaptureLimits {
            artifact_bytes: 10,
            ..CaptureLimits::default()
        };
        let mut budget = CaptureBudget::new(limits);

        assert!(budget.reserve_observation_bytes(6).is_ok());
        let result = budget.reserve_observation_bytes(5);

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.collector.payload_limit")
        );
        assert_eq!(budget.observation_bytes(), 6);
        assert_eq!(budget.remaining_observation_bytes(), 4);
    }

    #[test]
    fn frame_counter_rejects_numeric_overflow_without_mutating() {
        let limits = CaptureLimits {
            frames: u32::MAX,
            ..CaptureLimits::default()
        };
        let mut budget = CaptureBudget::new(limits);

        assert!(budget.add_frames(u32::MAX).is_ok());
        let result = budget.add_frame();

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.resource.limit")
        );
        assert_eq!(budget.frames(), u32::MAX);
    }

    #[test]
    fn node_counter_rejects_numeric_overflow_without_mutating() {
        let limits = CaptureLimits {
            nodes: u64::MAX,
            ..CaptureLimits::default()
        };
        let mut budget = CaptureBudget::new(limits);

        assert!(budget.add_nodes(u64::MAX).is_ok());
        let result = budget.add_nodes(1);

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.resource.limit")
        );
        assert_eq!(budget.nodes(), u64::MAX);
    }

    #[test]
    fn resource_counter_rejects_numeric_overflow_without_mutating() {
        let limits = CaptureLimits {
            resources: u32::MAX,
            ..CaptureLimits::default()
        };
        let mut budget = CaptureBudget::new(limits);
        // Start at the numeric boundary without issuing billions of reservations.
        budget.resources = u32::MAX;

        let result = budget.add_resource();

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.resource.limit")
        );
        assert_eq!(budget.resources(), u32::MAX);
    }

    #[test]
    fn resource_byte_counter_rejects_numeric_overflow_without_mutating() {
        let limits = CaptureLimits {
            resource_bytes: u64::MAX,
            total_resource_bytes: u64::MAX,
            ..CaptureLimits::default()
        };
        let mut budget = CaptureBudget::new(limits);

        assert!(budget.reserve_resource_bytes(u64::MAX).is_ok());
        let result = budget.reserve_resource_bytes(1);

        assert_eq!(
            result.as_ref().map_err(|error| error.code.as_str()),
            Err("offprint.resource.limit")
        );
        assert_eq!(budget.total_resource_bytes(), u64::MAX);
    }
}
