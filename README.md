# VRChat OSC/OSCQuery for Rust

[![Crates.io](https://img.shields.io/crates/v/vrchat_osc)](https://crates.io/crates/vrchat_osc)
[![Documentation](https://docs.rs/vrchat_osc/badge.svg)](https://docs.rs/vrchat_osc)

**`vrchat_osc` is a Rust crate designed to easily utilize VRChat's OSC (Open Sound Control) and OSCQuery protocols.**

This crate aims to help VRChat tool developers efficiently perform operations such as manipulating avatar parameters, retrieving information, and providing custom OSC services. It integrates OSC message sending/receiving with service discovery via mDNS for OSCQuery.

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
vrchat_osc = "1.1"
tokio = { version = "1", features = ["full"] }
rosc = "0.10" # For constructing OSC messages
````

## Usage Example
A basic usage example of this crate can be found in [examples/full.rs](./examples/full.rs).

## License

This project is licensed under the terms of either the MIT license or the Apache License 2.0.

## Contribution

Contributions are welcome\! Please follow these steps:

1.  Fork the repository.
2.  Create a new branch for your changes.
3.  Submit a pull request with a clear description of your modifications.

By contributing, you agree that your code will be licensed under the MIT license OR Apache License 2.0.

## Acknowledgements

The data models defined in `vrchat_osc::models::*` are implemented with reference to [tychedelia/osc-query](https://github.com/tychedelia/osc-query).
