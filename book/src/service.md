# Writing a service

<!-- toc -->

In this chapter, we are going to implement a small service with a method `SayHello`, to greet back
the calling client — and then grow it into something more interesting.

## No name to take — just bind a socket

Readers coming from D-Bus may expect a section here about requesting a well-known name on the bus
and its associated pitfalls. There is no such thing in the Varlink world: your service *is* the
socket it listens on. Setting up shop is exactly two steps — bind a listener and hand it to a
`Server` together with your service implementation:

```rust,noplayground
# struct Greeter;
# #[derive(Debug, serde::Serialize, serde::Deserialize, zlink::introspect::Type)]
# struct Greeting { greeting: String }
# #[zlink::service(interface = "org.zlink.MyGreeter")]
# impl Greeter {
#     async fn say_hello(&self, name: String) -> Greeting {
#         Greeting { greeting: format!("Hello {name}!") }
#     }
# }
# async fn example(greeter: Greeter) -> Result<(), Box<dyn std::error::Error>> {
let listener = zlink::tokio::unix::bind("/run/user/1000/org.zlink.MyGreeter")?;
let server = zlink::Server::new(listener, greeter);
server.run().await?;
# Ok(())
# }
```

There is no "hang on, incoming calls might be lost" window either: as soon as `bind` returns, the
socket exists and clients can connect; their calls simply queue up until `run` starts servicing
them.

## The `service` macro

The `service` attribute macro turns an ordinary `impl` block into a full Varlink service — it
generates the message handling, method dispatch, and introspection support:

```rust,no_run
use zlink::{service, tokio::unix, Server};
use serde::{Deserialize, Serialize};
use zlink::introspect;

struct Greeter;

#[derive(Debug, Serialize, Deserialize, introspect::Type)]
struct Greeting {
    greeting: String,
}

#[service(interface = "org.zlink.MyGreeter")]
impl Greeter {
    async fn say_hello(&self, name: &str) -> Greeting {
        Greeting {
            greeting: format!("Hello {name}!"),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = std::env::var("XDG_RUNTIME_DIR").map(std::path::PathBuf::from)?
        .join("org.zlink.MyGreeter");
    let listener = unix::bind(&socket_path)?;
    Server::new(listener, Greeter).run().await?;

    Ok(())
}
```

Let's greet the service from the shell:

```bash
$ varlinkctl call $XDG_RUNTIME_DIR/org.zlink.MyGreeter org.zlink.MyGreeter.SayHello '{"name": "zlink"}'
{
        "greeting" : "Hello zlink!"
}
```

And it can introspect itself, out of the box:

```bash
$ varlinkctl introspect $XDG_RUNTIME_DIR/org.zlink.MyGreeter org.zlink.MyGreeter
interface org.zlink.MyGreeter

method SayHello(name: string) -> (greeting: string)
```

Some things to note:

* Method names are converted from snake_case to PascalCase on the wire, parameters keep their names
  (`#[zlink(rename = "...")]` overrides either).
* Since Varlink methods return *named* output parameters and Rust has no anonymous structs, the
  reply is wrapped in a struct deriving `introspect::Type`; the macro unwraps its fields into the
  method's output parameters (`-> (greeting: string)`), both on the wire and in the IDL above.
* Types that should appear as *named* custom types in the IDL derive `introspect::CustomType`
  instead, and must be listed in `types = [...]` on the macro. Forgetting one is a *compile-time*
  error, so you can't accidentally publish an incomplete interface description.
* The `interface` can also be specified per-method with `#[zlink(interface = "...")]`; it
  propagates to subsequent methods, which is how a single service can implement **multiple
  interfaces**.

### ⚠ `Server::run` and `tokio::spawn`

Due to a [compiler bug][rustc-100013], the future returned by `Server::run` cannot currently be
`Send`, which means you can't `tokio::spawn(server.run())`. Run it on a
[`LocalSet`]/[`LocalRuntime`] (or a dedicated thread), or drive it concurrently with your other
work using `select!`:

```rust,noplayground
# async fn example<Svc: zlink::Service<zlink::tokio::unix::Stream>>(
#     server: zlink::Server<zlink::tokio::unix::Listener, Svc>,
# ) -> Result<(), Box<dyn std::error::Error>> {
# async fn do_other_things() -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
tokio::select! {
    res = server.run() => res?,
    res = do_other_things() => res?,
}
# Ok(())
# }
```

The examples and tests in the zlink repository all follow this pattern.

## Service state: mutable, without locks

Here's something that tends to raise eyebrows when people first see it. Let's give our service some
state:

