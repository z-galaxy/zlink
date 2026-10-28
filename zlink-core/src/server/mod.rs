pub(crate) mod listener;
mod select_all;
pub mod service;

mod infallible;
mod upgrade;

use alloc::{boxed::Box, vec::Vec};
use core::{
    future::{Future, poll_fn},
    pin::{Pin, pin},
    task::Poll,
};
use futures_util::{FutureExt, StreamExt};
use select_all::SelectAll;
use service::{MethodReply, TakeOver, Upgraded};

use crate::{Call, Connection, Reply, connection::Socket};

/// A server.
///
/// The server listens for incoming connections and handles method calls using a service.
#[derive(Debug)]
pub struct Server<Listener, Service> {
    listener: Option<Listener>,
    service: Service,
}

impl<Listener, Service> Server<Listener, Service>
where
    Listener: listener::Listener,
    Service: service::Service<Listener::Socket>,
{
    /// Create a new server that serves `service` to incoming connections from `listener`.
    pub fn new(listener: Listener, service: Service) -> Self {
        Self {
            listener: Some(listener),
            service,
        }
    }

    /// Run the server.
    ///
    /// The server serves each connection until it's closed, unless its upgrade to another protocol
    /// is confirmed: its socket is then handed over to the state that came with the reply (see
    /// [`TakeOver`]), and the server polls the future serving it instead, until it completes. If
    /// the listener stops accepting connections, the server returns once it's done with all of
    /// them, including the upgraded ones.
    ///
    /// # Caveats
    ///
    /// Due to [a bug in the rust compiler][abrc], the future returned by this method can not be
    /// treated as `Send`, even if all the specific types involved are `Send`. A major consequence
    /// of this fact unfortunately, is that it can not be spawned in a task of a multi-threaded
    /// runtime. For example, you can not currently do `tokio::spawn(server.run())`.
    ///
    /// Fortunately, there are easy workarounds for this. You can either:
    ///
    /// * Use a thread-local runtime (for example [`tokio::runtime::LocalRuntime`] or
    ///   [`tokio::task::LocalSet`]) to run the server in a local task, perhaps in a separate
    ///   thread.
    /// * Use some common API to run multiple futures at once, such as [`futures::select!`] or
    ///   [`tokio::select!`].
    ///
    /// Most importantly, this is most likely a temporary issue and will be fixed in the future. 😊
    ///
    /// [abrc]: https://github.com/rust-lang/rust/issues/100013
    /// [`tokio::runtime::LocalRuntime`]: https://docs.rs/tokio/latest/tokio/runtime/struct.LocalRuntime.html
    /// [`tokio::task::LocalSet`]: https://docs.rs/tokio/latest/tokio/task/struct.LocalSet.html
    /// [`futures::select!`]: https://docs.rs/futures/latest/futures/macro.select.html
    /// [`tokio::select!`]: https://docs.rs/tokio/latest/tokio/macro.select.html
    pub async fn run(mut self) -> crate::Result<()> {
        let mut listener = self.listener.take().unwrap();
        let mut connections = Vec::new();
        let mut reply_streams = Vec::<ReplyStream<Service::ReplyStream, Listener::Socket>>::new();
        let mut reply_stream_futures = Vec::new();
        // Vec for futures from `Connection::receive_call`. Reused across iterations to avoid
        // per-iteration allocations.
        let mut read_futures = Vec::new();
        // The futures serving the upgraded connections, along with the IDs of the connections.
        let mut upgrades = Vec::new();
        let mut last_reply_stream_winner = None;
        let mut last_method_call_winner = None;
        let mut listener_closed = false;

        loop {
            if listener_closed
                && connections.is_empty()
                && reply_streams.is_empty()
                && upgrades.is_empty()
            {
                return Ok(());
            }

            // We re-populate the `reply_stream_futures` in each iteration so we must clear it
            // first.
            reply_stream_futures.clear();
            {
                // SAFETY: Rust has no way to know that we don't re-use the mutable references in
                // each iteration (since we clear the `reply_stream_futures` vector) so we need to
                // go through a pointer to work around this.
                let reply_streams: &mut Vec<ReplyStream<Service::ReplyStream, Listener::Socket>> =
                    unsafe { &mut *(&mut reply_streams as *mut Vec<_>) };
                reply_stream_futures.extend(reply_streams.iter_mut().map(|s| s.stream.next()));
            }
            let start_index = last_reply_stream_winner.map(|idx| idx + 1);
            let mut reply_stream_select_all = SelectAll::new(start_index);
            for future in reply_stream_futures.iter_mut() {
                reply_stream_select_all.push(future);
            }

            // Prepare futures for reading method calls from connections.
            read_futures.clear();
            {
                // SAFETY: Same as above - mutable references are not reused across iterations.
                let connections: &mut Vec<Connection<Listener::Socket>> =
                    unsafe { &mut *(&mut connections as *mut Vec<_>) };
                read_futures.extend(connections.iter_mut().map(|c| c.receive_call()));
            }
            let mut read_select_all = SelectAll::new(last_method_call_winner.map(|idx| idx + 1));
            for future in &mut read_futures {
                // SAFETY: Futures in `read_futures` are dropped in place via `clear()` at the
                // start of the next iteration, never moved while pinned.
                unsafe {
                    read_select_all.push_unchecked(future);
                }
            }

            // Prepare the futures serving the upgraded connections. They only complete once, after
            // which they're removed, so there's no need to take turns to be polled first.
            let mut upgrade_select_all = SelectAll::new(None);
            for (_, future) in &mut upgrades {
                upgrade_select_all.push(future);
            }

            futures_util::select_biased! {
                // 1. Accept a new connection.
                conn = listener.accept().fuse() => match conn? {
                    Some(conn) => connections.push(conn),
                    None => listener_closed = true,
                },
                // 2. Serve the upgraded connections. They come before the method calls so that
                // they're polled even if there's always a method call to handle.
                (idx, ()) = upgrade_select_all.fuse() => {
                    let (id, _) = upgrades.swap_remove(idx);
                    trace!("Done serving upgraded client {}", id);
                },
                // 3. Read method calls from the existing connections and handle them.
                (idx, result) = read_select_all.fuse() => {
                        #[cfg(feature = "std")]
                        let (call, fds) = match result {
                            Ok((call, fds)) => (Ok(call), fds),
                            Err(e) => (Err(e), alloc::vec![]),
                        };
                        #[cfg(not(feature = "std"))]
                        let call = result;
                        last_method_call_winner = Some(idx);

                        let next = match call {
                            Ok(call) => {
                                #[cfg(feature = "std")]
                                let handling = self.handle_call(call, &mut connections[idx], fds);
                                #[cfg(not(feature = "std"))]
                                let handling = self.handle_call(call, &mut connections[idx]);
                                match while_serving_upgrades(handling, &mut upgrades).await {
                                    Ok(next) => Some(next),
                                    Err(e) => {
                                        warn!("Error writing to connection: {:?}", e);
                                        None
                                    }
                                }
                            }
                            Err(e) => {
                                debug!("Error reading from socket: {:?}", e);
                                None
                            }
                        };

                        match next {
                            Some(NextStep::Receive) => (),
                            Some(NextStep::Stream(stream)) => {
                                let conn = connections.swap_remove(idx);
                                reply_streams.push(ReplyStream::new(stream, conn));
                            }
                            Some(NextStep::Upgrade(state)) => {
                                let conn = connections.swap_remove(idx);
                                let id = conn.id();
                                trace!("Client {} upgraded, handing over its socket", id);
                                let socket = Upgraded::new(conn);
                                upgrades.push((id, Box::pin(state.take_over(socket))));
                            }
                            Some(NextStep::Close) | None => {
                                connections.swap_remove(idx);
                            }
                        }
                }
                // 4. Read replies from the reply streams and send them off.
                reply = reply_stream_select_all.fuse() => {
                    let (idx, item) = reply;
                    last_reply_stream_winner = Some(idx);
                    let id = reply_streams[idx].conn.id();

                    match item {
                        Some(item) => {
                            #[cfg(feature = "std")]
                            let (item, fds) = item;
                            let conn = &mut reply_streams[idx].conn;
                            #[cfg(feature = "std")]
                            let sending = async {
                                match item {
                                    Ok(reply) => conn.send_reply(&reply, fds).await,
                                    Err(error) => conn.send_error(&error, fds).await,
                                }
                            };
                            #[cfg(not(feature = "std"))]
                            let sending = async {
                                match item {
                                    Ok(reply) => conn.send_reply(&reply).await,
                                    Err(error) => conn.send_error(&error).await,
                                }
                            };
                            let send_result = while_serving_upgrades(sending, &mut upgrades).await;
                            if let Err(e) = send_result {
                                warn!("Error writing to client {}: {:?}", id, e);
                                reply_streams.swap_remove(idx);
                            }
                        }
                        None => {
                            trace!("Stream closed for client {}", id);
                            let stream = reply_streams.swap_remove(idx);
                            connections.push(stream.conn);
                        }
                    }
                }
            }
        }
    }

    async fn handle_call(
        &mut self,
        call: Call<Service::MethodCall<'_>>,
        conn: &mut Connection<Listener::Socket>,
        #[cfg(feature = "std")] fds: Vec<std::os::fd::OwnedFd>,
    ) -> crate::Result<NextStep<Service::ReplyStream, Service::Upgrade>> {
        #[cfg(feature = "std")]
        let (reply, reply_fds) = self.service.handle(&call, conn, fds).await;
        #[cfg(not(feature = "std"))]
        let reply = self.service.handle(&call, conn).await;

        let (params, state) = match reply {
            // Don't send replies or errors for oneway calls.
            MethodReply::Single(_) | MethodReply::Error(_) | MethodReply::Upgrade(..)
                if call.oneway() =>
            {
                return Ok(NextStep::Receive);
            }
            MethodReply::Single(params) => (params, None),
            MethodReply::Upgrade(params, state) => (params, Some(state)),
            MethodReply::Error(err) => {
                #[cfg(feature = "std")]
                conn.send_error(&err, reply_fds).await?;
                #[cfg(not(feature = "std"))]
                conn.send_error(&err).await?;

                return Ok(NextStep::Receive);
            }
            MethodReply::Multi(s) => {
                trace!("Client {} now turning into a reply stream", conn.id());

                return Ok(NextStep::Stream(s));
            }
        };

        let reply = Reply::new(params).set_continues(Some(false));
        #[cfg(feature = "std")]
        conn.send_reply(&reply, reply_fds).await?;
        #[cfg(not(feature = "std"))]
        conn.send_reply(&reply).await?;

        // A successful reply confirms an upgrade, whether or not it comes with the state to take
        // the connection over with.
        let next = match (call.upgrade(), state) {
            (true, Some(state)) => NextStep::Upgrade(state),
            (true, None) => {
                warn!(
                    "Upgrade of client {} confirmed without any state to take it over with, \
                     closing it",
                    conn.id()
                );
                NextStep::Close
            }
            (false, Some(_)) => {
                warn!(
                    "Upgrade of client {} confirmed although it didn't ask for one, ignoring it",
                    conn.id()
                );
                NextStep::Receive
            }
            (false, None) => NextStep::Receive,
        };

        Ok(next)
    }
}

