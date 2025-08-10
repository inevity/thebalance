use std::time::Duration;
use web_time::Instant as WebInstant;

/// a wrapper type over Instant to force checked additions and prevent
/// unintentional overflow. The type preserve the Copy semantics for the wrapped
#[derive(PartialEq, PartialOrd, Clone, Copy)]
pub(crate) struct Instant(WebInstant);

pub(crate) trait CheckedTimeOps {
    fn checked_add(&self, duration: Duration) -> Option<Self>
    where
        Self: Sized;
}

impl Instant {
    pub(crate) fn new(instant: WebInstant) -> Instant {
        Instant(instant)
    }

    pub(crate) fn now() -> Instant {
        Instant(WebInstant::now())
    }
}

impl CheckedTimeOps for Instant {
    fn checked_add(&self, duration: Duration) -> Option<Instant> {
        self.0.checked_add(duration).map(Instant)
    }
}
