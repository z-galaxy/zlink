//! Logging macros that abstract `tracing` and `defmt` one.
//!
//! Since these macros are internal API, we only have ones that we need.

// Re-export the logging crates so macros can use them.
#[doc(hidden)]
#[cfg(not(feature = "tracing"))]
pub use defmt;
#[doc(hidden)]
#[cfg(feature = "tracing")]
pub use tracing;

#[doc(hidden)]
#[macro_export]
#[cfg(feature = "tracing")]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::log::tracing::error!($($arg)*)
    }
}
// Note: Since user has to enable either `tracing` or `defmt` feature, we can assume that `defmt` is
// enabled when `tracing` is not.
#[doc(hidden)]
#[macro_export]
#[cfg(not(feature = "tracing"))]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::log::defmt::error!($($arg)*)
    }
}

#[doc(hidden)]
#[macro_export]
#[cfg(feature = "tracing")]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::log::tracing::warn!($($arg)*)
    }
}
#[doc(hidden)]
#[macro_export]
#[cfg(not(feature = "tracing"))]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::log::defmt::warn!($($arg)*)
    }
}

#[doc(hidden)]
#[macro_export]
#[cfg(feature = "tracing")]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::log::tracing::info!($($arg)*)
    }
}
#[doc(hidden)]
#[macro_export]
#[cfg(not(feature = "tracing"))]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::log::defmt::info!($($arg)*)
    }
}

#[doc(hidden)]
#[macro_export]
#[cfg(feature = "tracing")]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::log::tracing::debug!($($arg)*)
    }
}
#[doc(hidden)]
#[macro_export]
#[cfg(not(feature = "tracing"))]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::log::defmt::debug!($($arg)*)
    }
}

#[doc(hidden)]
#[macro_export]
#[cfg(feature = "tracing")]
macro_rules! trace {
    ($($arg:tt)*) => {
        $crate::log::tracing::trace!($($arg)*)
    }
}
#[doc(hidden)]
#[macro_export]
#[cfg(not(feature = "tracing"))]
macro_rules! trace {
    ($($arg:tt)*) => {
        $crate::log::defmt::trace!($($arg)*)
    }
}