/// Drive `future` to completion while serving the upgraded connections.
///
/// The server serves all its connections from a single task, so it would otherwise stop serving
/// the upgraded connections while it awaits anything else, even when that waits for one of them in
/// turn, e.g. a method call waiting for a transfer over an upgraded connection to complete.
async fn while_serving_upgrades<Fut, Upgrade>(
    future: Fut,
    upgrades: &mut Vec<(usize, Pin<Box<Upgrade>>)>,
) -> Fut::Output
where
    Fut: Future,
    Upgrade: Future<Output = ()>,
{
    let mut future = pin!(future);

    poll_fn(|cx| {
        if let Poll::Ready(output) = future.as_mut().poll(cx) {
            return Poll::Ready(output);
        }

        let mut idx = 0;
        while idx < upgrades.len() {
            if upgrades[idx].1.as_mut().poll(cx).is_pending() {
                idx += 1;
                continue;
            }
            let (id, _) = upgrades.swap_remove(idx);
            trace!("Done serving upgraded client {}", id);
        }

        Poll::Pending
    })
    .await
}

/// Method reply stream and connection pair.
#[derive(Debug)]
struct ReplyStream<St, Sock: Socket> {
    stream: St,
    conn: Connection<Sock>,
}

impl<St, Sock> ReplyStream<St, Sock>
where
    Sock: Socket,
{
    fn new(stream: St, conn: Connection<Sock>) -> Self {
        Self { stream, conn }
    }
}

