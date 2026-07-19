# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.7.0 - 2026-07-19

### Changed
- 🏗️ Make the runtime features additive.

### Documentation
- 📝 Mention the book in the README.
- 📝 Link to the examples directory instead of duplicating it.
- 📝 Remove obsolete USB transport note and document smol crate.
- 📝 Fix 0.6.0 changelogs: drop entries mis-attributed by shallow clone.

### Removed
- 🔥 Don't re-export the zlink-core API.

## 0.6.0 - 2026-07-11

### Added
- ✨ Auto-enable SO_PASSCRED on unix sockets.
- ✨ Send credentials over unix sockets.
- ✨ Construct unix::Stream from std::os::unix::net::UnixStream.

### Breaking
- 💥 TryFrom instead of From for unix Stream.
- 💥 Socket::read now returns a named struct.
- 💥 Let Listener::accept signal exhaustion.

## 0.5.0 - 2026-05-03

### Fixed
- 🐛 Make sure methods return objects in tests.

## 0.4.2 - 2026-04-26

### Documentation
- 📝 Configure docs.rs to build for all supported targets.

### Fixed
- 🐛 Workaround FD-passing on the same connection on Mac.

## 0.4.1 - 2026-03-28

### Dependencies
- ⬆️  Upgrade to Rust edition 2024.

### Fixed
- 🐛 `service` enables `introspection` feature. #232

## 0.4.0 - 2026-02-22

### Added
- ✨ Support FDs in Service impls.
- ✨ Add notified::State::stream_once.
- ✨ Split stream types for `notified`. #86
- ✨ notified::Stream now handles notified::State drop.
- ✨ Add `service` attribute macro. #76

### Breaking
- 💥 Move `Sock` from `handle` to the trait in Service. #207

### Changed
- 🏗️ Provide traits for `notified` API.
- ♻️ Split `notified` module into a hierarchy.
- 🏗️ Use pin-project-lite to consolidate notified.

### Dependencies
- ➕ Add direct dep on pin-project-lite.

### Documentation
- 📝 Document `service` feature.
- 📝 Use & recommend `service` macro.
- 💡 Add missing `.` in sample code comments.

### Other
- 🚩 Remove std requirement from introspection.
- 🚩 `notified` now gated behind `server` feature.
- 🚩 `service` feature now requires `introspection` feature.

### Removed
- 🔥 Drop notified::Once.

## 0.3.0 - 2026-01-12

### Added
- ✨ impl From<UnixListener> for unix::Listener.

### Breaking
- 💥 chain::ReplyStream's items now owned. #185

### Changed
- 🏗️ Move reply types from Chain to send method.
- ♻️ Rename service handle lifetime to 'service.

### Documentation
- 📝 List forgotten `smol` feature.
- 📝 Drop now incorrect `no_std` claims.

### Other
- ✏️ Fix a typo.
- ✏️  Add missing `.` at the end of sentences.

### Performance
- ⚡️ Allow service to return borrowed data from call.

### Removed
- 🔥 `proxy` only generate chain methods for owned types ret.
