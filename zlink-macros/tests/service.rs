#![cfg(feature = "service")]

// Tests for the service macro functionality.
// This file includes all service-related tests organized by feature.

#[path = "service/basic.rs"]
mod basic;
#[path = "service/borrowed-types.rs"]
mod borrowed_types;
#[path = "service/call_params.rs"]
mod call_params;
#[path = "service/custom_bounds.rs"]
mod custom_bounds;
#[path = "service/explicit-lifetimes.rs"]
mod explicit_lifetimes;
#[path = "service/fd_passing.rs"]
mod fd_passing;
#[path = "service/introspection.rs"]
mod introspection;
#[path = "service/metadata.rs"]
mod metadata;
#[path = "service/multiple_interfaces.rs"]
mod multiple_interfaces;
#[path = "service/per_interface_types.rs"]
mod per_interface_types;
#[path = "service/raw_idents.rs"]
mod raw_idents;
#[path = "service/streaming.rs"]
mod streaming;
#[path = "service/streaming_errors.rs"]
mod streaming_errors;
#[path = "service/streaming_fds.rs"]
mod streaming_fds;
#[path = "service/underscore_params.rs"]
mod underscore_params;
#[path = "service/upgrade.rs"]
mod upgrade;
