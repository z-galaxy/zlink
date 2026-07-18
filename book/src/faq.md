# FAQ

<!-- toc -->

## Should I use zlink or zbus?

Both are maintained by the same project, so this is an honest referee's answer: it depends on which
protocol you need to speak, and that's usually decided for you.

* Talking to an existing **D-Bus** service (desktop APIs, NetworkManager, BlueZ, ...) → [zbus].
* Talking to an existing **Varlink** service (a growing list of systemd components, ...) → zlink.
* Designing a **new** system service or a point-to-point IPC link and free to choose? Varlink and
  zlink make a compelling default: no broker dependency, a simpler protocol, and the
  [efficiency benefits](design.html) that come with it. If you need what a bus genuinely provides
  — broadcast signals to many unrelated listeners, discovery/enumeration of running services,
  bus-enforced policy — D-Bus still earns its keep.

## Where are the signals and properties?

Varlink doesn't have them as separate concepts — both patterns are served by
[streaming (`more`) calls](client.html#streaming-calls). A property is just a method returning the
value; a "property changed" signal or any other notification is a `more` call streaming updates to
each *interested* client on its own connection. There is no broadcast: nothing is emitted for
nobody, and subscribers state their interest explicitly. `notified::State` gives services the
[monitor-a-value pattern](service.html#streaming-methods) out of the box.

## Why do proxy methods take `&mut self`? How do I make concurrent calls?

Because a `Connection` is exclusively owned — mutation via `&mut` instead of internal locks is a
core design decision ([exterior mutability](design.html#exterior-mutability)). A single connection
is a strictly ordered request/reply pipeline, so concurrent calls over one connection wouldn't buy
what you might hope anyway.

To make concurrent calls, do the Varlink-idiomatic thing: **open one connection per concurrent
conversation**. Connections are [cheap](concepts.html#no-bus--point-to-point-connections) — this
is not the D-Bus world where connections are a scarce resource. For many calls to the *same*
service, consider [pipelining](pipelining.html) them over one connection instead: often faster
than true concurrency, with none of the coordination.

## Why can't I `tokio::spawn(server.run())`?

A [rustc bug][rustc-100013] prevents the future returned by `Server::run` from being `Send`. Until
that's fixed, drive the server with `select!` alongside your other work, or give it a
[`LocalSet`]/[`LocalRuntime`] (or a plain dedicated thread). See
[the service chapter](service.html#-serverrun-and-tokiospawn).

## How do I use `Option<T>`?

Just use it. Varlink has [nullable types](concepts.html#interfaces) (`?type` in the IDL), and
serde's `Option<T>` handling maps straight onto them — no `option-as-array` workarounds or
sentinel values as in the D-Bus world.

## Can I pass file descriptors?

Yes — over Unix domain sockets, methods can send and receive `OwnedFd`s alongside the JSON payload
(see [the client chapter](client.html#other-method-attributes) and the `fds` parameters throughout
the low-level API). Note that the Varlink IDL has no file-descriptor type, so the convention is for
JSON parameters to carry *indexes* into the accompanying descriptor array. This zlink extension
interoperates with systemd's use of the same convention; it requires `std`.

## Why doesn't my proxy method get a `chain_` variant?

Chain methods are only generated for methods whose reply and error types are owned
(`DeserializeOwned`). Borrowed reply types can't be used in chains because the read buffer is
reused from one reply to the next — see [the pipelining chapter](pipelining.html#chaining-proxy-methods).
Input parameters may borrow either way.

## How do I try this against real services?

Recent systemd versions ship a growing set of Varlink services. Look around in `/run` for sockets
named like interfaces (e.g. `/run/systemd/resolve/io.systemd.Resolve`,
`/run/systemd/io.systemd.Hostname`), point [`varlinkctl`](concepts.html#exploring-services-with-varlinkctl)
at them, and then do the same from Rust with a [proxy](client.html). The
[`resolved`][resolved-example] and [`varlink-inspect`][inspect-example] examples in the repository
are ready-made starting points.

## Is there a blocking (non-async) API?

No. zlink is async-first by design — IPC is I/O-bound, and the APIs (streaming replies, pipelining,
serving many connections from one task) lean heavily on async. If you're in a synchronous program,
you can wrap calls with your runtime's `block_on` (e.g. `tokio`'s current-thread runtime or
`smol::block_on`).

[zbus]: https://github.com/z-galaxy/zbus
[rustc-100013]: https://github.com/rust-lang/rust/issues/100013
[`LocalSet`]: https://docs.rs/tokio/latest/tokio/task/struct.LocalSet.html
[`LocalRuntime`]: https://docs.rs/tokio/latest/tokio/runtime/struct.LocalRuntime.html
[resolved-example]: https://github.com/z-galaxy/zlink/blob/main/zlink/examples/resolved.rs
[inspect-example]: https://github.com/z-galaxy/zlink/blob/main/zlink/examples/varlink-inspect.rs
