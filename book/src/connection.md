# Establishing connections

<!-- toc -->

The first thing you will have to do is to connect to a Varlink service. This is the entry point of
the zlink API.

## Picking an async runtime

zlink is async-first and runtime-agnostic at its core: it spawns no tasks of its own, so its
connections and servers make progress as their futures are polled, by whichever executor polls
them. It has ready-made integration for [zruntime], the default, and for the two popular runtimes,
tokio and smol, enabled through cargo features:

```toml
# zruntime (the default):
zlink = "0.8"

# ...or tokio:
zlink = { version = "0.8", default-features = false, features = [
    "tokio", "service", "proxy", "tracing",
] }

# ...or smol:
zlink = { version = "0.8", default-features = false, features = [
    "smol", "service", "proxy", "tracing",
] }
```

The features are additive — you can enable several at once. The bulk of the API is runtime-agnostic
and lives at the crate root; only the runtime-specific parts (`block_on` and the `unix` and
`notified` modules) are per-runtime, under `zlink::zruntime`, `zlink::tokio` and `zlink::smol`
respectively. The examples in this book use zruntime, but they work identically with the other
runtimes — just replace `zlink::zruntime` with `zlink::tokio` or `zlink::smol`. If you enable none
of these features, the crate fails to compile with an explicit error, since the transport
implementations live in the runtime-integration crates.

A program with no async runtime of its own drives zlink with the `block_on` of a runtime module,
which runs a future to completion on the calling thread: put the async code in one call to it, as
the examples in this book do with [`zlink::zruntime::block_on`]. zruntime is a small,
single-threaded runtime: between polls of the future, the thread also runs the I/O of the sockets
that `zlink::zruntime::unix::connect` and `bind` make inside the call. Those sockets can be used
from any other executor as well: where no `zlink::zruntime::block_on` call drives them, a helper
thread of zruntime does. `connect_local` and `bind_local` make them on the `LocalRuntime` the
calling thread is running instead, and those stay on that thread. The sockets of `zlink::smol` can
be used from any executor too, while those of `zlink::tokio` need a Tokio runtime: that of
`zlink::tokio::block_on`, or the program's own.

## Connecting to a service

As we saw in the [previous chapter](concepts.html), there is no bus to connect to — you connect
directly to the service you're interested in, through the socket it listens on:

```rust,no_run
fn main() -> zlink::Result<()> {
    zlink::zruntime::block_on(run())
}

async fn run() -> zlink::Result<()> {
    let mut conn = zlink::zruntime::unix::connect("/run/systemd/resolve/io.systemd.Resolve").await?;

    // ... use the connection ...
    Ok(())
}
```

That's it. No handshake, no authentication round-trips, no name registration: after `connect`
returns, you can immediately send your first method call. The returned type is
`zlink::zruntime::unix::Connection`, an alias for `zlink::Connection<zlink::zruntime::unix::Stream>`
— the `Connection` type is generic over its transport (more on that in the [embedded
chapter](embedded.html)).

Note that `connect()` gives you the connection *by value* and all its methods take `&mut self`.
This is a deliberate — and central — design decision in zlink: a connection is exclusively owned by
whoever is using it, and is not meant to be shared. If two tasks need to talk to the same service,
each opens its own connection. Since Varlink connections are cheap, this costs you very little and
buys you a lot; the [Design for efficiency](design.html) chapter explains why this is one of the
main reasons zlink can outperform bus-based IPC.

## Low-level method calls

For most use cases you'll want the high-level [`proxy`](client.html) and [`interface`](service.html)
macros, but let's start at the bottom to understand what the machinery looks like. `Connection`
provides `send_call`, `receive_reply` and the combined `call_method` for the client side.

