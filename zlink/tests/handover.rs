//! A service hands its connections over to its next instance, through the file descriptor store of
//! zoter's fake service manager, and the next instance goes on with them, as a service does that
//! restarts under systemd: a client that stays connected throughout gets the replies to its calls
//! from both instances, those that it sent as the first one stopped included, along with their
//! file descriptors. It runs on whichever runtime the features enable, tokio first, as the other
//! tests do. The calls read but not received go over with the credentials that came with them, a
//! reply that the first instance interrupted as it stopped is finished by the next one, and a
//! connection on a local zruntime runtime goes on on one too.

#![cfg(all(feature = "handover", feature = "service", target_os = "linux"))]

use std::{
    cell::{Cell, RefCell},
    future::poll_fn,
    io::{Read, Write},
    os::{fd::OwnedFd, unix::net::UnixStream as StdUnixStream},
    rc::Rc,
    task::{Poll, Waker},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::time::timeout;
use zlink::{
    Call, Connection, ReadyListener, Server, Stopped,
    connection::Handover,
    introspect::{self, CustomType},
};
use zoter::{Restore, Save, Store, testing::Manager};

#[cfg(all(feature = "smol", not(feature = "tokio")))]
use zlink::smol::unix::Stream;
#[cfg(feature = "tokio")]
use zlink::tokio::unix::Stream;
// The shared flavour of zruntime's, as a std socket becomes a stream of either.
#[cfg(all(feature = "zruntime", not(any(feature = "tokio", feature = "smol"))))]
type Stream = zlink::zruntime::unix::Stream;

#[test_log::test(tokio::test(flavor = "current_thread"))]
async fn connections_go_on_in_the_next_instance() {
    let manager = Manager::new().unwrap();
    let (server_end, client_end) = StdUnixStream::pair().unwrap();
    let mut client = Connection::new(Stream::try_from(client_end).unwrap());

    // The first instance stops once it answered the first of two calls that the client sends at
    // once, the second with a file descriptor, which the first instance may read but not handle.
    let stop = Rc::new(Flag::default());
    let listener = ReadyListener::new(Stream::try_from(server_end).unwrap());
    let server = Server::new(listener, CounterService(Counter::new(0, stop.clone())));
    let (read, mut write) = StdUnixStream::pair().unwrap();
    write.write_all(b"passed").unwrap();
    drop(write);
    let calls = async {
        client.enqueue_call(&Call::new(Methods::Count)).unwrap();
        let read_call = Call::new(Methods::Read);
        client
            .write_mut()
            .enqueue_call(&read_call, vec![read.into()])
            .unwrap();
        client.flush().await.unwrap();

        received_count(&mut client).await
    };
    let (stopped, count) = tokio::join!(server.run_until(stop.raised()), calls);
    assert_eq!(count, 1);
    let Stopped {
        listener,
        service,
        connections,
        ..
    } = stopped.unwrap();

    // It hands its count and its connection over to the next instance through the store.
    let first = store(&manager);
    let handovers: Vec<Handover> = connections
        .into_iter()
        .map(|conn| conn.hand_over().unwrap())
        .collect();
    let items = (0..).zip(&handovers);
    let state = service.0.count.get().to_le_bytes();
    let stored = first.handover().unwrap().store(&state[..], items).unwrap();
    assert_eq!(stored.items(), 1);
    drop(handovers);

    // The next instance takes over from it, and answers the second call, with its file
    // descriptor, before the client's next call.
    let mut passed = manager.passed().unwrap();
    let mut restored = store(&manager).take_over(&mut passed).unwrap().unwrap();
    let count = u32::from_le_bytes(restored.state().try_into().unwrap());
    let connections: Vec<Connection<Stream>> = restored
        .take_items()
        .map(|item| Connection::take_over(item.restore::<Handover>().unwrap()).unwrap())
        .collect();
    let stop = Rc::new(Flag::default());
    let server = Server::new(listener, CounterService(Counter::new(count, stop.clone())))
        .with_connections(connections);
    let calls = async {
        let (reply, _) = client.receive_reply::<Text, CounterError>().await.unwrap();
        let text = reply.unwrap().into_parameters().unwrap().text;
        client
            .send_call(&Call::new(Methods::Count), Vec::new())
            .await
            .unwrap();
        let count = received_count(&mut client).await;
        stop.raise();

        (text, count)
    };
    let (stopped, (text, count)) = tokio::join!(server.run_until(stop.raised()), calls);
    assert_eq!(text, "passed");
    assert_eq!(count, 2);
    assert_eq!(stopped.unwrap().connections.len(), 1);
}

/// The credentials that came with calls read but not received go over with them: the call that the
/// next instance receives off what the first one read has those of its sender, as it would without
/// the hand-over.
#[test_log::test(tokio::test(flavor = "current_thread"))]
async fn credentials_of_calls_read_but_not_received_go_over() {
    let (server_end, client_end) = StdUnixStream::pair().unwrap();
    let mut client = Connection::new(Stream::try_from(client_end).unwrap());
    let mut server = Connection::new(Stream::try_from(server_end).unwrap());
    // Two calls at once, which the server reads at once.
    client.enqueue_call(&Call::new(Methods::Count)).unwrap();
    client.enqueue_call(&Call::new(Methods::Count)).unwrap();
    client.flush().await.unwrap();
    server.receive_call::<Methods>().await.unwrap();
    let creds = server.read().received_credentials().unwrap();
    let sender = (creds.process_id(), creds.unix_user_id());

    // The connection goes over through its record, which the next instance takes it over from.
    let (record, fds) = saved(&server.hand_over().unwrap());
    let handover = Handover::restore(&record, fds).unwrap();
    let mut server = Connection::<Stream>::take_over(handover).unwrap();
    server.receive_call::<Methods>().await.unwrap();
    let creds = server.read().received_credentials().unwrap();
    assert_eq!((creds.process_id(), creds.unix_user_id()), sender);
}

/// A reply larger than the socket buffer, to a client that doesn't read it, is interrupted as the
/// first instance stops, and the next instance writes the rest of it once the client reads: the
/// client gets the reply, and its file descriptor, exactly once.
#[test_log::test(tokio::test(flavor = "current_thread"))]
async fn a_reply_cut_off_by_the_stop_goes_on_in_the_next_instance() {
    let (server_end, client_end) = StdUnixStream::pair().unwrap();
    let mut client = Connection::new(Stream::try_from(client_end).unwrap());
    client
        .send_call(&Call::new(Methods::Large), Vec::new())
        .await
        .unwrap();

    // The call raises the stop, so the first instance stops while it waits for the client to read
    // the reply, rather than waiting forever, and hands the connection over with the unwritten part
    // of the reply.
    let stop = Rc::new(Flag::default());
    let listener = ReadyListener::new(Stream::try_from(server_end).unwrap());
    let server = Server::new(listener, CounterService(Counter::new(0, stop.clone())));
    let stopped = timeout(Duration::from_secs(10), server.run_until(stop.raised()));
    let Stopped {
        listener,
        service,
        connections,
        ..
    } = stopped.await.expect("the server didn't stop").unwrap();
    let [conn] = <[_; 1]>::try_from(connections).unwrap();
    let (record, fds) = saved(&conn.hand_over().unwrap());
    let json: serde_json::Value = serde_json::from_slice(&record).unwrap();
    // The file descriptor went with the first bytes of the reply, and the rest of it is unwritten.
    assert_ne!(json["unwritten"], "");
    assert_eq!(json["unwrittenFds"], serde_json::json!([]));

    let conn = Connection::<Stream>::take_over(Handover::restore(&record, fds).unwrap()).unwrap();
    let stop = Rc::new(Flag::default());
    let server = Server::new(
        listener,
        CounterService(Counter::new(service.0.count.get(), stop.clone())),
    )
    .with_connections([conn]);
    let calls = async {
        let (reply, fds) = client.receive_reply::<Text, CounterError>().await.unwrap();
        let text = reply.unwrap().into_parameters().unwrap().text;
        // The reply to the next call comes next.
        client
            .send_call(&Call::new(Methods::Count), Vec::new())
            .await
            .unwrap();
        let count = received_count(&mut client).await;

        (text, fds, count)
    };
    let (stopped, (text, fds, count)) = tokio::join!(server.run_until(stop.raised()), calls);
    stopped.unwrap();
    assert!(text == large_text(), "a text of {} bytes", text.len());
    let [fd] = <[_; 1]>::try_from(fds).unwrap();
    let mut passed = String::new();
    StdUnixStream::from(fd).read_to_string(&mut passed).unwrap();
    assert_eq!(passed, "passed");
    assert_eq!(count, 1);
}

/// A connection on a local zruntime runtime is taken over on the local runtime of the thread that
/// takes it over, where it goes on.
#[cfg(feature = "zruntime")]
#[test]
fn a_connection_on_a_local_runtime_goes_on() {
    type LocalStream = zlink::zruntime::unix::Stream<zruntime::Local>;

    zruntime::LocalRuntime::new().unwrap().block_on(async {
        let (server_end, client_end) = StdUnixStream::pair().unwrap();
        let mut client = Connection::new(LocalStream::try_from(client_end).unwrap());
        let server = Connection::new(LocalStream::try_from(server_end).unwrap());
        client
            .send_call(&Call::new(Methods::Count), Vec::new())
            .await
            .unwrap();

        let handover = server.hand_over().unwrap();
        let mut server = Connection::<LocalStream>::take_over(handover).unwrap();
        let (call, _) = server.receive_call::<Methods>().await.unwrap();
        assert!(matches!(call.method(), Methods::Count));
    });
}

/// The count that the next reply on `client` gives.
async fn received_count(client: &mut Connection<Stream>) -> u32 {
    let (reply, _) = client.receive_reply::<Count, CounterError>().await.unwrap();

    reply.unwrap().into_parameters().unwrap().count
}

/// The record of `handover`, and duplicates of its file descriptors, as zoter saves them.
fn saved(handover: &Handover) -> (Vec<u8>, Vec<OwnedFd>) {
    let mut record = vec![0; handover.saved_len().unwrap()];
    handover.save(&mut record).unwrap();
    let fds = handover
        .fds()
        .iter()
        .map(|fd| fd.try_clone_to_owned().unwrap())
        .collect();

    (record, fds)
}

/// A text larger than the buffer of a socket.
fn large_text() -> String {
    "x".repeat(4 << 20)
}

/// A store of the manager, under the key that the instances hand over under.
fn store(manager: &Manager) -> Store {
    Store::new(
        manager.notifier().unwrap(),
        "counter",
        Some(manager.store_max()),
    )
    .unwrap()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "method")]
