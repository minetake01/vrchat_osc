mod host_info;
mod node;

pub use host_info::*;
pub use node::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn transport_keeps_its_wire_names() {
        let host_info = HostInfo::new("test".to_string(), IpAddr::V4(Ipv4Addr::LOCALHOST), 9000);

        assert_eq!(
            serde_json::to_value(&host_info).unwrap()["OSC_TRANSPORT"],
            "UDP"
        );

        let host_info = HostInfo {
            osc_transport: OscTransport::Tcp,
            ..host_info
        };
        assert_eq!(
            serde_json::to_value(&host_info).unwrap()["OSC_TRANSPORT"],
            "TCP"
        );
    }

    #[test]
    fn node_type_tag_keeps_its_wire_name() {
        let node = OscNode {
            osc_type: OscTypeTag::from_tag("f"),
            ..Default::default()
        };

        let json = serde_json::to_value(&node).unwrap();

        assert_eq!(json["TYPE"], "f");
        assert!(json.get("OSC_TYPE").is_none());
    }
}
