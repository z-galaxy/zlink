//! Convenience API for maintaining state, that notifies on changes.

mod state;

pub use state::{State, Stream};
pub use zlink_core::notified as traits;