/// What the server does with a connection after handling a method call received on it.
enum NextStep<St, Upgrade> {
    /// Receive the next method call.
    Receive,
    /// Send the replies of the stream.
    Stream(St),
    /// Hand the socket of the connection over to the state, since its upgrade was confirmed.
    Upgrade(Upgrade),
    /// Close the connection, since its upgrade was confirmed without any state to take it over
    /// with.
    Close,
}

#[cfg(test)]
mod tests {
    use alloc::{collections::VecDeque, rc::Rc};
    use core::{
        cell::{Cell, RefCell},
        future::{Future, poll_fn},
        pin::pin,
        task::{Context, Poll, Waker},
    };

    use serde::Deserialize;

    use super::*;
    use crate::{
        ReadyListener, Service,
        connection::socket::ReadHalf,
        service::{HandleResult, Infallible, ReplyStreamItem},
        test_utils::mock_socket::MockSocket,
        varlink_service,
    };

    #[tokio::test]
    async fn confirmed_upgrade_is_handed_over() {
        // The data of the other protocol, which starts with a NUL byte, follows the call.
        let reads: [&[u8]; 1] = [b"{\"method\":\"org.example.Upgrade\",\"upgrade\":true}\0\0raw"];
        let state = serve(&reads, TestService).await;

        let mut state = state.take();
        assert_eq!(state.handled, 1);
        assert_eq!(state.upgraded.len(), 1);
        let (socket, upgrade_state) = state.upgraded.pop().unwrap();
        // The state that came with the reply was handed over along with the socket.
        assert_eq!(upgrade_state, "upgraded");
        let (mut read, write) = socket.split();
        // The data that followed the call is read first.
        let mut buf = [0; 16];
        let bytes_read = read.read(&mut buf).await.unwrap().bytes_read();
        assert_eq!(&buf[..bytes_read], b"\0raw");
        // The reply was sent before the socket was handed over.
        assert_eq!(write.written_data(), b"{\"continues\":false}\0");
    }

