#![cfg(feature = "service")]

// Tests for the `interface` attribute macro, with the services that the `Service` derive makes of
// the interfaces it implements.
// This file includes all the interface-related tests, organized by feature.

#[path = "interface/basic.rs"]
mod basic;
#[path = "interface/borrowed-types.rs"]
mod borrowed_types;
#[path = "interface/call_params.rs"]
mod call_params;
#[path = "interface/custom_bounds.rs"]
mod custom_bounds;
#[path = "interface/explicit-lifetimes.rs"]
mod explicit_lifetimes;
#[path = "interface/fd_passing.rs"]
mod fd_passing;
#[path = "interface/handle.rs"]
mod handle;
#[path = "interface/introspection.rs"]
mod introspection;
#[path = "interface/metadata.rs"]
mod metadata;
#[path = "interface/multiple_interfaces.rs"]
mod multiple_interfaces;
#[path = "interface/per_interface_types.rs"]
mod per_interface_types;
#[path = "interface/raw_idents.rs"]
mod raw_idents;
#[path = "interface/state.rs"]
mod state;
#[path = "interface/streaming.rs"]
mod streaming;
#[path = "interface/streaming_errors.rs"]
mod streaming_errors;
#[path = "interface/streaming_fds.rs"]
mod streaming_fds;
#[path = "interface/underscore_params.rs"]
mod underscore_params;
#[path = "interface/upgrade.rs"]
mod upgrade;
