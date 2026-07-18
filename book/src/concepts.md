# Some Varlink concepts to help newcomers

<!-- toc -->

If you're coming from the D-Bus world (perhaps even from [zbus]), many Varlink concepts will feel
familiar — interfaces, methods, errors, introspection. But the differences are just as important as
the similarities, so this chapter points them out along the way.

## No bus — point-to-point connections

The most fundamental difference from D-Bus: **there is no bus**. A Varlink service listens on a
socket (typically a Unix domain socket) and clients connect *directly* to it. There is no broker in
the middle relaying messages, no handshake or authentication dance, no name registration and no
service discovery through a central authority. A connection is just... a socket connection.

This has a couple of far-reaching consequences:

* **Connections are cheap.** Establishing a Varlink connection is essentially the cost of a
  `connect(2)` call. There is no equivalent of the D-Bus authentication handshake and no
  bus-enforced limit on the number of connections a process may have (unless a specific service
  imposes one). Where a D-Bus process typically opens *one* connection to the bus and shares it
  between all its threads and tasks, a Varlink client is *encouraged* to open as many connections
  as it has independent conversations to conduct — for example, one per task.
* **Messages arrive in order and are never multiplexed.** A service processes the calls it receives
  on a connection strictly in the order they were received, and replies in that same order. This is
  guaranteed by the [Varlink specification][spec] and it's what makes [pipelining](pipelining.html)
  both possible and trivial. (The D-Bus specification makes no such sequential-processing
  guarantee.)

These two properties allow zlink to be dramatically simpler — and faster — on the inside than a
D-Bus implementation can be. The [Design for efficiency](design.html) chapter explains how.

## Services and addresses

Since there is no bus, there are no bus names either. A service is identified by the address of the
socket it listens on. Varlink expresses addresses in URI notation, e.g.:

* `unix:/run/org.example.ftl` — a Unix domain socket,
* `tcp:127.0.0.1:12345` — a TCP socket.

By convention, system services put their sockets in `/run` (e.g. systemd-resolved listens on
`/run/systemd/resolve/io.systemd.Resolve`), while user-level services would typically use
`$XDG_RUNTIME_DIR`. Socket files are usually named after the primary interface the service
provides, which makes them discoverable with a simple `ls`.

## Interfaces

An interface defines the API a service exposes, exactly like a D-Bus interface does. Interfaces are
identified by [reverse-domain names][idl-spec] (e.g. `io.systemd.Resolve` or
`org.example.Calculator`) and described in Varlink's *interface definition language* (IDL). Here's
a small example:

```text
interface org.example.Calculator

type CalculationResult (
    result: float
)

method Add(a: float, b: float) -> (result: float)
method Divide(dividend: float, divisor: float) -> (result: float)

error DivisionByZero(message: string)
```

A few things worth noting:

* **Methods** take named parameters and return named parameters. Method names are in PascalCase.
* **Custom types** (`type`) are structures with named, typed fields. The primitive types are
  `bool`, `int`, `float`, `string`, `object` (a foreign, untyped object) and `any`. Compound types
  include arrays (`[]string`), maps (`[string]int`), enums (`(red, green, blue)`) and anonymous
  structs.
* **Nullable types** are spelled with a `?` prefix: `?string`. Yes — unlike D-Bus, Varlink has
  nullable types, so Rust's `Option<T>` maps directly to the wire format. 🎉
* **Errors** (`error`) are first-class citizens of the interface definition, with typed parameters
  of their own.

Unlike D-Bus, where the interface description (in XML) is mostly a machine-level detail, the
Varlink IDL is designed to be written and read by humans. It is also available from the service
itself at runtime, through [introspection](introspection.html).

## Method calls

All communication happens through method calls. A call is a JSON object with the fully-qualified
method name and (optionally) its parameters:

```json
{
  "method": "org.example.Calculator.Add",
  "parameters": { "a": 4.0, "b": 2.0 }
}
```

The reply is a JSON object as well:

```json
{
  "parameters": { "result": 6.0 }
}
```

...unless the call failed, in which case the reply carries the fully-qualified error name instead:

```json
{
  "error": "org.example.Calculator.DivisionByZero",
  "parameters": { "message": "Cannot divide by zero" }
}
```

Each message is terminated by a single `NUL` byte. That's the whole wire format! One pleasant
side-effect of the human-readable encoding: your IPC traffic doubles as a log. Dump the socket
traffic and you can read exactly what your service is being asked and what it answered — no special
decoding tools required.

A call can also carry a few flags that modify its behavior:

* **`oneway`**: The client doesn't want a reply. Fire and forget.
* **`more`**: The client expects *multiple* replies to this one call — a stream. Each intermediate
  reply carries `"continues": true`; the final one doesn't. This is Varlink's replacement for
  D-Bus signals: instead of broadcasting to whoever might listen, a client explicitly subscribes by
  issuing a `more` call on a connection dedicated to that stream, and the service streams replies
  for as long as needed.
* **`upgrade`**: The client wants to take over the connection with a custom protocol after this
  call. (Rarely used and out of scope for this book.)

## Errors

As shown above, an error reply names the error in fully-qualified, reverse-domain form and can
carry parameters, just like a method reply. Errors declared in the interface IDL are part of the
API contract. In zlink, errors map naturally to Rust enums — you'll see this in action in the
[client](client.html) and [service](service.html) chapters.

## Introspection

Every well-behaved Varlink service implements the standard `org.varlink.service` interface, which
offers two methods:

* `GetInfo` — returns metadata about the service (vendor, product, version, URL) and the list of
  interfaces it implements.
* `GetInterfaceDescription` — returns the IDL text of a given interface.

That's considerably simpler than D-Bus introspection, and just as with zbus, zlink's `service`
macro [implements it for you automatically](service.html#introspection-for-free), while both a
low-level and a high-level client API is available for [querying it](introspection.html).

## Exploring services with `varlinkctl`

Just like `busctl` for D-Bus, systemd ships a handy CLI tool for poking at Varlink services:
[`varlinkctl`] (available since systemd v255). A few examples to try on a recent Linux system:

```bash
# What does systemd-resolved offer?
varlinkctl info /run/systemd/resolve/io.systemd.Resolve

# Show the IDL of the io.systemd.Resolve interface
varlinkctl introspect /run/systemd/resolve/io.systemd.Resolve io.systemd.Resolve

# Call a method
varlinkctl call /run/systemd/resolve/io.systemd.Resolve \
  io.systemd.Resolve.ResolveHostname '{"name": "systemd.io", "family": 2}'
```

We'll use `varlinkctl` throughout this book to interact with the services we build.

## Good practices & API design

Just as with D-Bus, it is recommended to organize interface names by using fully-qualified,
reverse domain names you control, in order to avoid conflicts. Design your methods around named
parameters and declare all the errors they can produce in the IDL — your future users (and your
future self) will thank you.

Onwards to implementation details & examples!

[zbus]: https://dbus2.github.io/zbus/
[spec]: https://varlink.org/Method-Call
[idl-spec]: https://varlink.org/Interface-Definition
[`varlinkctl`]: https://www.freedesktop.org/software/systemd/man/latest/varlinkctl.html