```rust,noplayground
# use zlink::{introspect, service};
# #[derive(Debug, serde::Serialize, serde::Deserialize, introspect::Type)]
# struct Count { count: u64 }
struct Counter {
    count: u64,
}

#[service(interface = "org.zlink.Counter")]
impl Counter {
    async fn increment(&mut self) -> Count {
        self.count += 1; // <- plain mutation. No Mutex. No RwLock. No atomics.
        Count { count: self.count }
    }
}
```

That's a `&mut self` method mutating a plain field — no `Arc<Mutex<...>>` in sight. In zbus, an
interface method blocking on `&mut self` for too long famously stalls all other method calls on
that interface, and the recommended fix is interior mutability. zlink takes the opposite stance,
which we call **exterior mutability**: the `Server` drives all connections from a single task, so
at any moment at most one method handler is running, and it may freely get exclusive access to the
state. The compiler statically guarantees there is no concurrent access — no locks, no lock
contention, no deadlocks, at zero runtime cost.

"But doesn't one task limit throughput?" For the vast majority of control-plane services, no — and
the [Design for efficiency](design.html) chapter explains why this model, combined with cheap
connections and pipelining, usually comes out *ahead*. If a method genuinely needs long-running
work, spawn a task for it and stream progress back through a
[streaming method](#streaming-methods).

## Method errors

To return errors from methods, declare an error enum (the same `ReplyError` +
`introspect::ReplyError` pair we saw in the [client chapter](client.html#reply-errors)) and return
a `Result`:

```rust,noplayground
use zlink::{ReplyError, introspect, service};

#[derive(Debug, ReplyError, introspect::ReplyError)]
#[zlink(interface = "org.zlink.Counter")]
enum CounterError {
    Overflow,
}

# struct Counter { count: u64 }
# #[derive(Debug, serde::Serialize, serde::Deserialize, introspect::Type)]
# struct Count { count: u64 }
#[service(interface = "org.zlink.Counter")]
impl Counter {
    async fn increment(&mut self) -> Result<Count, CounterError> {
        self.count = self.count.checked_add(1).ok_or(CounterError::Overflow)?;
        Ok(Count { count: self.count })
    }
}
```

An `Err` return is sent as a Varlink error reply — for the wire format, see the
[concepts chapter](concepts.html#errors). Different methods can use different error types; the
macro unifies them internally.

## Streaming methods

A method annotated with `#[zlink(more)]` implements Varlink's streaming — the service-side
counterpart of what D-Bus offers through signals and property-change notifications. Such a method:

* takes `more: bool` as its first parameter (after `self`) — the flag from the incoming call,
* returns a `Stream` of `Reply` values (each with `continues` set appropriately).

The `notified::State` helper makes the most common case — "let clients monitor a value" — a
one-liner. Here's the relevant part of the FTL (faster-than-light drive 🚀) example from zlink's
test suite:

```rust,noplayground
use zlink::tokio::notified::{self, traits::State as _};
# use zlink::service;
# #[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, zlink::introspect::CustomType)]
# struct DriveCondition { tylium_level: i64 }

struct Ftl {
    drive_condition: notified::State<DriveCondition, DriveCondition>,
}

#[service(interface = "org.example.ftl", types = [DriveCondition])]
impl Ftl {
    /// When `more` is false, reply once with the current condition;
    /// when true, stream every change as it happens.
    #[zlink(more)]
    async fn get_drive_condition(&self, more: bool) -> notified::Stream<DriveCondition> {
        if more {
            self.drive_condition.stream()
        } else {
            self.drive_condition.stream_once()
        }
    }

    async fn set_drive_condition(&mut self, condition: DriveCondition) -> DriveCondition {
        self.drive_condition.set(condition).await;
        condition
    }
}
```

Every client that called `GetDriveCondition` with the `more` flag receives a new reply whenever
`set` is called. While a stream is active, the `Server` keeps serving other calls and other
connections; the streaming connection is simply parked in the server's event loop.

For full control, a streaming method can also return any custom
`impl Stream<Item = Reply<T>>` (or `Item = Result<Reply<T>, E>` to be able to emit errors
mid-stream), constructing replies with `Reply::new(...).set_continues(...)` yourself.

A client can monitor the drive condition with:

```bash
varlinkctl call --more /run/org.example.ftl org.example.ftl.GetDriveCondition '{}'
```

## Upgrading connections

Some calls are better served by a protocol other than JSON messages, for example to stream large
amounts of data or to pass file descriptors around. That's what the `upgrade` flag of a call is
for: it asks for the connection to be taken over by a custom protocol after the call. The service
confirms the upgrade with a single successful reply, after which neither side speaks Varlink on the
connection anymore. An error reply refuses the upgrade instead, and the connection remains a
Varlink one.

A service marks the methods that upgrade their connections with `#[zlink(upgrade)]`. Along with its
reply, such a method returns the state that takes the upgraded connection over: a value of a type
that implements `TakeOver`, whose `take_over` method gets the socket of the connection and returns
the future that speaks the new protocol over it. Here's a service that sends a blob over the
connection it upgrades:

```rust,noplayground
use serde::Serialize;
use std::os::fd::OwnedFd;
use zlink::{
    connection::socket::{Socket, WriteHalf},
    introspect, service,
    service::{TakeOver, Upgraded},
};

#[derive(Debug, Serialize, introspect::Type)]
struct Download {
    size: u64,
}

struct Blobs {
    blob: Vec<u8>,
}

#[service(interface = "org.example.Blobs")]
impl Blobs {
    /// Replies with the size of the blob, which is then sent over the upgraded connection.
    #[zlink(upgrade)]
    async fn download(&self) -> (Download, SendBlob) {
        let blob = self.blob.clone();
        let download = Download {
            size: blob.len() as u64,
        };

        (download, SendBlob(blob))
    }
}

struct SendBlob(Vec<u8>);

impl<Sock> TakeOver<Sock> for SendBlob
where
    Sock: Socket,
{
    async fn take_over(self, socket: Upgraded<Sock>) {
        let (_read, mut write) = socket.split();
        let fds = &[] as &[OwnedFd];
        let _ = write
            .write(&self.0, fds, #[cfg(target_os = "linux")] None)
            .await;
    }
}
```

Some things to note:

* The state carries what the new protocol needs from the call, like the blob above, so the service
  doesn't have to store anything for it. `take_over` gets the socket of the upgraded connection, an
  `Upgraded<Sock>`, and returns the future that speaks the new protocol over it. The `Server` polls
  the future along with all the other connections until it completes, so it doesn't need a task of
  its own. The future can't borrow from the service though, since the server keeps calling into the
  service meanwhile: move what it needs into the state.
* An `Upgraded<Sock>` is a `Socket` itself, and `split` gives the halves to read from and write to.
  The client doesn't have to wait for the reply before it starts talking the new protocol, so the
  server may have read some of its data along with the call: the read half returns that data first.
* The future is `Send` whenever the state and the futures of the socket are, so a server with
  upgrade methods can be `Send` as well, once the [compiler bug](#-serverrun-and-tokiospawn) that
  keeps `Server::run` from being `Send` is fixed.
* All the upgrade methods return the same type of state. To upgrade to several protocols, make it
  an enum with a variant for each, whose `take_over` matches on itself.
* The reply confirming the upgrade has already been sent by the time the state takes over, so it
  can't refuse the upgrade anymore. Do the checks that the upgrade depends on, e.g. of the peer's
  credentials, in the upgrade method instead, using a `#[zlink(connection)]` parameter, and return
  an error to refuse the upgrade.
* The macro refuses what wouldn't make sense, without calling the method: a call to an upgrade
  method that doesn't ask for an upgrade gets the `ExpectedUpgrade` error that systemd's services
  reply with as well, and a call that asks for an upgrade of any other method gets the standard
  `InvalidParameter` error naming `upgrade`. One-way calls to an upgrade method aren't served
  either, since they can't get a reply that confirms the upgrade. Upgrade methods can't be combined
  with `#[zlink(more)]`, since a single reply confirms an upgrade, or `#[zlink(return_fds)]`: pass
  file descriptors over the upgraded connection instead.

On the client side, mark the proxy method with `#[zlink(upgrade)]` too. If the call returns a
successful reply, the upgrade is confirmed and the socket can be taken out of the connection. The
connection reads from the socket in chunks, so the reply may well come with the beginning of the new
protocol's data: the read half that `into_read_half` returns hands it out first, ahead of what is
left to read from the socket.

```rust,noplayground
use zlink::{connection::socket::ReadHalf, proxy};
# #[derive(Debug, serde::Deserialize)]
# struct Download { size: u64 }
# #[derive(Debug, zlink::ReplyError)]
# #[zlink(interface = "org.example.Blobs")]
# enum BlobError { NotFound }

#[proxy("org.example.Blobs")]
trait BlobsProxy {
    #[zlink(upgrade)]
    async fn download(&mut self) -> zlink::Result<Result<Download, BlobError>>;
}

async fn download(
    mut conn: zlink::tokio::unix::Connection,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let download = conn.download().await?.map_err(|e| format!("{e:?}"))?;

    // The upgrade is confirmed so the connection doesn't speak Varlink anymore.
    let (read, _write) = conn.split();
    let mut read = read.into_read_half();
    let mut blob = Vec::new();
    let mut buf = [0; 4096];
    while (blob.len() as u64) < download.size {
        let len = read.read(&mut buf).await?.bytes_read();
        if len == 0 {
            return Err("the connection was closed before the blob was received".into());
        }
        blob.extend_from_slice(&buf[..len]);
    }

    Ok(blob)
}
```

An error reply means the service refused the upgrade, which leaves a working Varlink connection.
That is why proxy upgrade methods take `&mut self`, and why the data of the new protocol is best
not sent before the reply has confirmed the upgrade: it would break the Varlink connection if the
upgrade is refused.

## Introspection for free

The macro implements the standard `org.varlink.service` interface for you:

* **`GetInfo`** returns the service metadata you declare on the macro:

  ```rust,noplayground
  # struct Ftl;
  # #[derive(Debug, serde::Serialize, serde::Deserialize, zlink::introspect::Type)]
  # struct Speed { speed: i64 }
  #[zlink::service(
      interface = "org.example.ftl",
      vendor = "The FTL project",
      product = "FTL-capable Spaceship 🚀",
      version = "1",
      url = "https://example.org/ftl",
  )]
  impl Ftl {
      // ...methods...
  #     async fn get_speed(&self) -> Speed { Speed { speed: 42 } }
  }
  ```

* **`GetInterfaceDescription`** returns the IDL of any implemented interface, generated at compile
  time from your method signatures and the `types = [...]` list.
* Calls to unknown methods or interfaces automatically get the standard `MethodNotFound` /
  `InterfaceNotFound` error replies.

## Socket activation and inherited sockets

Beyond `unix::bind`, a `Listener` can be created from an inherited file descriptor via
`TryFrom<OwnedFd>` — handy for systemd socket activation (`Accept=no`). For the `Accept=yes` style
(and `varlinkctl`'s `exec:` addresses), where the process is handed a single already-connected
socket, `ReadyListener` wraps that one connection; `Server::run` then serves it and returns cleanly
when the client disconnects.

## Discoverable sockets

On Linux, zlink tags every socket it creates with a `user.varlink` extended attribute, following
the convention systemd's `sd-varlink` established in v262: the socket inode `bind` creates is
marked `entrypoint`, the listener itself `listen`, connected client sockets `client` and accepted
ones `server`. This is what lets `varlinkctl list-sockets` enumerate the Varlink services on a
system, and what future eBPF-based tracing of Varlink traffic will key on. Tagging is best effort:
kernels older than Linux 7.1 do not allow extended attributes on sockets and are silently tolerated.
Extended attributes are only readable by users who may read the inode itself, so a restrictive
umask at `bind` time also hides the tag from other users. Like `sd-varlink`, zlink only tags the
inode when `bind` was given an absolute path; listeners adopted from a file descriptor or bound
to a relative path only get the `listen` tag, since their inode cannot be located safely.

A service can publish attributes of its own on its entrypoint inode with `Listener::set_xattr`, so
that clients and tooling learn something about it before connecting. systemd's user database, for
instance, reads `user.userdb.uid` and friends to skip providers that cannot serve a query. The
method is part of the `Listener` trait and, like the attributes themselves, only exists on Linux.
It fails with `Unsupported` for listeners without a socket of their own such as `ReadyListener`,
and with `InvalidInput` for a listener adopted from a file descriptor or bound to a relative path,
whose entrypoint inode is unknown.

```rust,noplayground
# use zlink::Listener as _;
# fn example() -> Result<(), Box<dyn std::error::Error>> {
let listener = zlink::tokio::unix::bind("/run/user/1000/io.example.Accounts")?;
#[cfg(target_os = "linux")]
if let Err(e) = listener.set_xattr("user.userdb.uid", "1000-1999") {
    eprintln!("could not publish the UID range: {e}");
}
# Ok(())
# }
```

## Under the hood: the `Service` trait

The `service` macro is sugar for implementing the `zlink::Service` trait, whose `handle` method
receives each incoming `Call` and returns a `MethodReply` (single reply, error, stream, or upgrade).
An upgrade reply comes with a value of the trait's `Upgrade` type, which implements `TakeOver` to
take the connection over once the reply is sent. The macro sets that type to the state that the
upgrade methods return, and to `()`, which just closes the connection, if there are none. You can
implement the trait by hand for maximum control — the macro-generated introspection then becomes
your responsibility — but for nearly all services, the macro is the way to go.

[rustc-100013]: https://github.com/rust-lang/rust/issues/100013
[`LocalSet`]: https://docs.rs/tokio/latest/tokio/task/struct.LocalSet.html
[`LocalRuntime`]: https://docs.rs/tokio/latest/tokio/runtime/struct.LocalRuntime.html
