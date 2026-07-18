# no_std & embedded usage

<!-- toc -->

One of zlink's founding goals is to bring Varlink to places no D-Bus implementation can go:
microcontrollers. Picture a device where firmware on a microcontroller talks to a Linux "brain"
board — traditionally that means a gateway process translating between some ad-hoc serial protocol
and the system IPC. If the microcontroller could speak the IPC protocol *itself*, a layer of
middlemen disappears. Varlink's minimal wire format (JSON + `NUL` framing over any byte stream)
makes that realistic, and zlink's core is built to deliver it: `zlink-core` is `no_std`-compatible,
avoids allocations wherever possible, and — [true to form](design.html) — uses no atomics and no
interior mutability, which also makes it well-behaved on single-core embedded targets.

## What works without `std`

The `no_std` surface is the *client* side and the plumbing underneath it:

* `Connection`, `Call`, `Reply` and the whole send/receive machinery,
* the [`proxy`](client.html) macro — type-safe clients work fully in `no_std`,
* [chaining/pipelining](pipelining.html),
* IDL types and parsing (`idl`, `idl-parse` features),
* logging via [`defmt`](https://defmt.ferrous-systems.com/) instead of `tracing`.

The server side (`Server`, `Listener`, the `service` macro) and file-descriptor passing currently
require `std`.

To use zlink in `no_std`, depend on `zlink-core` directly (the `zlink` crate's runtime integrations
are `std`-only) and disable default features:

```toml
[dependencies]
zlink-core = { version = "0.7", default-features = false, features = ["proxy", "idl-parse", "defmt"] }
```

Note that either `tracing` or `defmt` must be enabled — for `no_std`, that means `defmt`. This
configuration is exercised by zlink's CI, so it is guaranteed to keep building:

```bash
cargo check -p zlink-core --no-default-features --features idl-parse,proxy,defmt
```

Note that `no_std` does not (currently) mean `no_alloc`: zlink-core relies on the `alloc` crate
(e.g. for its message buffers), so your target needs a global allocator. Allocations are
nonetheless kept to a minimum — reply deserialization borrows from the connection's read buffer
rather than copying out of it.

## Bringing your own transport

On a Linux system, the transport is a Unix socket provided by `zlink-tokio`/`zlink-smol`. On an
embedded target, *you* provide the transport, by implementing three small traits from
`zlink_core::connection::socket`:

```rust,noplayground
use zlink_core::connection::socket::{Socket, ReadHalf, WriteHalf, ReadResult};

#[derive(Debug)]
struct UartSocket { /* ... */ }
#[derive(Debug)]
struct UartRead { /* ... */ }
#[derive(Debug)]
struct UartWrite { /* ... */ }

impl Socket for UartSocket {
    type ReadHalf = UartRead;
    type WriteHalf = UartWrite;

    fn split(self) -> (UartRead, UartWrite) {
        // hand out the two directions of your transport
        # todo!()
    }
}

impl ReadHalf for UartRead {
    async fn read(&mut self, buf: &mut [u8]) -> zlink_core::Result<ReadResult> {
        // read some bytes into `buf`
        # todo!()
    }
}

impl WriteHalf for UartWrite {
    async fn write(&mut self, buf: &[u8]) -> zlink_core::Result<()> {
        // write all of `buf`
        # todo!()
    }
}
```

That's the entire integration contract. `Connection::new(UartSocket { .. })` then gives you
everything this book has covered on the client side — proxies, chains, zero-copy replies — over
your UART (or SPI, or USB CDC, or RTT...). Varlink deliberately doesn't prescribe the byte stream
underneath; anything that can carry bytes in order will do.

Two requirements to keep in mind for the async plumbing: the futures returned by `read`/`write`
must be cancel-safe, and they should behave as `Unpin` would require — zlink avoids boxing them
(that would mean allocation), so they get polled in place.

For tests — embedded or not — `zlink-core` also ships a mock transport,
`zlink_core::test_utils::mock_socket::MockSocket`, that replays pre-configured replies; the
codegen integration tests use it to exercise generated proxies without any real socket.

## An honest word on scope

Should you build your safety-critical, hard-real-time control loop on JSON-over-serial? No — a
protocol with variable-length human-readable framing is not where you go for guaranteed latency,
and zlink won't pretend otherwise. The sweet spot for embedded Varlink is the *management plane*:
configuration, status queries, monitoring streams — the same request/reply and subscribe patterns
this book has covered throughout, now reaching all the way down to the firmware. And thanks to
zero-copy deserialization and the no-atomics, allocation-averse discipline in the core, the
footprint stays appropriately tiny.
