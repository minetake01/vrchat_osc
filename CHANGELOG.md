# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `Service` describes a discovered service by its instance name and address.
- `ServiceType` implements `Debug`, `Clone`, `PartialEq` and `Eq`.
- `OscRootNode` implements `Default`.
- `hickory_proto` is re-exported, so the types carried by `Error::HickoryError`
  and `MdnsError::MdnsProtoError` can be named without pinning a version.
- Regression tests for the mDNS announcement wire format and the OSCQuery JSON
  field names.
- `rust-version = "1.88"`.

### Changed

- `VRChatOsc::new` returns `VRChatOsc` instead of `Arc<VRChatOsc>`. Wrap the
  handle in an `Arc` when the discovery callback needs to call back into it.
- `ServiceType` carries a `Service` instead of a string holding the full mDNS
  name. Patterns passed to `send` and `get_parameter` are matched against the
  instance name.
- `send` returns the number of services the packet was addressed to.
- `get_parameter` returns one entry per matching service with the fetch result
  inside, and no longer wraps the collection in a `Result`.
- `OSCTransport` is now `OscTransport`, and its variants `Udp` and `Tcp`. The
  serialized names are unchanged.
- `OscNode::r#type` is now `OscNode::osc_type`. The serialized name is unchanged.
- `Error`, `ServiceType` and the module error enums are `#[non_exhaustive]`.
- `hickory-proto` is updated to 0.26.
- tokio's `full` feature is replaced by `net`, `rt`, `sync` and `time`.

### Fixed

- `register` rolls back the OSC mDNS record and stops the OSC listener when the
  OSCQuery announcement fails.
- `find_non_loopback_ipv4` documentation no longer claims that link state is
  checked.
