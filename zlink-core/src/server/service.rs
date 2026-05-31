//! Service-related API.

use core::{fmt::Debug, future::Future};

use futures_util::Stream;
use serde::{Deserialize, Serialize};

pub use super::infallible::Infallible;
use crate::{Call, Connection, Reply, connection::Socket};

/// The item type that a [`Service::ReplyStream`] yields.
///
/// Each item is either an [`Ok`] success [`Reply`] or an [`Err`] error reply. Services whose
/// streaming methods never fail should set [`Service::ReplyStreamError`] to [`Infallible`], in
/// which case the `Err` arm is statically unreachable.
///
/// On `std`, the alias additionally pairs the result with the file descriptors to send. On
/// `no_std`, it is just the result.
#[cfg(feature = "std")]
pub type ReplyStreamItem<Params, Error> = (
    core::result::Result<Reply<Params>, Error>,
    Vec<std::os::fd::OwnedFd>,
);
/// The item type that a [`Service::ReplyStream`] yields.
///
/// Each item is either an [`Ok`] success [`Reply`] or an [`Err`] error reply. Services whose
/// streaming methods never fail should set [`Service::ReplyStreamError`] to [`Infallible`], in
/// which case the `Err` arm is statically unreachable.
///
/// On `std`, the alias additionally pairs the result with the file descriptors to send. On
/// `no_std`, it is just the result.
#[cfg(not(feature = "std"))]
pub type ReplyStreamItem<Params, Error> = core::result::Result<Reply<Params>, Error>;

/// Service trait for handling method calls.
///
/// Instead of implementing this trait manually, prefer using the [`service`] attribute macro which
/// generates the implementation for you. The macro provides a more ergonomic API and handles the
/// boilerplate of method dispatching, error handling, and streaming replies.
///
/// See the [`service`] macro documentation for details and examples.
///
/// [`service`]: macro@crate::service
pub trait Service<Sock>
where
    Sock: Socket,
{
    /// The type of method call that this service handles.
    ///
    /// This should be a type that can deserialize itself from a complete method call message: i-e
    /// an object containing `method` and `parameter` fields. This can be easily achieved using the
    /// `serde::Deserialize` derive (See the code snippet in
    /// [`crate::connection::WriteConnection::send_call`] documentation for an example).
    type MethodCall<'de>: Deserialize<'de> + Debug;
    /// The type of the successful reply.
    ///
    /// This should be a type that can serialize itself as the `parameters` field of the reply.
    type ReplyParams<'ser>: Serialize + Debug
    where
        Self: 'ser;
    /// The type of the item that [`Service::ReplyStream`] will be expected to yield.
    ///
    /// This should be a type that can serialize itself as the `parameters` field of the reply.
    type ReplyStreamParams: Serialize + Debug;
    /// The type of an error reply produced by a streaming method.
    ///
    /// Unlike [`Self::ReplyError`], this type cannot borrow from `&self` (the stream outlives the
    /// `handle` call). Services whose streaming methods never fail should set this to
    /// [`Infallible`] — the `Err` arm of [`ReplyStreamItem`] then becomes statically unreachable.
    type ReplyStreamError: Serialize + Debug;
    /// The type of the multi-reply stream.
    ///
    /// If the client asks for multiple replies, this stream will be used to send them. Each
    /// stream item is either a success [`Reply`] or an error of type [`Self::ReplyStreamError`].
    type ReplyStream: Stream<Item = ReplyStreamItem<Self::ReplyStreamParams, Self::ReplyStreamError>>
        + Unpin;
    /// The type of the error reply.
    ///
    /// This should be a type that can serialize itself to the whole reply object, containing
    /// `error` and `parameter` fields. This can be easily achieved using the `serde::Serialize`
    /// derive (See the code snippet in [`crate::connection::ReadConnection::receive_reply`]
    /// documentation for an example).
    ///
    /// Services whose methods never fail can use [`Infallible`] for this type as well.
    type ReplyError<'ser>: Serialize + Debug
    where
        Self: 'ser;

    /// Handle a method call.
    fn handle<'ser>(
        &'ser mut self,
        method: &'ser Call<Self::MethodCall<'_>>,
        conn: &mut Connection<Sock>,
        #[cfg(feature = "std")] fds: Vec<std::os::fd::OwnedFd>,
    ) -> impl Future<
        Output = HandleResult<Self::ReplyParams<'ser>, Self::ReplyStream, Self::ReplyError<'ser>>,
    >;

    /// Whether the service handles protocol upgrades.
    #[cfg(feature = "std")]
    const HANDLES_UPGRADE: bool = false;

    /// Called when an upgrade has been successfully negotiated.
    #[cfg(feature = "std")]
    fn on_upgrade(
        &mut self,
        parts: crate::connection::ConnectionParts<Sock>,
    ) -> impl Future<Output = crate::Result<()>> {
        async move {
            let _ = parts;
            Ok(())
        }
    }
}

/// The result of a [`Service::handle`] call.
///
/// On `std`, this is a tuple of the method reply and the file descriptors to send with it. On
/// `no_std`, this is just the method reply.
#[cfg(feature = "std")]
pub type HandleResult<Params, ReplyStream, ReplyError> = (
    MethodReply<Params, ReplyStream, ReplyError>,
    Vec<std::os::fd::OwnedFd>,
);
/// The result of a [`Service::handle`] call.
///
/// On `std`, this is a tuple of the method reply and the file descriptors to send with it. On
/// `no_std`, this is just the method reply.
#[cfg(not(feature = "std"))]
pub type HandleResult<Params, ReplyStream, ReplyError> =
    MethodReply<Params, ReplyStream, ReplyError>;

/// A service method call reply.
#[derive(Debug)]
pub enum MethodReply<Params, ReplyStream, ReplyError> {
    /// A single reply.
    Single(Option<Params>),
    /// An error reply.
    Error(ReplyError),
    /// A multi-reply stream.
    Multi(ReplyStream),
}
