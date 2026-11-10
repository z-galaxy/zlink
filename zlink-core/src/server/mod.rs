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
pub struct Server<Listener, Service>
where
    Listener: listener::Listener,
{
    listener: Option<Listener>,
    service: Service,
    /// The connections to serve along with those that the listener accepts.
    connections: Vec<Connection<Listener::Socket>>,
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
            connections: Vec::new(),
        }
    }

    /// Serve `connections` too, along with those that the listener accepts.
    ///
    /// These are connections that something else served before, such as a server that stopped
    /// (see [`Server::run_until`]), which may have read method calls off them that it didn't
    /// handle: the server handles those first. Before anything else, it writes what they still have
    /// to write, such as the rest of a reply that a stopped server interrupted.
    pub fn with_connections<Connections>(mut self, connections: Connections) -> Self
    where
        Connections: IntoIterator<Item = Connection<Listener::Socket>>,
    {
        self.connections.extend(connections);

        self
    }

    /// Run the server.
    ///
    /// The server serves each connection until it's closed, unless its upgrade to another protocol
    /// is confirmed: its socket is then handed over to the state that came with the reply (see
    /// [`TakeOver`]), and the server polls the future serving it instead, until it completes. If
    /// the listener stops accepting connections, the server returns once it's done with all of
    /// them, including the upgraded ones. [`Server::run_until`] stops it gracefully instead.
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
    pub async fn run(self) -> crate::Result<()> {
        self.run_until(core::future::pending()).await?;

        Ok(())
    }

    /// Run the server until `stop` completes, and then stop it gracefully.
    ///
    /// The server serves its connections as [`Server::run`] does until `stop` completes. It checks
    /// `stop` between method calls, so that the calls in progress are handled first, and while it
    /// waits to write to a client, so that a client that doesn't read its replies can't block the
    /// shutdown. It then stops accepting connections and reading method calls, and returns its
    /// listener, its service and the connections that it served, each between two method calls
    /// (see [`Stopped`]). Dropping them shuts the server down, and serving them again continues
    /// from where it stopped, as another server does that is given the connections (see
    /// [`Server::with_connections`]): the method calls that the server read but didn't handle stay
    /// in their connections, and so does the unwritten part of an interrupted reply, which is
    /// written first. The connections whose replies still stream, and those that were upgraded to
    /// another protocol, are closed, as nothing generic can continue them.
    ///
    /// A write is interrupted whenever it has to wait for the client, as it does for a reply larger
    /// than the socket buffer, even if the client is reading. Dropping the connection then loses
    /// the rest of the reply, so flush the connections before dropping them (see
    /// [`Connection::flush`]), with a timeout of your choice.
    ///
    /// If the listener stops accepting connections, the server returns once it's done with all of
    /// them, as [`Server::run`] does, without any connection to give back.
    ///
    /// # Caveats
    ///
    /// As for [`Server::run`], the returned future can't be treated as `Send`.
    pub async fn run_until<Stop>(mut self, stop: Stop) -> crate::Result<Stopped<Listener, Service>>
    where
        Stop: Future<Output = ()>,
    {
        let stop = pin!(stop);
        let mut stop = stop.fuse();
        let mut stopped = false;
        let mut listener = self.listener.take().unwrap();
        let mut connections = Vec::with_capacity(self.connections.len());
        for mut conn in core::mem::take(&mut self.connections) {
            // Once stopped, the server returns the remaining connections as they are.
            if !stopped {
                match unless_stopped(conn.flush(), &mut stop).await {
                    Some(Ok(())) => (),
                    Some(Err(e)) => {
                        warn!("Error writing to client {}: {:?}", conn.id(), e);
                        continue;
                    }
                    None => stopped = true,
                }
            }
            connections.push(conn);
        }
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

        while !stopped {
            if listener_closed
                && connections.is_empty()
                && reply_streams.is_empty()
                && upgrades.is_empty()
            {
                break;
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
                // 0. Stop, which comes first so that nothing more is taken once it's time to.
                () = stop => stopped = true,
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
                                let conn = &mut connections[idx];
                                #[cfg(feature = "std")]
                                let handling = self.handle_call(call, conn, fds);
                                #[cfg(not(feature = "std"))]
                                let handling = self.handle_call(call, conn);
                                match while_serving_upgrades(handling, &mut upgrades).await {
                                    Ok(next) => {
                                        // Write the reply, unless the server stops first, so that
                                        // a client that doesn't read it can't block the shutdown.
                                        let writing = unless_stopped(conn.flush(), &mut stop);
                                        match while_serving_upgrades(writing, &mut upgrades).await {
                                            Some(Ok(())) => Some(next),
                                            Some(Err(e)) => {
                                                warn!(
                                                    "Error writing to client {}: {:?}",
                                                    conn.id(),
                                                    e
                                                );
                                                None
                                            }
                                            None => {
                                                stopped = true;
                                                Some(next)
                                            }
                                        }
                                    }
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
                            // The socket is handed over only once the reply that confirms the
                            // upgrade was written in full.
                            Some(NextStep::Upgrade(state)) if !stopped => {
                                let conn = connections.swap_remove(idx);
                                let id = conn.id();
                                trace!("Client {} upgraded, handing over its socket", id);
                                let socket = Upgraded::new(conn);
                                upgrades.push((id, Box::pin(state.take_over(socket))));
                            }
                            Some(NextStep::Upgrade(_)) => {
                                let conn = connections.swap_remove(idx);
                                debug!(
                                    "Stopped before confirming the upgrade of client {}, closing \
                                     it",
                                    conn.id()
                                );
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
                            // Write the reply, unless the server stops first, which closes the
                            // connection anyway.
                            let sending = unless_stopped(sending, &mut stop);
                            match while_serving_upgrades(sending, &mut upgrades).await {
                                Some(Ok(())) => (),
                                Some(Err(e)) => {
                                    warn!("Error writing to client {}: {:?}", id, e);
                                    reply_streams.swap_remove(idx);
                                }
                                None => stopped = true,
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
        // The futures borrow the connections and the reply streams.
        read_futures.clear();
        reply_stream_futures.clear();
        if stopped {
            debug!(
                "Stopping the server, giving back {} connections and closing {} that stream \
                 replies or were upgraded",
                connections.len(),
                reply_streams.len() + upgrades.len(),
            );
        }

        Ok(Stopped {
            listener,
            service: self.service,
            connections,
        })
    }

    /// Handle `call`, enqueuing its reply, if any, for the server to write.
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
                conn.enqueue(&err, reply_fds)?;
                #[cfg(not(feature = "std"))]
                conn.enqueue(&err)?;

                return Ok(NextStep::Receive);
            }
            MethodReply::Multi(s) => {
                trace!("Client {} now turning into a reply stream", conn.id());

                return Ok(NextStep::Stream(s));
            }
        };

        let reply = Reply::new(params).set_continues(Some(false));
        #[cfg(feature = "std")]
        conn.enqueue(&reply, reply_fds)?;
        #[cfg(not(feature = "std"))]
        conn.enqueue(&reply)?;

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

/// What a server gives back as it stops (see [`Server::run_until`]).
#[derive(Debug)]
#[non_exhaustive]
pub struct Stopped<Listener, Service>
where
    Listener: listener::Listener,
{
    /// The listener, which has accepted no more connections since the server stopped.
    pub listener: Listener,
    /// The service.
    pub service: Service,
    /// The connections that the server served, each between two method calls. They keep the method
    /// calls that the server read but didn't handle, and the unwritten part of an interrupted
    /// reply (see [`Server::run_until`]).
    pub connections: Vec<Connection<Listener::Socket>>,
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

/// The output of `future`, unless `stop` completes first.
///
/// `future` is polled first, so that it completes if it doesn't have to wait.
async fn unless_stopped<Fut, Stop>(future: Fut, stop: &mut Stop) -> Option<Fut::Output>
where
    Fut: Future,
    Stop: Future<Output = ()> + Unpin,
{
    let mut future = pin!(future);

    poll_fn(|cx| {
        if let Poll::Ready(output) = future.as_mut().poll(cx) {
            return Poll::Ready(Some(output));
        }

        stop.poll_unpin(cx).map(|()| None)
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
        pin::{Pin, pin},
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

    #[tokio::test]
    async fn stopped_server_gives_back_its_connections() {
        // The second call comes in the same read as the first, after which the server stops, so
        // it reads the second call but doesn't handle it.
        let reads: [&[u8]; 1] =
            [b"{\"method\":\"org.example.Count\"}\0{\"method\":\"org.example.Count\"}\0"];
        let socket = MockSocket::with_reads(
            &reads,
            #[cfg(feature = "std")]
            Vec::new(),
        );
        let calls = Rc::new(Calls::stopping_after(1));
        let server = Server::new(ReadyListener::new(socket), Stopping(calls.clone()));
        let stopped = server.run_until(calls.stop.raised()).await.unwrap();
        assert_eq!(calls.counted.get(), 1);
        let [conn] = <[_; 1]>::try_from(stopped.connections).unwrap();
        // The call under way got its reply before the server stopped.
        assert_eq!(
            conn.write().write_half().written_data(),
            b"{\"continues\":false}\0"
        );

        // Another server serves the connection on, starting with the call that the first one read.
        Server::new(Sockets(Some(VecDeque::new())), stopped.service)
            .with_connections([conn])
            .run()
            .await
            .unwrap();
        assert_eq!(calls.counted.get(), 2);
    }

    #[tokio::test]
    async fn streaming_and_upgraded_connections_are_closed_on_stop() {
        let calls: [&[u8]; 2] = [
            b"{\"method\":\"org.example.Stream\",\"more\":true}\0",
            b"{\"method\":\"org.example.Upgrade\",\"upgrade\":true}\0",
        ];
        for call in calls {
            let socket = MockSocket::with_reads(
                &[call],
                #[cfg(feature = "std")]
                Vec::new(),
            );
            let counter = Rc::new(Calls::stopping_after(1));
            let server = Server::new(ReadyListener::new(socket), Stopping(counter.clone()));
            let stopped = server.run_until(counter.stop.raised()).await.unwrap();
            assert_eq!(counter.handled.get(), 1);
            assert!(
                stopped.connections.is_empty(),
                "{:?}: {:?}",
                core::str::from_utf8(call),
                stopped.connections
            );
        }
    }

    #[tokio::test]
    async fn stop_cuts_off_a_reply_that_waits_for_its_client() {
        // The client reads 4 bytes of the reply and makes room for more after a while, by which
        // time the stop that the call raised has completed.
        let reads: [&[u8]; 1] = [b"{\"method\":\"org.example.Count\"}\0"];
        let socket = MockSocket::with_reads(
            &reads,
            #[cfg(feature = "std")]
            Vec::new(),
        )
        .writing_in_chunks(4);
        let calls = Rc::new(Calls::stopping_after(1));
        let server = Server::new(ReadyListener::new(socket), Stopping(calls.clone()));
        let stopped = server.run_until(calls.stop.raised()).await.unwrap();
        let [mut conn] = <[_; 1]>::try_from(stopped.connections).unwrap();
        assert_eq!(conn.write().write_half().written_data(), b"{\"co");

        // The connection has the rest of the reply to write, which goes once.
        conn.flush().await.unwrap();
        assert_eq!(
            conn.write().write_half().written_data(),
            b"{\"continues\":false}\0"
        );
    }

    #[test]
    fn stop_cuts_off_a_streamed_reply_that_its_client_never_reads() {
        // The stream raises the stop as it yields its reply, which the client reads 4 bytes of and
        // no more.
        let reads: [&[u8]; 1] = [b"{\"method\":\"org.example.StreamAndStop\",\"more\":true}\0"];
        let socket = MockSocket::with_reads(
            &reads,
            #[cfg(feature = "std")]
            Vec::new(),
        )
        .stalling_after(4);
        let calls = Rc::new(Calls::stopping_after(0));
        let server = Server::new(ReadyListener::new(socket), Stopping(calls.clone()));

        // Nothing wakes the server once the client stops reading, so it stops in as many polls as
        // it takes to get there, or never.
        let mut run = pin!(server.run_until(calls.stop.raised()));
        let mut cx = Context::from_waker(Waker::noop());
        for _ in 0..100 {
            if let Poll::Ready(stopped) = run.as_mut().poll(&mut cx) {
                // The connection whose replies stream is closed.
                assert!(stopped.unwrap().connections.is_empty());

                return;
            }
        }
        panic!("the server didn't stop");
    }

    #[tokio::test]
    async fn stop_cuts_off_what_given_connections_have_left_to_write() {
        // Each connection has a reply left to write, which its client reads 4 bytes of and makes
        // room for more of after a while, by which time the stop has completed.
        let connections = [(); 2].map(|()| {
            let mut conn = Connection::new(MockSocket::with_responses(&[]).writing_in_chunks(4));
            let reply = Reply::<()>::new(None).set_continues(Some(false));
            #[cfg(feature = "std")]
            conn.enqueue(&reply, Vec::new()).unwrap();
            #[cfg(not(feature = "std"))]
            conn.enqueue(&reply).unwrap();

            conn
        });
        let server = Server::new(Sockets(None), Stopping(Rc::new(Calls::stopping_after(0))))
            .with_connections(connections);
        let stopped = server.run_until(core::future::ready(())).await.unwrap();

        // The server stopped as it waited to write the rest of the first reply, and gave the
        // second connection back as it was.
        let mut connections = stopped.connections;
        let written: Vec<_> = connections
            .iter()
            .map(|conn| conn.write().write_half().written_data())
            .collect();
        assert_eq!(written, [&b"{\"co"[..], b""]);
        for conn in &mut connections {
            conn.flush().await.unwrap();
            assert_eq!(
                conn.write().write_half().written_data(),
                b"{\"continues\":false}\0"
            );
        }
    }

    #[tokio::test]
    async fn upgrade_is_taken_over_once_its_reply_went_in_full() {
        // The reply that confirms the upgrade goes in full, and the server then stops, closing the
        // upgraded connection, or the client reads 4 bytes of it and makes room for more after a
        // while, by which time the stop has completed, and the server closes the connection without
        // handing it over.
        for chunked in [false, true] {
            let reads: [&[u8]; 1] = [b"{\"method\":\"org.example.Upgrade\",\"upgrade\":true}\0"];
            let mut socket = MockSocket::with_reads(
                &reads,
                #[cfg(feature = "std")]
                Vec::new(),
            );
            if chunked {
                socket = socket.writing_in_chunks(4);
            }
            let calls = Rc::new(Calls::stopping_after(1));
            let server = Server::new(ReadyListener::new(socket), Stopping(calls.clone()));
            let stopped = server.run_until(calls.stop.raised()).await.unwrap();
            assert!(stopped.connections.is_empty(), "chunked: {chunked}");
            assert_eq!(
                calls.taken_over.get(),
                usize::from(!chunked),
                "chunked: {chunked}"
            );
        }
    }

    #[tokio::test]
    async fn server_stopped_at_once_accepts_nothing() {
        let socket = MockSocket::with_responses(&[]);
        let server = Server::new(
            ReadyListener::new(socket),
            Stopping(Rc::new(Calls::stopping_after(0))),
        );
        let stopped = server.run_until(core::future::ready(())).await.unwrap();
        assert!(stopped.connections.is_empty());
        // The listener still has the connection that it didn't hand out.
        let mut listener = stopped.listener;
        let accepted = listener::Listener::accept(&mut listener).await.unwrap();
        assert!(accepted.is_some());
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

    // How many calls `Stopping` handled, and the flag that it raises once it handled as many as it
    // was told to, and how many upgraded connections it took over.
    #[derive(Debug)]
    struct Calls {
        handled: Cell<usize>,
        counted: Cell<usize>,
        taken_over: Cell<usize>,
        stop_after: usize,
        stop: Flag,
    }

    impl Calls {
        fn stopping_after(stop_after: usize) -> Self {
            Self {
                handled: Cell::new(0),
                counted: Cell::new(0),
                taken_over: Cell::new(0),
                stop_after,
                stop: Flag::default(),
            }
        }
    }

    // Counts the calls that it handles, streams replies that never come, or a single one that
    // raises the stop, and parks the upgraded connections.
    #[derive(Debug)]
    struct Stopping(Rc<Calls>);

    #[derive(Debug, Deserialize)]
    #[serde(tag = "method")]
    enum StoppingMethod {
        #[serde(rename = "org.example.Count")]
        Count,
        #[serde(rename = "org.example.Stream")]
        Stream,
        #[serde(rename = "org.example.StreamAndStop")]
        StreamAndStop,
        #[serde(rename = "org.example.Upgrade")]
        Upgrade,
    }

    impl Service<MockSocket> for Stopping {
        type MethodCall<'de> = StoppingMethod;
        type ReplyParams<'ser>
            = ()
        where
            Self: 'ser;
        type ReplyStreamParams = ();
        type ReplyStreamError = Infallible;
        type ReplyStream = Replies;
        type ReplyError<'ser>
            = varlink_service::Error<'ser>
        where
            Self: 'ser;
        type Upgrade = Parked;

        async fn handle<'ser>(
            &'ser mut self,
            method: &'ser Call<Self::MethodCall<'_>>,
            _conn: &mut Connection<MockSocket>,
            #[cfg(feature = "std")] _fds: Vec<std::os::fd::OwnedFd>,
        ) -> HandleResult<(), Self::ReplyStream, varlink_service::Error<'ser>, Parked> {
            let calls = &self.0;
            calls.handled.set(calls.handled.get() + 1);
            if calls.handled.get() == calls.stop_after {
                calls.stop.raise();
            }
            let reply = match method.method() {
                StoppingMethod::Count => {
                    calls.counted.set(calls.counted.get() + 1);

                    MethodReply::Single(None)
                }
                StoppingMethod::Stream => MethodReply::Multi(Replies(None)),
                StoppingMethod::StreamAndStop => MethodReply::Multi(Replies(Some(calls.clone()))),
                StoppingMethod::Upgrade => MethodReply::Upgrade(None, Parked(calls.clone())),
            };

            #[cfg(feature = "std")]
            return (reply, Vec::new());
            #[cfg(not(feature = "std"))]
            return reply;
        }
    }

    // The replies that a call of `Stopping` streams: a single one, which raises the stop of the
    // calls as it comes, and then none, ever.
    #[derive(Debug)]
    struct Replies(Option<Rc<Calls>>);

    impl futures_util::Stream for Replies {
        type Item = ReplyStreamItem<(), Infallible>;

        fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            let Some(calls) = self.0.take() else {
                return Poll::Pending;
            };
            calls.stop.raise();
            let reply = Ok(Reply::new(None).set_continues(Some(true)));

            #[cfg(feature = "std")]
            return Poll::Ready(Some((reply, Vec::new())));
            #[cfg(not(feature = "std"))]
            return Poll::Ready(Some(reply));
        }
    }

    // Takes over an upgraded connection, counting it among those of the calls, and keeps it until
    // it's dropped.
    #[derive(Debug)]
    struct Parked(Rc<Calls>);

    impl TakeOver<MockSocket> for Parked {
        fn take_over(self, socket: Upgraded<MockSocket>) -> impl Future<Output = ()> {
            let calls = &self.0;
            calls.taken_over.set(calls.taken_over.get() + 1);

            async move {
                let _socket = socket;
                core::future::pending().await
            }
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
