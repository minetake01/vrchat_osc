use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    sync::Arc,
};

use hickory_proto::{
    op::{Message, MessageType, OpCode},
    rr::{
        rdata::{A, AAAA, PTR, SRV},
        Name, RData, Record,
    },
    serialize::binary::BinEncodable,
};
use socket_pktinfo::{AsyncPktInfoUdpSocket, PktInfo};

use crate::mdns::{if_monitor::IfMonitor, MDNS_IPV4_ADDR, MDNS_IPV6_ADDR, MDNS_PORT};

/// TTL (Time to Live) for mDNS records in seconds.
const RECORD_TTL: u32 = 120;

/// Sets up IPv4 and IPv6 multicast sockets for mDNS.
///
/// This function creates UDP sockets, configures them for address reuse,
/// binds them to the mDNS port on wildcard addresses, and joins the
/// mDNS multicast groups on all provided network interfaces.
///
/// # Arguments
/// * `if_addrs` - A vector of network interfaces to join multicast groups on.
///
/// # Returns
/// A `Result` containing an array of two `Arc<AsyncPktInfoUdpSocket>`s (IPv4 and IPv6) if successful.
pub async fn setup_multicast_socket(
    if_addrs: Vec<if_addrs::Interface>,
) -> Result<[Arc<AsyncPktInfoUdpSocket>; 2], std::io::Error> {
    let socket_v4 = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    let socket_v6 = socket2::Socket::new(
        socket2::Domain::IPV6,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;

    for socket in [&socket_v4, &socket_v6] {
        socket.set_reuse_address(true)?;
        #[cfg(target_family = "unix")]
        socket.set_reuse_port(true)?;
    }

    socket_v4.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, MDNS_PORT).into())?;
    socket_v6.bind(&SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, MDNS_PORT, 0, 0).into())?;

    socket_v4.set_multicast_loop_v4(true)?;
    socket_v6.set_multicast_loop_v6(true)?;

    let mut joined_ifindexes = HashSet::new();
    for if_addr in if_addrs.iter() {
        match &if_addr.addr {
            if_addrs::IfAddr::V4(ifv4) => {
                socket_v4.join_multicast_v4(&MDNS_IPV4_ADDR, &ifv4.ip)?;
                log::debug!(
                    "Joined mDNS IPv4 multicast group on interface {} with address {}",
                    if_addr.name,
                    ifv4.ip
                );
            }
            if_addrs::IfAddr::V6(_) => {
                if let Some(if_index) = if_addr.index {
                    if joined_ifindexes.insert(if_index) {
                        socket_v6.join_multicast_v6(&MDNS_IPV6_ADDR, if_index)?;
                        log::debug!(
                            "Joined mDNS IPv6 multicast group on interface {} with index {}",
                            if_addr.name,
                            if_index
                        );
                    }
                }
            }
        }
    }

    Ok([
        Arc::new(AsyncPktInfoUdpSocket::from_std(socket_v4.into())?),
        Arc::new(AsyncPktInfoUdpSocket::from_std(socket_v6.into())?),
    ])
}

