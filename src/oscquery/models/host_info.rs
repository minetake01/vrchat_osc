use std::{collections::HashMap, net::IpAddr};

use serde::{Deserialize, Serialize};

/// Represents the `HOST_INFO` data for an OSCQuery server.
/// This struct contains information about the OSC server that OSCQuery is describing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct HostInfo {
    /// The human-readable name of the OSC server application.
    pub name: String,
    /// A map of OSCQuery extensions supported by the server and their status (true for supported).
    /// Common extensions include "ACCESS", "VALUE", "RANGE", "CLIPMODE", "TYPE", "DESCRIPTION".
    pub extensions: HashMap<String, bool>,
    /// The IP address where the OSC server is listening for messages.
    pub osc_ip: IpAddr,
    /// The port number where the OSC server is listening for messages.
    pub osc_port: u16,
    /// The transport protocol used by the OSC server (e.g., UDP or TCP).
    pub osc_transport: OSCTransport,
}

impl HostInfo {
    /// Creates a new `HostInfo` instance with default extensions.
    ///
    /// # Arguments
    /// * `name` - The name of the OSC server.
    /// * `osc_ip` - The IP address of the OSC server.
    /// * `osc_port` - The port of the OSC server.
    pub fn new(name: String, osc_ip: IpAddr, osc_port: u16) -> Self {
        HostInfo {
            name,
            // Default set of supported OSCQuery extensions.
            // Applications should accurately reflect which extensions they truly support.
            extensions: HashMap::from([
                ("ACCESS".to_string(), true),    // Supports querying node access mode.
                ("CLIPMODE".to_string(), false), // Example: Not supporting CLIPMODE by default.
                ("RANGE".to_string(), true),     // Supports querying node value ranges.
                ("TYPE".to_string(), true),      // Supports querying node OSC types.
                ("VALUE".to_string(), true),     // Supports querying current node values.
                                                 // "DESCRIPTION" is another common one, could be added if supported.
            ]),
            osc_ip,
            osc_port,
            osc_transport: OSCTransport::UDP, // Defaulting to UDP as it's common for OSC.
        }
    }
}

/// Enumerates the possible OSC transport protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OSCTransport {
    UDP,
    TCP,
}
