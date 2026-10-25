#![cfg(feature = "proxy")]

// Tests for the proxy macro functionality
// This file includes all proxy-related tests organized by feature

#[path = "proxy/basic.rs"]
mod basic;
#[path = "proxy/complex_lifetimes.rs"]
mod complex_lifetimes;
#[path = "proxy/fd_passing.rs"]
mod fd_passing;
#[path = "proxy/generics.rs"]
mod generics;
#[path = "proxy/lifetimes.rs"]
mod lifetimes;
#[path = "proxy/optional_params.rs"]
mod optional_params;
#[path = "proxy/rename.rs"]
mod rename;
#[path = "proxy/streaming.rs"]
mod streaming;
#[path = "proxy/upgrade.rs"]
mod upgrade;