    #[tokio::test]
    async fn refused_upgrade_is_not_handed_over() {
        let reads: [&[u8]; 3] = [
            // An error reply refuses the upgrade.
            b"{\"method\":\"org.example.Refuse\",\"upgrade\":true}\0",
            // A one-way call gets no reply that could confirm its upgrade.
            b"{\"method\":\"org.example.Upgrade\",\"oneway\":true,\"upgrade\":true}\0",
            // The connection is still served after the above. A call that doesn't ask for an
            // upgrade doesn't get one either, even if the service confirms one.
            b"{\"method\":\"org.example.Upgrade\"}\0",
        ];
        let state = serve(&reads, TestService).await;

        let state = state.borrow();
        assert_eq!(state.handled, 3);
        assert!(state.upgraded.is_empty());
    }

    #[tokio::test]
    async fn upgraded_connection_is_closed_without_take_over() {
        // A single reply confirms the upgrade without any state to take the connection over with,
        // and `()` doesn't take it over either.
        for single in [true, false] {
            let reads: [&[u8]; 2] = [
                b"{\"method\":\"org.example.Upgrade\",\"upgrade\":true}\0",
                // Not read since the connection is closed once it's upgraded.
                b"{\"method\":\"org.example.Upgrade\"}\0",
            ];
            let state = serve(&reads, |state| Closing { state, single }).await;

            assert_eq!(state.borrow().handled, 1, "single: {single}");
        }
    }

