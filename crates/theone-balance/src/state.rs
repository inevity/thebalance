// Each storage strategy is implemented in its own module.
// We then use `pub use ... as strategy` to export the active implementation.

#[cfg(feature = "do_kv")]
pub use crate::state_do_kv as strategy;

// #[cfg(feature = "do_sqlite")]
// pub use crate::state_do_sqlite as strategy;