enum Methods {
    #[serde(rename = "org.example.counter.Count")]
    Count,
    #[serde(rename = "org.example.counter.Read")]
    Read,
    #[serde(rename = "org.example.counter.Large")]
    Large,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct Count {
    count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, CustomType)]
struct Text {
    text: String,
}

#[derive(Debug, Clone, PartialEq, zlink::ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.example.counter")]
enum CounterError {
    NoFd,
}

/// The service of the `Counter` interface.
#[derive(zlink::Service)]
#[zlink(
    vendor = "zlink tests",
    product = "counter",
    version = "1",
    url = "https://example.invalid/"
)]
struct CounterService(Counter);

/// Counts the calls to `count`, reads the contents of the file descriptor passed to `read`, and
/// answers `large` with a large text and a file descriptor. It raises `stop` as it answers its
/// first call to `count`, and calls to `large`.
struct Counter {
    count: Cell<u32>,
    stop: Rc<Flag>,
}

impl Counter {
    fn new(count: u32, stop: Rc<Flag>) -> Self {
        Self {
            count: Cell::new(count),
            stop,
        }
    }
}

#[zlink::interface(interface = "org.example.counter", types = [Count, Text])]
impl Counter {
    async fn count(&self) -> Count {
        let count = self.count.get() + 1;
        self.count.set(count);
        if count == 1 {
            self.stop.raise();
        }

        Count { count }
    }

    async fn read(&self, #[zlink(fds)] fds: Vec<OwnedFd>) -> Result<Text, CounterError> {
        let fd = fds.into_iter().next().ok_or(CounterError::NoFd)?;
        let mut text = String::new();
        StdUnixStream::from(fd).read_to_string(&mut text).unwrap();

        Ok(Text { text })
    }

    /// A text larger than the buffer of a socket, with a file descriptor to read `passed` from.
    #[zlink(return_fds)]
    async fn large(&self) -> (Text, Vec<OwnedFd>) {
        self.stop.raise();
        let (read, mut write) = StdUnixStream::pair().unwrap();
        write.write_all(b"passed").unwrap();

        (Text { text: large_text() }, vec![read.into()])
    }
}

/// A flag that futures can wait to be raised.
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
