#![cfg_attr(not(any(feature = "tokio", feature = "smol")), no_std)]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/z-galaxy/zlink/3660d731d7de8f60c8d82e122b3ece15617185e4/data/logo.png"
)]
#![deny(
    missing_debug_implementations,
    nonstandard_style,
    rust_2018_idioms,
    missing_docs
)]
#![warn(unreachable_pub)]
#![doc = include_str!("../README.md")]

#[cfg(not(any(feature = "tokio", feature = "smol")))]
compile_error!("At least one runtime feature must be enabled: 'tokio' or 'smol'");

#[cfg(feature = "tokio")]
pub use zlink_tokio::*;

#[cfg(all(feature = "smol", not(feature = "tokio")))]
pub use zlink_smol::*;
