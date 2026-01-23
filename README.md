# VRChat OSC/OSCQuery for Rust

[![Crates.io](https://img.shields.io/crates/v/vrchat_osc)](https://crates.io/crates/vrchat_osc)
[![Documentation](https://docs.rs/vrchat_osc/badge.svg)](https://docs.rs/vrchat_osc)

**`vrchat_osc` is a Rust crate designed to easily utilize VRChat's OSC (Open Sound Control) and OSCQuery protocols.**

This crate is specifically designed to handle VRChat's unique network implementation behaviors, such as non-standard mDNS responses and binding specificities.
For details on technical workarounds implemented in this crate, please refer to [WORKAROUNDS.md](./WORKAROUNDS.md).

## Supported Network Environments

Most features work out-of-the-box on a local machine (Localhost) or within a standard Local Area Network (LAN) supporting multicast.
However, due to VRChat's implementation choices (reliance on mDNS for discovery and loopback bindings), some features are limited in VPN or non-multicast environments.

| Feature | Method | Localhost (Same PC) | LAN (Local Network) | VPN / Non-Multicast |
| :--- | :--- | :---: | :---: | :---: |
| **Send OSC** | `send()` (Auto-Discovery) | ✅ | ✅ | ❌ |
| | `send_to_addr()` (Direct) | ✅ | ✅ | ✅ |
| **Receive OSC** | `register()` (Bind 0.0.0.0) | ✅ | ✅ | ❌ (*1) |
| **OSCQuery (Get)** | `get_parameter()` (Auto-Discovery) | ✅ | ❌ | ❌ |
| | `get_parameter_from_addr()` (Direct) | ✅ | ❌ | ❌ |
| **OSCQuery (Host)** | `register()` (Serve 0.0.0.0) | ✅ | ✅ | ✅ (*2) |

* (*1) VRChat relies on mDNS for discovery and cannot be manually configured to send to an arbitrary IP.
* (*2) Functionality works (server is reachable), but VRChat will not discover it via mDNS in this environment.

## Supported Platforms

This crate is cross-platform and supports the following operating systems:

| Platform | Support | Notes |
| :--- | :---: | :--- |
| **Windows** | ✅ | Native support. |
| **Linux** | ✅ | Native support. |
| **MacOS** | ✅ | VRChat does not run natively. **LAN** limitations apply when connecting to VRChat on another device. |

## Key Features

* **OSC Communication**: Easily send and receive OSC messages with the VRChat client.
* **OSCQuery Service Discovery**:
    * Automatically discovers VRChat clients and other OSCQuery-compatible services on the local network using mDNS.
* **OSC Service Registration**: Publish your own OSC services and respond to queries from VRChat or other tools.
* **OSCQuery Get Parameter**: Retrieve specific parameters from VRChat clients using OSCQuery.
* **Asynchronous Support**: Achieves efficient I/O operations based on the Tokio runtime for asynchronous processing.

## Setup

To use this crate, add the following dependencies to your `Cargo.toml`:

```toml
[dependencies]
vrchat_osc = "1"
tokio = { version = "1", features = ["full"] }
```

## Usage Example
A basic usage example of this crate can be found in [examples/full.rs](./examples/full.rs).

## License

This project is licensed under the terms of either the MIT license or the Apache License 2.0.

## Contribution

Contributions are welcome! Please follow these steps:

1.  Fork the repository.
2.  Create a new branch for your changes.
3.  Submit a pull request with a clear description of your modifications.

By contributing, you agree that your code will be licensed under the MIT license OR Apache License 2.0.

## Acknowledgements

The data models defined in `vrchat_osc::models::*` are implemented with reference to [tychedelia/osc-query](https://github.com/tychedelia/osc-query).