Method calls are represented by the `Call<M>` type, where `M` is any serializable type that
produces the `method` and `parameters` fields of the [wire format](concepts.html#method-calls). An
enum with serde's `tag`/`content` attributes fits this shape perfectly, and the
[`serde_prefix_all`] crate saves us from repeating the interface name on every variant:

```rust,no_run
use serde::{Deserialize, Serialize};
use serde_prefix_all::prefix_all;
use zlink::{Call, reply};

// The methods of the `io.systemd.Resolve` interface (well, two of them).
#[prefix_all("io.systemd.Resolve.")]
#[derive(Debug, Serialize)]
#[serde(tag = "method", content = "parameters")]
enum ResolveMethods<'m> {
    ResolveHostname { name: &'m str },
    ResolveAddress { address: &'m [u8] },
}

// The relevant fields of a `ResolveHostname` reply.
#[derive(Debug, Deserialize)]
struct ResolveHostnameReply {
    addresses: Vec<ResolvedAddress>,
}

#[derive(Debug, Deserialize)]
struct ResolvedAddress {
    family: i32,
    address: Vec<u8>,
}

// The errors of the interface (again, abbreviated).
#[derive(Debug, zlink::ReplyError)]
#[zlink(interface = "io.systemd.Resolve")]
enum ResolveError {
    NoNameServers,
    QueryTimedOut,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    zlink::zruntime::block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = zlink::zruntime::unix::connect("/run/systemd/resolve/io.systemd.Resolve").await?;

    let call = Call::new(ResolveMethods::ResolveHostname { name: "systemd.io" });
    let (reply, _fds): (reply::Result<ResolveHostnameReply, ResolveError>, _) =
        conn.call_method(&call, vec![]).await?;

    match reply {
        Ok(reply) => println!("resolved: {:?}", reply.parameters()),
        Err(e) => eprintln!("failed to resolve: {e:?}"),
    }

    Ok(())
}
```

A few things to note here:

* `call_method` (and the underlying `receive_reply`) is generic over the reply *and* the error
  type. The result is a nested `Result`: the outer `zlink::Result` covers connection-level problems
  (I/O errors, protocol violations, etc. — anything that isn't an answer from the service) while
  the inner `reply::Result<Params, Error>` distinguishes a successful reply from a method error.
* The second argument to `send_call`/`call_method` is a `Vec<OwnedFd>` of file descriptors to pass
  along with the call — an empty vector in the common case. Similarly, received messages come back
  paired with any file descriptors that accompanied them; hence the `(reply, _fds)` tuple.
* The reply types can *borrow* from the connection's internal read buffer (note the lifetime
  relationship in `receive_reply<'r, ...>`) — deserialization can be zero-copy. Using `&str` fields
  instead of `String` in your reply types avoids allocations entirely.

`Call` also exposes the [protocol flags](concepts.html#method-calls) as builder-style setters:

```rust,noplayground
# use zlink::Call;
# #[derive(Debug, serde::Serialize)]
# struct Ping;
// Fire-and-forget:
let call = Call::new(Ping).set_oneway(true);
// Ask for a stream of replies:
let call = Call::new(Ping).set_more(true);
```

For `more` calls, you keep invoking `receive_reply` until the received `Reply`'s `continues()`
method no longer returns `Some(true)`.

## The two halves

A `Connection` is really a pair of independent halves: a `ReadConnection` and a `WriteConnection`,
each owning its own buffer. You can split a connection into its halves and use them concurrently —
for instance, one task feeding calls while another consumes streaming replies:

```rust,noplayground
# fn example(conn: zlink::zruntime::unix::Connection) {
let (mut read, mut write) = conn.split();
// `read.receive_reply(...)` and `write.send_call(...)` can now be driven
// from separate tasks. `Connection::join(read, write)` puts them back together.
# }
```

Both halves share the connection's unique `id()`, so they can be re-associated later. They can also
be turned into the halves of the underlying socket with `ReadConnection::into_read_half` and
`WriteConnection::into_write_half`, to switch to another protocol after an
[upgrade](service.html#upgrading-connections). The read half that the former returns first hands out
the data that was read past the last message received.

## Server-side connections

On the service side, you get `Connection`s by `accept`ing them from a listener — but you will
rarely do even that yourself, since `zlink::Server` handles the accept loop for you. We'll cover
that in the [service chapter](service.html).

Now that we know how connections work, let's climb up the abstraction ladder — next stop:
convenient, type-safe client proxies.

[`serde_prefix_all`]: https://crates.io/crates/serde_prefix_all
[zruntime]: https://github.com/z-galaxy/zruntime
[`zlink::zruntime::block_on`]: https://docs.rs/zlink/latest/zlink/zruntime/fn.block_on.html
