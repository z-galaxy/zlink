pub(crate) mod listener;
mod select_all;
pub mod service;

use alloc::vec::Vec;
use futures_util::{FutureExt, StreamExt};
use select_all::SelectAll;
use service::MethodReply;

use crate::{
    connection::{ReadConnection, Socket, WriteConnection},
    Call, Connection, Reply,
};

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
    Service: service::Service,
{
    /// Create a new server that serves `service` to incomming connections from `listener`.
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
    ///   [`tokio::task::LocalSet`]) to run the server in a local task, perhaps in a seprate thread.
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
        let mut readers = Vec::new();
        let mut writers = Vec::new();
        let mut reply_streams = Vec::<ReplyStream<Service::ReplyStream, Listener::Socket>>::new();
        let mut last_reply_stream_winner = None;
        let mut last_method_call_winner = None;

        loop {
            let mut reply_stream_futures: Vec<_> =
                reply_streams.iter_mut().map(|s| s.stream.next()).collect();
            let start_index = last_reply_stream_winner.map(|idx| idx + 1);
            let mut reply_stream_select_all = SelectAll::new(start_index);
            for future in reply_stream_futures.iter_mut() {
                reply_stream_select_all.push(future);
            }

            futures_util::select_biased! {
                // 1. Accept a new connection.
                conn = listener.accept().fuse() => {
                    let conn = conn?;
                    let (read, write) = conn.split();
                    readers.push(read);
                    writers.push(write);
                }
                // 2. Read method calls from the existing connections and handle them.
                res = self.get_next_call(
                    // SAFETY: `readers` is not invalidated or dropped until the output of this
                    // future is dropped.
                    unsafe { &mut *(&mut readers as *mut _) },
                    last_method_call_winner.map(|idx| idx + 1),
                ).fuse() => {
                        let (idx, call) = res?;
                        last_method_call_winner = Some(idx);

                        let mut stream = None;
                        let mut remove = true;
                        match call {
                            Ok(call) => match self.handle_call(call, &mut writers[idx]).await {
                                Ok(None) => remove = false,
                                Ok(Some(s)) => stream = Some(s),
                                Err(e) => warn!("Error writing to connection: {:?}", e),
                            },
                            Err(e) => warn!("Error reading from socket: {:?}", e),
                        }

                        if stream.is_some() || remove {
                            let reader = readers.remove(idx);
                            let writer = writers.remove(idx);

                            if let Some(stream) = stream.map(|s| ReplyStream::new(s, reader, writer)) {
                                reply_streams.push(stream);
                            }
                        }
                }
                // 3. Read replies from the reply streams and send them off.
                reply = reply_stream_select_all.fuse() => {
                    let (idx, reply) = reply;
                    last_reply_stream_winner = Some(idx);
                    let id = reply_streams.get(idx).unwrap().conn.id();

                    match reply {
                        Some(reply) => {
                            if let Err(e) = reply_streams
                                .get_mut(idx)
                                .unwrap()
                                .conn
                                .write_mut()
                                .send_reply(&reply)
                                .await
                            {
                                warn!("Error writing to client {}: {:?}", id, e);
                                reply_streams.remove(idx);
                            }
                        }
                        None => {
                            trace!("Stream closed for client {}", id);
                            let stream = reply_streams.remove(idx);

                            let (read, write) = stream.conn.split();
                            readers.push(read);
                            writers.push(write);
                        }
                    }
                }
            }
        }
    }

    /// Read the next method call from the connection.
    ///
    /// # Return value
    ///
    /// On success, this method returns a tuple containing:
    ///
    /// * The index of the reader that yielded a call.
    /// * A Result, containing a method call if reading was successful.
    async fn get_next_call<'r>(
        &mut self,
        readers: &'r mut Vec<
            ReadConnection<<<Listener as crate::Listener>::Socket as Socket>::ReadHalf>,
        >,
        start_index: Option<usize>,
    ) -> crate::Result<(usize, crate::Result<Call<Service::MethodCall<'r>>>)> {
        let mut read_futures: Vec<_> = readers.iter_mut().map(|r| r.receive_call()).collect();
        let mut select_all = SelectAll::new(start_index);
        for future in &mut read_futures {
            // Safety: `future` is in fact `Unpin` but the compiler doesn't know that.
            unsafe {
                select_all.push_unchecked(future);
            }
        }

        Ok(select_all.await)
    }

    async fn handle_call(
        &mut self,
        call: Call<Service::MethodCall<'_>>,
        writer: &mut WriteConnection<<Listener::Socket as Socket>::WriteHalf>,
    ) -> crate::Result<Option<Service::ReplyStream>> {
        let mut stream = None;
        match self.service.handle(call).await {
            MethodReply::Single(params) => {
                let reply = Reply::new(params).set_continues(Some(false));
                writer.send_reply(&reply).await?
            }
            MethodReply::Error(err) => writer.send_error(&err).await?,
            MethodReply::Multi(s) => {
                trace!("Client {} now turning into a reply stream", writer.id());
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
    fn new(
        stream: St,
        read_conn: ReadConnection<Sock::ReadHalf>,
        write_conn: WriteConnection<Sock::WriteHalf>,
    ) -> Self {
        Self {
            stream,
            conn: Connection::join(read_conn, write_conn),
        }
    }
}
