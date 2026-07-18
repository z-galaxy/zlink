# Design for efficiency

<!-- toc -->

zlink is not "zbus with JSON instead of a binary format". The Varlink protocol's point-to-point
architecture allows for a fundamentally different — and leaner — design than what is possible for
any D-Bus implementation. This chapter explains that design and the reasoning behind it. If you've
ever wondered why zlink's API insists on `&mut self` everywhere, or why this book keeps repeating
"just open another connection", here is the payoff.

> Much of this chapter is based on the talk [*Forget zbus, zlink is the future of IPC in Rust*]
> given by zlink's author at All Systems Go! 2025 ([recording also on media.ccc.de][ccc]).

## The problem with a shared connection

In the D-Bus world, a process typically opens **one** connection to the bus and shares it between
all its threads and tasks. This isn't just habit: connections are relatively expensive to establish
(authentication handshake, hello dance) and the bus enforces per-user connection limits, so a
library can't just open connections liberally.

Sharing a connection has deep consequences for a library's architecture. Consider how zbus must
receive messages: a dedicated *socket reader* task reads each message off the shared socket and
broadcasts it to whichever tasks are interested. To avoid deep-copying every message to every
consumer, messages get wrapped in `Arc`s. And since many tasks hold references to the same
connection state, that state needs *interior mutability* — mutexes and atomics. Add it all up and
every incoming message pays for:

* at least two copies (socket → read buffer → parsed message), regardless of how many consumers
  there are;
* atomic reference counting on every message (cheap, but not free — especially off the big-core
  x86 happy path);
* lock acquisition on shared connection state, with the contention — and in the worst case,
  deadlock potential — that mutexes always bring;
* an extra task-scheduling hop: the socket reader runs, *then* the consuming task gets woken up.

None of this is a zbus design flaw. It's the price of the one-shared-connection model that D-Bus
imposes, and every D-Bus library in every language pays it in some currency or other.

## Varlink changes the economics

Varlink connections are **cheap**: connecting is a bare `connect(2)` — no handshake, no
registration — and with no broker in the middle, there's no bus-imposed connection limit either.
The protocol, in fact, *expects* clients to open multiple connections to the same service — that is
e.g. how you have concurrent method calls in flight, given that each connection is a strictly
ordered request/reply pipeline.

Cheap connections dissolve the entire problem from the previous section: if every task can afford
its own connection, then nothing about a connection ever needs to be shared.

## Exterior mutability

zlink leans into this completely. A `Connection` is exclusively owned, and every operation on it
takes `&mut self`. We call this design **exterior mutability** — mutation happens through plain
exclusive references, with Rust's borrow checker proving at *compile time* that no two contexts
ever touch the same connection state concurrently. Compare:

| | shared connection (zbus model) | connection per task (zlink model) |
|---|---|---|
| socket reading | central socket-reader task | each task reads its own socket |
| message copies | two + `Arc` per consumer | one, into the connection's buffer |
| string/data access | owned/`Arc`'d data | zero-copy borrows from the buffer |
| shared state | mutexes + atomics | none — `&mut` proves exclusivity |
| deadlocks | possible | impossible (nothing to lock) |
| wake-ups per message | reader task + consumer task | consumer only |

Because replies deserialize straight out of the connection's read buffer, reply types can borrow
(`&str` fields, `Cow<'_, str>`, lifetime-parameterized structs) and the common
read-a-reply-and-look-at-it path performs **no allocation and no copy** beyond the single
socket-to-buffer read. No atomics anywhere. No mutexes anywhere. The type system does at compile
time what locks would otherwise do at runtime.

This is also why the [`proxy` macro](client.html) implements the generated trait directly on
`Connection` rather than on some proxy object wrapping a shared handle: the connection *is* the
client, and holding `&mut` to it is all the synchronization there will ever be.

And when you *do* want concurrency? Open another connection — one per logical conversation, e.g.
one for your streaming subscription and one for request/response calls. Each is independent, each
reads its own socket, and none of them needs to coordinate with any other.

## A server without locks

The same philosophy runs through the service side. `Server::run` drives *all* connections — new
accepts, incoming calls, and outgoing reply streams — from a single task using a fair,
round-robin-ish select loop. At most one method handler executes at any moment, so the handler can
take `&mut self` on your service and mutate plain fields directly. Your service state needs no
`Arc<Mutex<...>>`, and the dispatch loop itself acquires no locks — connections just sit in
vectors, polled fairly.

A single task sounds like a bottleneck; for the small-message control-plane traffic that IPC
overwhelmingly consists of, it isn't — the per-message work is tiny, and eliminating locks,
atomics, copies, and cross-task hand-offs buys back much more than parallel handlers would.
Compute-heavy methods can always spawn work off and [stream results
back](service.html#streaming-methods).

## Pipelining, guaranteed

Varlink's strict ordering guarantee (calls are processed, and replies delivered, in order) makes
[pipelining](pipelining.html) safe by construction — batch N calls into one `write()`, get woken
once for the replies. The D-Bus specification never made this promise, so D-Bus clients are stuck
with call-and-wait round trips. When systemd's developers measured D-Bus performance, batching was
one of the biggest wins available — Varlink builds it into the protocol contract, and zlink
surfaces it as the chaining API at every level.

## "But JSON is slow!"

That was zlink's author's first objection too, and it deserves an honest answer.

Parsing JSON does cost more than parsing a well-designed binary format. But for IPC-sized messages
what dominates end-to-end cost is not parsing — it's **context switching**, which modern CPUs are
comparatively bad at (and mitigations for speculative-execution vulnerabilities made worse).
Benchmarks comparing JSON against D-Bus's binary format ([data here][benchmarks]) show that for
typical D-Bus-style payloads — dictionaries with named keys, exactly what most real-world IPC
messages look like — JSON's speed and size are in the same ballpark; the binary format only pulls
meaningfully ahead for large arrays of raw values. Meanwhile the JSON choice buys:

* **[serde_json]** — one of the most mature, optimized and battle-tested serialization libraries
  in the Rust ecosystem, for free;
* **your traffic is your log** — captured Varlink traffic is human-readable as-is, no decoder
  needed;
* **nullable types** — `Option<T>` maps directly onto Varlink's `?type`, something D-Bus never
  had.

And where message framing costs *could* bite — locating the `NUL` terminators in a buffer full of
pipelined messages — zlink lets the deserializer report where each message ends instead of
re-scanning the buffer, and drains every complete message from a single `read()`.

## The scorecard

Putting it together, on a typical method call zlink's client path does: one buffered `write()`
(shared with any pipelined friends), one `read()` into a connection-owned buffer, one zero-copy
deserialization borrowing from that buffer — with no locks taken, no atomics touched, no
reference counts bumped, and no intermediate task woken. It is hard to see what's left to remove —
which is, in the end, the best summary of zlink's design philosophy: the fastest work is the work
you never do.

[*Forget zbus, zlink is the future of IPC in Rust*]: https://www.youtube.com/watch?v=CHgzWbvl9eE
[ccc]: https://media.ccc.de/v/all-systems-go-2025-340-forget-zbus-zlink-is-the-future-of-ipc-in-rust
[benchmarks]: https://github.com/zeenix/json-vs-bin
[serde_json]: https://crates.io/crates/serde_json
