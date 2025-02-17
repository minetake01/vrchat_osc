use std::{collections::HashMap, net::IpAddr};

use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub struct HostInfo {
    pub name: String,
    pub extensions: HashMap<String, bool>,
    pub osc_ip: IpAddr,
    pub osc_port: u16,
    pub osc_transport: OSCTransport,
}

impl HostInfo {
    pub fn new(name: String, osc_ip: IpAddr, osc_port: u16) -> Self {
        HostInfo {
            name,
            extensions: HashMap::from([
                ("ACCESS".to_string(), true),
                ("CLIPMODE".to_string(), false),
                ("RANGE".to_string(), true),
                ("TYPE".to_string(), true),
                ("VALUE".to_string(), true),
            ]),
            osc_ip,
            osc_port,
            osc_transport: OSCTransport::UDP,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OSCTransport {
    UDP,
    TCP,
}

impl Default for OSCTransport {
    fn default() -> Self {
        OSCTransport::UDP
    }
}