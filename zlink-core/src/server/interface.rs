//! Contains the API to make a service up of several interfaces.

use core::{fmt::Debug, future::Future};

use futures_util::Stream;
use serde::{Deserialize, Serialize};

use super::service::{HandleResult, ReplyStreamItem, TakeOver};
use crate::{Call, Connection, connection::Socket, idl};

/// One interface of a service that is made up of several.
///
/// A [`Service`] handles all the method calls that a server receives. To split a service up, give
/// each of its interfaces a type of its own that implements this trait, make those types the fields
/// of a struct, and derive [`Service`] for that struct: its `Service` implementation hands each
/// method call over to the field whose interface the method belongs to, and implements the
/// `org.varlink.service` interface for all of them.
///
/// Instead of implementing this trait manually, prefer using the [`interface`] attribute macro,
/// which implements it for the type of the `impl` block it is applied to.
///
/// # Shared state
///
/// The interfaces of a service are separate values, so the methods of one can't reach the data of
/// another. `State` is the type of what they share instead: the struct that they are the fields of
/// keeps one value of it, and lends it to the interface that handles a call. An interface that
/// needs no shared state implements this trait for every `State`, which is what the [`interface`]
/// macro does for it, so that it fits into any service.
///
/// [`Service`]: trait@super::service::Service
/// [`interface`]: macro@crate::interface
pub trait Interface<Sock, State = ()>
where
    Sock: Socket,
{
    /// The description of the interface, which gives its name and its methods.
    ///
    /// A service that is made up of interfaces finds the interface of a call from it: a call goes
    /// to the interface whose description has the called method, under the name of the interface.
    /// So the description must list every method that [`Interface::handle`] handles, or the calls
    /// to the missing ones are replied to as calls to methods that don't exist, and the interfaces
    /// of a service must all differ in name.
    const DESCRIPTION: &'static idl::Interface<'static>;

    /// The type of method call that this interface handles.
    ///
    /// Like [`Service::MethodCall`], it deserializes itself from an object containing `method` and
    /// `parameters` fields.
    ///
    /// [`Service::MethodCall`]: super::service::Service::MethodCall
    type MethodCall<'de>: Deserialize<'de> + Debug;
    /// The type of the successful reply. See [`Service::ReplyParams`].
    ///
    /// [`Service::ReplyParams`]: super::service::Service::ReplyParams
    type ReplyParams<'ser>: Serialize + Debug
    where
        Self: 'ser,
        State: 'ser;
    /// The type of the item that [`Interface::ReplyStream`] will be expected to yield.
    type ReplyStreamParams: Serialize + Debug;
    /// The type of an error reply produced by a streaming method. See
    /// [`Service::ReplyStreamError`].
    ///
    /// [`Service::ReplyStreamError`]: super::service::Service::ReplyStreamError
    type ReplyStreamError: Serialize + Debug;
    /// The type of the multi-reply stream. See [`Service::ReplyStream`].
    ///
    /// [`Service::ReplyStream`]: super::service::Service::ReplyStream
    type ReplyStream: Stream<Item = ReplyStreamItem<Self::ReplyStreamParams, Self::ReplyStreamError>>
        + Unpin;
    /// The type of the error reply. See [`Service::ReplyError`].
    ///
    /// [`Service::ReplyError`]: super::service::Service::ReplyError
    type ReplyError<'ser>: Serialize + Debug
    where
        Self: 'ser,
        State: 'ser;
    /// The state that takes over the connections upgraded to another protocol. See
    /// [`Service::Upgrade`].
    ///
    /// [`Service::Upgrade`]: super::service::Service::Upgrade
    type Upgrade: TakeOver<Sock>;

    /// Handle a call to a method of this interface.
    ///
    /// `state` is what the interfaces of the service share.
    fn handle<'ser>(
        &'ser mut self,
        state: &'ser mut State,
        call: Call<&'ser Self::MethodCall<'_>>,
        conn: &mut Connection<Sock>,
        #[cfg(feature = "std")] fds: alloc::vec::Vec<std::os::fd::OwnedFd>,
    ) -> impl Future<
        Output = HandleResult<
            Self::ReplyParams<'ser>,
            Self::ReplyStream,
            Self::ReplyError<'ser>,
            Self::Upgrade,
        >,
    >;
}
