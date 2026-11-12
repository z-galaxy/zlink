//! [`serde::Serialize`]-carrying wrapper around [`core::convert::Infallible`].

use core::fmt::Debug;

use serde::{Serialize, Serializer};

/// [`serde::Serialize`]-carrying wrapper around [`core::convert::Infallible`].
///
/// Recommended choice for [`Service::ReplyError`] and [`Service::ReplyStreamError`] when the
/// corresponding methods cannot fail. The [`interface`] macro also picks it for
/// [`Service::ReplyStreamError`] when no streaming method declares an error type.
///
/// It exists because of [serde-rs/serde#2740]: `core::convert::Infallible` has no `Serialize`
/// impl in `serde`, which makes it unusable for trait associated types bounded by `Serialize`.
/// This wrapper carries the missing impl. The inner [`core::convert::Infallible`] cannot be
/// constructed, so neither can this — the `Serialize` impl is statically unreachable.
///
/// Once serde lands a built-in `Serialize` impl for `Infallible`, this wrapper will be
/// deprecated in favor of `core::convert::Infallible`.
///
/// [`Service::ReplyError`]: super::service::Service::ReplyError
/// [`Service::ReplyStreamError`]: super::service::Service::ReplyStreamError
/// [`interface`]: macro@crate::interface
/// [serde-rs/serde#2740]: https://github.com/serde-rs/serde/issues/2740
#[derive(Debug)]
pub struct Infallible(core::convert::Infallible);

impl Serialize for Infallible {
    fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        unreachable!("`Infallible` is uninhabited, so this method can never run")
    }
}