/// Sends mDNS service announcement across all available network interfaces.
///
/// This function creates an mDNS response message for each interface with the
/// appropriate IP address and sends it via multicast.
///
/// # Arguments
/// * `socket` - An `AsyncPktInfoUdpSocket` used for sending the data.
/// * `instance_name` - The fully qualified name of the service instance.
/// * `port` - The port number where the service is hosted.
/// * `if_monitor` - An `IfMonitor` instance to get current network interfaces.
///
/// # Returns
/// A `Result` containing the total number of bytes sent, or an error if sending fails.
pub async fn send_mdns_announcement(
    socket: &AsyncPktInfoUdpSocket,
    instance_name: &Name,
    port: u16,
    if_monitor: &IfMonitor,
) -> Result<usize, hickory_proto::ProtoError> {
    let local_addr = socket.local_addr()?;
    let ifs = if_monitor.get_interfaces().await;
    let mut total_sent: usize = 0;

    if local_addr.is_ipv4() {
        for if_addr in ifs.iter() {
            if let if_addrs::IfAddr::V4(ref v4) = if_addr.addr {
                let response_message =
                    create_mdns_response_message(instance_name, IpAddr::V4(v4.ip), port);
                let bytes = response_message.to_bytes()?;

                if let Err(e) = socket.set_multicast_if_v4(&v4.ip) {
                    log::warn!("Failed to set multicast IPv4 interface {}: {}", v4.ip, e);
                    continue;
                }
                match socket.send_to(&bytes, (MDNS_IPV4_ADDR, MDNS_PORT)).await {
                    Ok(n) => {
                        total_sent = total_sent.saturating_add(n);
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to send mDNS IPv4 announcement on interface {}: {}",
                            v4.ip,
                            e
                        );
                    }
                }
            }
        }
    } else if local_addr.is_ipv6() {
        let mut joined_ifindexes = std::collections::HashSet::new();
        for if_addr in ifs.iter() {
            if let if_addrs::IfAddr::V6(ref v6) = if_addr.addr {
                if let Some(idx) = if_addr.index {
                    if joined_ifindexes.insert(idx) {
                        let response_message =
                            create_mdns_response_message(instance_name, IpAddr::V6(v6.ip), port);
                        let bytes = response_message.to_bytes()?;

                        if let Err(e) = socket.set_multicast_if_v6(idx) {
                            log::warn!(
                                "Failed to set multicast IPv6 interface index {}: {}",
                                idx,
                                e
                            );
                            continue;
                        }
                        match socket.send_to(&bytes, (MDNS_IPV6_ADDR, MDNS_PORT)).await {
                            Ok(n) => {
                                total_sent = total_sent.saturating_add(n);
                            }
                            Err(e) => {
                                log::warn!(
                                    "Failed to send mDNS IPv6 announcement on if_index {}: {}",
                                    idx,
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(total_sent)
}

/// Sends byte data to the appropriate mDNS multicast address (IPv4 or IPv6)
/// across all available network interfaces for the socket's family.
///
/// # Arguments
/// * `socket` - An `AsyncPktInfoUdpSocket` used for sending the data. The socket's family
///   determines whether to use the IPv4 or IPv6 mDNS multicast address.
/// * `bytes` - A slice of bytes representing the mDNS message to send.
/// * `if_monitor` - An `IfMonitor` instance to get current network interfaces.
///
/// # Returns
/// A `Result` containing the total number of bytes sent, or an `std::io::Error` if sending fails.
pub async fn send_to_mdns(
    socket: &AsyncPktInfoUdpSocket,
    bytes: &[u8],
    if_monitor: &IfMonitor,
) -> Result<usize, std::io::Error> {
    // Send mDNS on all interfaces for the socket's IP family
    let local_addr = socket.local_addr()?;
    let ifs = if_monitor.get_interfaces().await;

    let mut total_sent: usize = 0;

    if local_addr.is_ipv4() {
        // Iterate all IPv4 interfaces and send via each
        for if_addr in ifs.into_iter() {
            if let if_addrs::IfAddr::V4(v4) = if_addr.addr {
                if let Err(e) = socket.set_multicast_if_v4(&v4.ip) {
                    log::warn!("Failed to set multicast IPv4 interface {}: {}", v4.ip, e);
                    continue;
                }
                match socket.send_to(bytes, (MDNS_IPV4_ADDR, MDNS_PORT)).await {
                    Ok(n) => {
                        total_sent = total_sent.saturating_add(n);
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to send mDNS IPv4 packet on interface {}: {}",
                            v4.ip,
                            e
                        );
                    }
                }
            }
        }
        Ok(total_sent)
    } else if local_addr.is_ipv6() {
        // Iterate all IPv6 interfaces and send via each
        let mut joined_ifindexes = std::collections::HashSet::new();
        for if_addr in ifs.into_iter() {
            if let if_addrs::IfAddr::V6(_v6) = if_addr.addr {
                if let Some(idx) = if_addr.index {
                    if joined_ifindexes.insert(idx) {
                        if let Err(e) = socket.set_multicast_if_v6(idx) {
                            log::warn!(
                                "Failed to set multicast IPv6 interface index {}: {}",
                                idx,
                                e
                            );
                            continue;
                        }
                        match socket.send_to(bytes, (MDNS_IPV6_ADDR, MDNS_PORT)).await {
                            Ok(n) => {
                                total_sent = total_sent.saturating_add(n);
                            }
                            Err(e) => {
                                log::warn!(
                                    "Failed to send mDNS IPv6 packet on if_index {}: {}",
                                    idx,
                                    e
                                );
                            }
                        }
                    }
                }
            }
        }
        Ok(total_sent)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "Socket local address is neither IPv4 nor IPv6",
        ))
    }
}

/// Converts service instance information (name and socket address) into a specially formatted
/// mDNS response `Message`.
///
/// This function creates a specific packet structure.
/// The message includes:
/// - A PTR record in the Answer section: `service_type_name -> instance_name`
/// - SRV, A (for IPv4), or AAAA (for IPv6) records in the Additional section for the `instance_name`.
///
/// # Arguments
/// * `instance_name` - The fully qualified name of the service instance (e.g., `MyInstance._myservice._tcp.local.`).
/// * `interface_ip` - The `IpAddr` to be included in the A/AAAA records.
/// * `port` - The port number where the service instance is hosted.
///
/// # Returns
/// An mDNS `Message` configured as a response, ready to be serialized and sent.
pub fn create_mdns_response_message(instance_name: &Name, interface_ip: IpAddr, port: u16) -> Message {
    let mut message = Message::new();
    message
        .set_id(0)
        .set_message_type(MessageType::Response)
        .set_op_code(OpCode::Query)
        .set_authoritative(true); // Indicates this is an authoritative answer for the records

    // --- PTR Record (Answer Section) ---
    // Points from the service type name to the service instance name.
    // e.g., _oscjson._tcp.local. PTR VRChat-Client-0000._oscjson._tcp.local.
    // The `instance_name.trim_to(3)` extracts the service type part.
    let service_type_name = instance_name.trim_to(3); // Assumes service type has 3 labels like _type._proto.local.
    message.add_answer(Record::from_rdata(
        service_type_name.clone(), // Owner: service type name
        RECORD_TTL,
        RData::PTR(PTR(instance_name.clone())), // RDATA: points to instance name
    ));

    // --- SRV Record (Additional Section) ---
    // Provides port and target host for the service instance.
    // e.g., VRChat-Client-0000._oscjson._tcp.local. SRV 0 0 9000 hostname.local.
    // The target in SRV should be the actual hostname providing the service.
    // Here, `instance_name` is used as the target.
    message.add_additional(Record::from_rdata(
        instance_name.clone(),
        RECORD_TTL,
        RData::SRV(SRV::new(0, 0, port, instance_name.clone())),
    ));

    // --- A or AAAA Record (Additional Section) ---
    // Provides the IP address for the target host specified in the SRV record (here, `instance_name`).
    match interface_ip {
        IpAddr::V4(ipv4_addr) => {
            message.add_additional(Record::from_rdata(
                instance_name.clone(),
                RECORD_TTL,
                RData::A(A(ipv4_addr)),
            ));
        }
        IpAddr::V6(ipv6_addr) => {
            message.add_additional(Record::from_rdata(
                instance_name.clone(),
                RECORD_TTL,
                RData::AAAA(AAAA(ipv6_addr)),
            ));
        }
    }
    message
}

/// Extracts service instance information (name, IP address, port) from a received mDNS `Message`.
///
/// This function is designed to parse the specific mDNS packet format that `convert_to_message` creates.
/// It looks for PTR, SRV, and A/AAAA records to reconstruct the service details.
///
/// # Arguments
/// * `message` - The parsed mDNS `Message` object (typically a response).
///
/// # Returns
/// An `Option<(Name, SocketAddr)>` containing the service instance name and its socket address
/// if all necessary information is found. Returns `None` otherwise.
pub fn extract_service_info(message: &Message) -> Option<(Name, SocketAddr)> {
    // We expect the following records for a complete service announcement:
    // 1. PTR record: `service_type.local. PTR instance_name.local.` (in Answers or Additionals)
    // 2. SRV record: `instance_name.local. SRV priority weight port target_host.local.` (in Additionals or Answers)
    // 3. A/AAAA record: `target_host.local. A/AAAA ip_address` (in Additionals or Answers)

    let mut ptr_instance_name: Option<Name> = None;
    let mut srv_target_name: Option<Name> = None;
    let mut srv_port: Option<u16> = None;
    let mut srv_actual_target_host: Option<Name> = None;

    let mut ip_address: Option<IpAddr> = None;
    let mut ip_owner_name: Option<Name> = None;

    // Iterate through all records in both Answers and Additional sections.
    for record in message.answers().iter().chain(message.additionals()) {
        match record.data() {
            RData::PTR(ptr_data) => {
                // PTR record: owner is service_type, data is instance_name
                if ptr_instance_name.is_none() {
                    ptr_instance_name = Some(ptr_data.0.clone());
                    log::trace!("Found PTR record: {} -> {}", record.name(), ptr_data.0);
                }
            }
            RData::SRV(srv_data) => {
                // SRV record: owner is instance_name (or srv_target_name)
                if srv_target_name.is_none() && srv_port.is_none() {
                    srv_target_name = Some(record.name().clone());
                    srv_port = Some(srv_data.port());
                    srv_actual_target_host = Some(srv_data.target().clone());
                    log::trace!(
                        "Found SRV record: {} port {} target {}",
                        record.name(),
                        srv_data.port(),
                        srv_data.target()
                    );
                }
            }
            RData::A(a_data) => {
                // A record: owner is target_host from SRV (or instance_name)
                if ip_address.is_none() {
                    ip_address = Some(IpAddr::V4(a_data.0));
                    ip_owner_name = Some(record.name().clone());
                    log::trace!("Found A record: {} -> {}", record.name(), a_data.0);
                }
            }
            RData::AAAA(aaaa_data) => {
                // AAAA record: owner is target_host from SRV (or instance_name)
                if ip_address.is_none() {
                    ip_address = Some(IpAddr::V6(aaaa_data.0));
                    ip_owner_name = Some(record.name().clone());
                    log::trace!("Found AAAA record: {} -> {}", record.name(), aaaa_data.0);
                }
            }
            _ => {} // Ignore other record types
        }
    }

    if let (
        Some(instance_name),
        Some(srv_owner),
        Some(port),
        Some(srv_target),
        Some(ip_addr_val),
        Some(ip_owner),
    ) = (
        ptr_instance_name.clone(),
        srv_target_name.clone(),
        srv_port,
        srv_actual_target_host.clone(),
        ip_address,
        ip_owner_name.clone(),
    ) {
        if srv_owner == instance_name && ip_owner == srv_target {
            log::trace!(
                "Successfully extracted service info: Name='{}', IP='{}', Port='{}'",
                instance_name,
                ip_addr_val,
                port
            );
            return Some((instance_name, SocketAddr::new(ip_addr_val, port)));
        } else {
            log::warn!(
                "Inconsistent records for service extraction. PTR instance: {}, SRV owner: {}, SRV target: {}, IP owner: {}",
                instance_name, srv_owner, srv_target, ip_owner
            );
        }
    } else {
        log::trace!("Could not extract complete service info. Missing one or more required records. PTR: {:?}, SRV_Owner: {:?}, Port: {:?}, SRV_Target: {:?}, IP: {:?}, IP_Owner: {:?}",
            ptr_instance_name.as_ref().map(|n| n.to_utf8()),
            srv_target_name.as_ref().map(|n| n.to_utf8()),
            srv_port,
            srv_actual_target_host.as_ref().map(|n| n.to_utf8()),
            ip_address,
            ip_owner_name.as_ref().map(|n| n.to_utf8())
        );
    }

    None
}

/// Resolves the local IP address for the interface that received the packet.
pub async fn resolve_interface_ip(
    pkt_info: &PktInfo,
    if_monitor: &Arc<IfMonitor>,
    prefer_ipv6: bool,
) -> IpAddr {
    if pkt_info.addr_dst.is_multicast() {
        let ifs = if_monitor.get_interfaces().await;
        let iface_addrs: Vec<_> = ifs
            .iter()
            .filter(|iface| iface.index == Some(pkt_info.if_index))
            .collect();

        // Try to find the preferred address family first.
        let found = iface_addrs
            .iter()
            .find(|iface| match iface.addr {
                if_addrs::IfAddr::V4(_) => !prefer_ipv6,
                if_addrs::IfAddr::V6(_) => prefer_ipv6,
            })
            .copied()
            // If the preferred family is not found, fallback to any address on the same interface.
            .or_else(|| iface_addrs.first().copied());

        found
            .map(|iface| match iface.addr {
                if_addrs::IfAddr::V4(ref v4) => IpAddr::V4(v4.ip),
                if_addrs::IfAddr::V6(ref v6) => IpAddr::V6(v6.ip),
            })
            .unwrap_or(pkt_info.addr_dst)
    } else {
        pkt_info.addr_dst
    }
}
