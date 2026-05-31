pub(crate) mod listener;
mod select_all;
pub mod service;

mod infallible;

use alloc::vec::Vec;
use futures_util::{FutureExt, StreamExt};
use select_all::SelectAll;
use service::MethodReply;

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
        let mut last_reply_stream_winner = None;
        let mut last_method_call_winner = None;
        let mut listener_closed = false;

        loop {
            if listener_closed && connections.is_empty() && reply_streams.is_empty() {
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

            futures_util::select_biased! {
                // 1. Accept a new connection.
                conn = listener.accept().fuse() => match conn? {
                    Some(conn) => connections.push(conn),
                    None => listener_closed = true,
                },
                // 2. Read method calls from the existing connections and handle them.
                (idx, result) = read_select_all.fuse() => {
                        #[cfg(feature = "std")]
                        let (call, fds) = match result {
                            Ok((call, fds)) => (Ok(call), fds),
                            Err(e) => (Err(e), alloc::vec![]),
                        };
                        #[cfg(not(feature = "std"))]
                        let call = result;
                        last_method_call_winner = Some(idx);

                        let mut stream = None;
                        let mut remove = true;
                        match call {
                            Ok(call) => {
                                let is_upgrade = call.upgrade();
                                #[cfg(feature = "std")]
                                let handles_upgrade = <Service as service::Service<Listener::Socket>>::HANDLES_UPGRADE;
                                #[cfg(not(feature = "std"))]
                                let handles_upgrade = false;

                                if is_upgrade && !handles_upgrade {
                                    // The service doesn't support upgrades: reply with an error and
                                    // keep the connection in JSON mode (don't hand off, don't drop
                                    // on success). On a send failure, fall through to removal via
                                    // the default `remove = true`.
                                    let error = crate::varlink_service::Error::MethodNotImplemented {
                                        method: alloc::borrow::Cow::Borrowed("upgrade"),
                                    };
                                    #[cfg(feature = "std")]
                                    let send_err_res = connections[idx].send_error(&error, alloc::vec![]).await;
                                    #[cfg(not(feature = "std"))]
                                    let send_err_res = connections[idx].send_error(&error).await;
                                    match send_err_res {
                                        Ok(()) => remove = false,
                                        Err(e) => warn!("Error sending upgrade error reply: {:?}", e),
                                    }
                                } else {
                                    #[cfg(feature = "std")]
                                    let result =
                                        self.handle_call(call, &mut connections[idx], fds).await;
                                    #[cfg(not(feature = "std"))]
                                    let result =
                                        self.handle_call(call, &mut connections[idx]).await;
                                    match result {
                                        Ok(None) => {
                                            // The method's own reply has been sent. For an upgrade
                                            // call (only reachable when the service handles it),
                                            // hand the now-raw connection off to `on_upgrade`. In
                                            // both cases the connection is no longer managed by the
                                            // normal removal path below.
                                            remove = false;
                                            #[cfg(feature = "std")]
                                            if is_upgrade {
                                                let conn = connections.swap_remove(idx);
                                                let parts = conn.into_parts();
                                                if let Err(e) = self.service.on_upgrade(parts).await {
                                                    warn!("Upgrade handler failed: {:?}", e);
                                                }
                                            }
                                        }
                                        Ok(Some(s)) => stream = Some(s),
                                        Err(e) => warn!("Error writing to connection: {:?}", e),
                                    }
                                }
                            }
                            Err(e) => debug!("Error reading from socket: {:?}", e),
                        }

                        if stream.is_some() || remove {
                            let conn = connections.swap_remove(idx);

                            if let Some(stream) = stream {
                                reply_streams.push(ReplyStream::new(stream, conn));
                            }
                        }
                }
                // 3. Read replies from the reply streams and send them off.
                reply = reply_stream_select_all.fuse() => {
                    let (idx, item) = reply;
                    last_reply_stream_winner = Some(idx);
                    let id = reply_streams[idx].conn.id();

                    match item {
                        Some(item) => {
                            #[cfg(feature = "std")]
                            let (item, fds) = item;
                            #[cfg(feature = "std")]
                            let send_result = match item {
                                Ok(reply) => reply_streams[idx]
                                    .conn
                                    .send_reply(&reply, fds)
                                    .await,
                                Err(error) => reply_streams[idx]
                                    .conn
                                    .send_error(&error, fds)
                                    .await,
                            };
                            #[cfg(not(feature = "std"))]
                            let send_result = match item {
                                Ok(reply) => reply_streams[idx].conn.send_reply(&reply).await,
                                Err(error) => reply_streams[idx].conn.send_error(&error).await,
                            };
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
    ) -> crate::Result<Option<Service::ReplyStream>> {
        let mut stream = None;

        #[cfg(feature = "std")]
        let (reply, reply_fds) = self.service.handle(&call, conn, fds).await;
        #[cfg(not(feature = "std"))]
        let reply = self.service.handle(&call, conn).await;

        match reply {
            // Don't send replies or errors for oneway calls.
            MethodReply::Single(_) | MethodReply::Error(_) if call.oneway() => (),
            MethodReply::Single(params) => {
                let reply = Reply::new(params).set_continues(Some(false));
                #[cfg(feature = "std")]
                conn.send_reply(&reply, reply_fds).await?;
                #[cfg(not(feature = "std"))]
                conn.send_reply(&reply).await?;
            }
            #[cfg(feature = "std")]
            MethodReply::Error(err) => conn.send_error(&err, reply_fds).await?,
            #[cfg(not(feature = "std"))]
            MethodReply::Error(err) => conn.send_error(&err).await?,
            MethodReply::Multi(s) => {
                trace!("Client {} now turning into a reply stream", conn.id());
                stream = Some(s)
            }
        }

        Ok(stream)
    }
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
