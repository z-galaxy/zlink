#![cfg_attr(not(feature = "tokio"), no_std)]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/zeenix/zlink/3660d731d7de8f60c8d82e122b3ece15617185e4/data/logo.png"
)]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    missing_docs
)]
#![warn(unreachable_pub)]
#![doc = include_str!("../README.md")]

#[cfg(not(feature = "tokio"))]
compile_error!("Currently 'tokio' feature must be enabled.");

#[cfg(feature = "tokio")]
pub use zlink_tokio::*;
