# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## 0.7.0 - 2026-07-19

### Breaking
- 💥 Give IDL parsing its own error type.

### Changed
- 🏗️ Make the runtime features additive.

### Documentation
- 📝 Mention the book in the README.
- 📝 Link to the examples directory instead of duplicating it.
- 📝 Remove obsolete USB transport note and document smol crate.
- 📝 Fix 0.6.0 changelogs: drop entries mis-attributed by shallow clone.

### Testing
- ✅ Test the book examples as doctests.

## 0.6.0 - 2026-07-11

### Added
- ✨ Add --rustfmt to zlink-codegen.
- ✨ Introduce generate_files() function.
- ✨ Introduce an error.rs.

### Other
- 🚨 Fix clippy warnings from latest stable Rust.
- ♻ Refactor main.rs to use generate_files().

### Testing
- ✅ Update test-integration to use generate_files().

## 0.5.0 - 2026-05-03

### Fixed
- 🐛 Make sure methods return objects in tests.

## 0.4.2 - 2026-04-26

### Documentation
- 📝 Configure docs.rs to build for all supported targets.

## 0.4.1 - 2026-03-28

### Other
- Updated the following local packages: zlink.

## 0.4.0 - 2026-02-22

### Added
- ✨ add support for ANY type.
- ✨ Support FDs in Service impls.

### Breaking
- 💥 Move `Sock` from `handle` to the trait in Service. #207

## 0.3.0 - 2026-01-12

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
