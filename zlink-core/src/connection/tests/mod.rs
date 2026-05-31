mod read_connection_tests;
mod write_connection_tests;

#[cfg(feature = "std")]
mod chain_fds_tests;
#[cfg(feature = "std")]
mod connection_tests;
#[cfg(feature = "std")]
mod read_connection_fds_tests;
#[cfg(feature = "std")]
mod upgrade_tests;
#[cfg(feature = "std")]
mod write_connection_fds_tests;
