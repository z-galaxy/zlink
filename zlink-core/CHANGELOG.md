# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.7.0 - 2026-07-19

### Breaking
- 💥 Give IDL parsing its own error type.

### Changed
- 🏗️ Make the runtime features additive.
- 🏗️ Split the IDL types and parser into their own crate. #78
- 🎨 Collapse nested ifs into let-chains.

### Documentation
- 📝 Mention the book in the README.
- 📝 Link to the examples directory instead of duplicating it.
- 📝 Remove obsolete USB transport note and document smol crate.
- 📝 Fix 0.6.0 changelogs: drop entries mis-attributed by shallow clone.

### Testing
- ✅ Test the book examples as doctests.

## 0.6.0 - 2026-07-11

### Added
- ✨ Auto-enable SO_PASSCRED on unix sockets.
- ✨ Send credentials over unix sockets.
- ✨ Implement credentials receiving from unix sockets. #267
- ✨ API for receiving credentials.
- ✨ Add PassedCredentials.
- ✨ ReadyListener signals exhaustion after handing out its socket.
- ✨ Support custom types per interface in introspection.

### Breaking
- 💥 Socket::read now returns a named struct.
- 💥 Let Listener::accept signal exhaustion.

### Changed
- 🎨 A micro refactor.

### Documentation
- 📝 Drop redundant internal docs.

### Fixed
- 🐛 Exit Server::run when a listener signals exhaustion.

### Other
- 🚨 Fix clippy warnings from latest stable Rust.
- 🦺 add safety check to getsockopt call.

### Security
- 🔒️ remove pidfd_open fallback.

## 0.5.0 - 2026-05-03

### Added
- ✨ Add accessor to `CustomObject` fields slice.
- ✨ make `CustomType::as_object` const.
- ✨ Allow fallible reply streams in `Service`.
- ✨ Add `service::Infallible` wrapper.

### Fixed
- 🐛 Make sure methods return objects in tests.

## 0.4.2 - 2026-04-26

### Changed
- ♻️ Replace remaining manual byte loops in IDL parser.
- ♻️ Rewrite parameter list and type body with combinators. #256
- ♻️ Rewrite inline struct/enum parsers with combinators. #254
- ♻️ Dispatch inline types with `alt` instead of byte lookahead. #253

### Documentation
- 📝 Configure docs.rs to build for all supported targets.

### Fixed
- 🐛 fix missing comma in enum with comments.
- 🐛 Workaround FD-passing on the same connection on Mac.

## 0.4.1 - 2026-03-28

### Added
- ✨ support (supplementary) groups in peer credentials. #238
- 🚸 Add `ReadyListener`.

### Dependencies
- ⬆️  Upgrade to Rust edition 2024.

### Fixed
- 🐛 Don't hide `notified` mod from docs. #241
- 🚑️ Don't assume futures_util dep. #230

## 0.4.0 - 2026-02-22

### Added
- ✨ add support for ANY type.
- ✨ Support FDs in Service impls.
- ✨ Add Reply::map method.
- ✨ Add notified::State::stream_once.
- ✨ Add `service` attribute macro. #76

### Breaking
- 💥 Move `Sock` from `handle` to the trait in Service. #207

### Changed
- 🎨 Re-arrange attributes to satisfy rust-analyzer.
- 🏗️ Re-export pin-project-lite but keep it hidden.
- 🏗️ Provide traits for `notified` API.

### Other
- 🚩 Remove std requirement from introspection.
- 🚩 Feature-gate std-only introspect Type impls.
- 🚩 `service` feature now requires `introspection` feature.

### Removed
- 🔥 Drop notified::Once.

## 0.3.0 - 2026-01-12

### Added
- ✨ Allow creating chains from Iterator types. #168
- ✨ Internal MockSocket to handle pipelined messages.

### Breaking
- 💥 chain::ReplyStream's items now owned. #185

### Changed
- 🎨 varlink_service Owned types wrappers around borrowed siblings.
- 🏗️ Move reply types from Chain to send method.
- ♻️ Rename service handle lifetime to 'service.

### Documentation
- 📝 Don't hide ReplyStream from docs anymore.
- 📝 List forgotten `smol` feature.
- 📝 Drop now incorrect `no_std` claims.

### Fixed
- 🩹 Add some missing `cfg`s.
- 🐛 Add missing feature gate on a Deserialize impl.
- 🐛 Don't send reply for `oneway` methods.

### Other
- ✏️ fix docstring typos.
- 🦺 Box the Future type of the ReplyStream.
- ✏️ Fix a typo.
- 🚸 Add Chain::send_ignore_replies.
- ✏️  Add missing `.` at the end of sentences.

### Performance
- ⚡️ Reduce allocations.
- ⚡️ Allow service to return borrowed data from call.

### Removed
- 🔥 `proxy` only generate chain methods for owned types ret.
- 🔥 Remove a redundant referencing operation.

### Testing
- ✅ Rename 2 tests.