    #[test]
    fn upgraded_connections_are_served_while_handling_calls() {
        let start: &[&[u8]] = &[b"{\"method\":\"org.example.Start\",\"upgrade\":true}\0"];
        let finish: &[&[u8]] = &[b"{\"method\":\"org.example.Finish\"}\0"];
        let sockets = [start, finish].map(|reads| {
            MockSocket::with_reads(
                reads,
                #[cfg(feature = "std")]
                Vec::new(),
            )
        });
        let transfer = Rc::new(Transfer::default());
        let server = Server::new(
            Sockets(Some(sockets.into())),
            Transferring(transfer.clone()),
        );

        // The sockets never block, so the server only ever waits for the transfer over the
        // upgraded connection and for the `Finish` call, which waits for the transfer in turn.
        let mut run = pin!(server.run());
        let mut cx = Context::from_waker(Waker::noop());
        for _ in 0..100 {
            if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
                result.unwrap();
                assert!(transfer.done.raised.get());

                return;
            }
        }
        panic!("the server stopped serving the upgraded connection");
    }

    // Serves a connection that reads `reads` until the end or until it's handed over, with the
    // service that `new` creates.
    async fn serve<New, Svc>(reads: &[&[u8]], new: New) -> Rc<RefCell<State>>
    where
        New: FnOnce(Rc<RefCell<State>>) -> Svc,
        Svc: Service<MockSocket>,
    {
        let state = Rc::new(RefCell::new(State::default()));
        let socket = MockSocket::with_reads(
            reads,
            #[cfg(feature = "std")]
            Vec::new(),
        );
        Server::new(ReadyListener::new(socket), new(state.clone()))
            .run()
            .await
            .unwrap();

        state
    }

    #[derive(Debug, Default)]
    struct State {
        handled: usize,
        upgraded: Vec<(Upgraded<MockSocket>, &'static str)>,
    }

    #[derive(Debug)]
    struct TestService(Rc<RefCell<State>>);

    #[derive(Debug, Deserialize)]
    #[serde(tag = "method")]
    enum Method {
        #[serde(rename = "org.example.Upgrade")]
        Upgrade,
        #[serde(rename = "org.example.Refuse")]
        Refuse,
    }

    impl Service<MockSocket> for TestService {
        type MethodCall<'de> = Method;
        type ReplyParams<'ser>
            = ()
        where
            Self: 'ser;
        type ReplyStreamParams = ();
        type ReplyStreamError = Infallible;
        type ReplyStream = futures_util::stream::Empty<ReplyStreamItem<(), Infallible>>;
        type ReplyError<'ser>
            = varlink_service::Error<'ser>
        where
            Self: 'ser;
        type Upgrade = Recording;

        async fn handle<'ser>(
            &'ser mut self,
            method: &'ser Call<Self::MethodCall<'_>>,
            _conn: &mut Connection<MockSocket>,
            #[cfg(feature = "std")] _fds: Vec<std::os::fd::OwnedFd>,
        ) -> HandleResult<(), Self::ReplyStream, varlink_service::Error<'ser>, Recording> {
            self.0.borrow_mut().handled += 1;
            let reply = match method.method() {
                Method::Upgrade => MethodReply::Upgrade(
                    None,
                    Recording {
                        state: self.0.clone(),
                        label: "upgraded",
                    },
                ),
                Method::Refuse => MethodReply::Error(varlink_service::Error::InvalidParameter {
                    parameter: "upgrade".into(),
                }),
            };

            #[cfg(feature = "std")]
            return (reply, Vec::new());
            #[cfg(not(feature = "std"))]
            return reply;
        }
    }

    // Takes over the upgraded connections by recording their sockets, along with its label.
    #[derive(Debug)]
    struct Recording {
        state: Rc<RefCell<State>>,
        label: &'static str,
    }

    impl TakeOver<MockSocket> for Recording {
        async fn take_over(self, socket: Upgraded<MockSocket>) {
            // The socket is only taken over once the server polls the future.
            self.state.borrow_mut().upgraded.push((socket, self.label));
        }
    }

    // Confirms all upgrades, with a single reply or not, but doesn't take over the upgraded
    // connections.
    #[derive(Debug)]
    struct Closing {
        state: Rc<RefCell<State>>,
        single: bool,
    }

    impl Service<MockSocket> for Closing {
        type MethodCall<'de> = Method;
        type ReplyParams<'ser>
            = ()
        where
            Self: 'ser;
        type ReplyStreamParams = ();
        type ReplyStreamError = Infallible;
        type ReplyStream = futures_util::stream::Empty<ReplyStreamItem<(), Infallible>>;
        type ReplyError<'ser>
            = varlink_service::Error<'ser>
        where
            Self: 'ser;
        type Upgrade = ();

        async fn handle<'ser>(
            &'ser mut self,
            _method: &'ser Call<Self::MethodCall<'_>>,
            _conn: &mut Connection<MockSocket>,
            #[cfg(feature = "std")] _fds: Vec<std::os::fd::OwnedFd>,
        ) -> HandleResult<(), Self::ReplyStream, varlink_service::Error<'ser>> {
            self.state.borrow_mut().handled += 1;
            let reply = if self.single {
                MethodReply::Single(None)
            } else {
                MethodReply::Upgrade(None, ())
            };

            #[cfg(feature = "std")]
            return (reply, Vec::new());
            #[cfg(not(feature = "std"))]
            return reply;
        }
    }

    // Serves a transfer over the connection that a `Start` call upgrades, which a `Finish` call on
    // another connection waits for.
    #[derive(Debug)]
    struct Transferring(Rc<Transfer>);

    #[derive(Debug, Default)]
    struct Transfer {
        // Raised by the `Finish` call for the transfer to complete.
        go: Flag,
        // Raised by the transfer once it's complete.
        done: Flag,
    }

    #[derive(Debug, Deserialize)]
    #[serde(tag = "method")]
    enum TransferMethod {
        #[serde(rename = "org.example.Start")]
        Start,
        #[serde(rename = "org.example.Finish")]
        Finish,
    }

    impl Service<MockSocket> for Transferring {
        type MethodCall<'de> = TransferMethod;
        type ReplyParams<'ser>
            = ()
        where
            Self: 'ser;
        type ReplyStreamParams = ();
        type ReplyStreamError = Infallible;
        type ReplyStream = futures_util::stream::Empty<ReplyStreamItem<(), Infallible>>;
        type ReplyError<'ser>
            = varlink_service::Error<'ser>
        where
            Self: 'ser;
        type Upgrade = Transferred;

        async fn handle<'ser>(
            &'ser mut self,
            method: &'ser Call<Self::MethodCall<'_>>,
            _conn: &mut Connection<MockSocket>,
            #[cfg(feature = "std")] _fds: Vec<std::os::fd::OwnedFd>,
        ) -> HandleResult<(), Self::ReplyStream, varlink_service::Error<'ser>, Transferred>
        {
            let reply = match method.method() {
                TransferMethod::Start => MethodReply::Upgrade(None, Transferred(self.0.clone())),
                TransferMethod::Finish => {
                    self.0.go.raise();
                    self.0.done.raised().await;

                    MethodReply::Single(None)
                }
            };

            #[cfg(feature = "std")]
            return (reply, Vec::new());
            #[cfg(not(feature = "std"))]
            return reply;
        }
    }

    // Takes over the connection that a `Start` call upgrades, to complete the transfer over it once
    // the `Finish` call lets it.
    #[derive(Debug)]
    struct Transferred(Rc<Transfer>);

    impl TakeOver<MockSocket> for Transferred {
        async fn take_over(self, _socket: Upgraded<MockSocket>) {
            self.0.go.raised().await;
            self.0.done.raise();
        }
    }

    // A flag that futures can wait to be raised.
    #[derive(Debug, Default)]
    struct Flag {
        raised: Cell<bool>,
        waker: RefCell<Option<Waker>>,
    }

    impl Flag {
        fn raise(&self) {
            self.raised.set(true);
            if let Some(waker) = self.waker.take() {
                waker.wake();
            }
        }

        async fn raised(&self) {
            poll_fn(|cx| {
                if self.raised.get() {
                    return Poll::Ready(());
                }
                *self.waker.borrow_mut() = Some(cx.waker().clone());

                Poll::Pending
            })
            .await
        }
    }

    // Hands out its sockets, one connection after the other, and then reports its closure.
    #[derive(Debug)]
    struct Sockets(Option<VecDeque<MockSocket>>);

    impl listener::Listener for Sockets {
        type Socket = MockSocket;

        async fn accept(&mut self) -> crate::Result<Option<Connection<MockSocket>>> {
            let Some(sockets) = &mut self.0 else {
                return core::future::pending().await;
            };
            let socket = sockets.pop_front();
            if socket.is_none() {
                self.0 = None;
            }

            Ok(socket.map(Connection::new))
        }
    }
}
