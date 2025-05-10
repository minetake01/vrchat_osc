# VRChat OSC/OSCQuery for Rust

**`vrchat-osc` is a Rust crate designed to easily utilize VRChat's OSC (Open Sound Control) and OSCQuery protocols.**

This crate aims to help VRChat tool developers efficiently perform operations such as manipulating avatar parameters, retrieving information, and providing custom OSC services. It integrates OSC message sending/receiving with service discovery via mDNS for OSCQuery.

## Key Features

* **OSC Communication**: Easily send and receive OSC messages with the VRChat client.
* **OSCQuery Service Discovery**:
    * Automatically discovers VRChat clients and other OSCQuery-compatible services on the local network using mDNS.
    * Uses all available network interfaces without special configuration.
* **OSC Service Registration**: Publish your own OSC services and respond to queries from VRChat or other tools.
* **VRChat Parameter Manipulation**: Provides utilities to simplify access to OSC paths published by VRChat, such as avatar parameters (future enhancement).
* **Asynchronous Support**: Achieves efficient I/O operations based on the Tokio runtime for asynchronous processing.

## Setup

To use this crate, add the following dependencies to your `Cargo.toml`:

```toml
[dependencies]
vrchat-osc = "1" # Replace with the latest version as appropriate
tokio = { version = "1", features = ["full"] }
log = "0.4"
env_logger = "0.11" # For examples, necessary for logging
rosc = "0.10" # For constructing OSC messages
````

## Basic Usage

### Key Methods of `VRChatOSC`

  * `VRChatOSC::new().await?`:
    Creates a new `VRChatOSC` instance and starts mDNS service discovery.
  * `vrchat_osc.on_connect(|name, addr| { ... }).await`:
    Registers a callback that is invoked when a VRChat client (OSCQuery server) is discovered and connection information is obtained.
  * `vrchat_osc.register("service_name", port, root_node, handler_fn).await?`:
    Publishes your custom OSC service to the network.
      * `service_name`: The service name for your tool (e.g., "MySuperTool"). This will be announced via mDNS like `MySuperTool._oscjson._tcp.local.`.
      * `port`: The UDP port your tool will listen on for OSC messages.
      * `root_node`: An `OscRootNode` defining the OSC path hierarchy your service provides.
      * `handler_fn`: A callback function invoked when an OSC message arrives for your service.
  * `vrchat_osc.send(&packet, "VRChat-Client-*").await?`:
    Sends an OSC packet to OSC servers (e.g., VRChat clients) whose service names match the specified pattern.
    Wildcards like `"VRChat-Client-*"` can be used.
  * `vrchat_osc.get_parameter("/", "VRChat-Client-*").await?`:
    Retrieves information for the specified OSC path from the target service via OSCQuery.
    This can be used to get VRChat avatar parameters, etc.

## Error Handling

Many asynchronous functions in this crate return `Result<T, vrchat_osc::Error>`. Ensure proper error handling to maintain application stability. The `Error` type includes I/O errors, mDNS protocol errors, OSC parsing errors, and more.

## License

This project is licensed under the terms of either the MIT license or the Apache License 2.0.

## Contribution

Contributions are welcome\! Please follow these steps:

1.  Fork the repository.
2.  Create a new branch for your changes.
3.  Submit a pull request with a clear description of your modifications.

By contributing, you agree that your code will be licensed under the MIT license OR Apache License 2.0.

## Acknowledgements

The data models defined in `vrchat_osc::models::osc_query::*` are implemented with reference to [tychedelia/osc-query](https://www.google.com/search?q=https://github.com/tychedelia/osc-query).
