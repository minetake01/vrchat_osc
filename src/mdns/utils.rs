use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    sync::Arc,
};

use tokio::{net::UdpSocket, sync::RwLock};

use hickory_proto::{
    op::{Message, MessageType, OpCode},
    rr::{
        rdata::{A, AAAA, PTR, SRV},
        Name, RData, Record,
    },
    serialize::binary::BinEncodable,
};

use crate::mdns::{MDNS_IPV4_ADDR, MDNS_IPV6_ADDR, MDNS_PORT};

/// TTL (Time to Live) for mDNS records in seconds.
const RECORD_TTL: u32 = 120;

/// Gets the interface index for a given IP address using if_addrs.
///
/// # Arguments
/// * `ip` - The IP address to look up.
/// * `if_addrs` - A slice of network interfaces to search.
///
/// # Returns
/// An `Option<u32>` containing the interface index if found.
pub fn get_interface_index(ip: &IpAddr, if_addrs: &[if_addrs::Interface]) -> Option<u32> {
    for iface in if_addrs {
        match (&iface.addr, ip) {
            (if_addrs::IfAddr::V4(v4), IpAddr::V4(target)) if v4.ip == *target => {
                return iface.index;
            }
            (if_addrs::IfAddr::V6(v6), IpAddr::V6(target)) if v6.ip == *target => {
                return iface.index;
            }
            _ => {}
        }
    }
    None
}

/// Sets up a multicast socket for mDNS matching the advertised IP family.
///
/// This function creates a UDP socket, configures it for address reuse,
/// binds it to the mDNS port on a wildcard address, and joins the
/// mDNS multicast group on the interface matching the advertised IP.
///
/// # Arguments
/// * `if_addrs` - A slice of network interfaces to join multicast groups on.
/// * `advertised_ip` - The IP address to advertise, used to determine IPv4 or IPv6.
///
/// # Returns
/// A `Result` containing an `Arc<AsyncPktInfoUdpSocket>` if successful.
pub async fn setup_multicast_socket(
    advertised_ip: IpAddr,
) -> Result<Arc<UdpSocket>, std::io::Error> {
    match advertised_ip {
        IpAddr::V4(ipv4) => {
            let socket = socket2::Socket::new(
                socket2::Domain::IPV4,
                socket2::Type::DGRAM,
                Some(socket2::Protocol::UDP),
            )?;
            socket.set_reuse_address(true)?;
            #[cfg(target_family = "unix")]
            socket.set_reuse_port(true)?;
            socket.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, MDNS_PORT).into())?;
            socket.set_multicast_loop_v4(true)?;

            // Join multicast group on the advertised IP interface
            socket.join_multicast_v4(&MDNS_IPV4_ADDR, &ipv4)?;
            log::debug!("Joined mDNS IPv4 multicast group on interface {}", ipv4);

            // Set multicast interface to the advertised IP
            socket.set_multicast_if_v4(&ipv4)?;
            log::debug!("Set multicast IPv4 interface to {}", ipv4);

            socket.set_nonblocking(true)?;
            let socket = std::net::UdpSocket::from(socket);
            Ok(Arc::new(UdpSocket::from_std(socket)?))
        }
        IpAddr::V6(ipv6) => {
            let socket = socket2::Socket::new(
                socket2::Domain::IPV6,
                socket2::Type::DGRAM,
                Some(socket2::Protocol::UDP),
            )?;
            socket.set_reuse_address(true)?;
            #[cfg(target_family = "unix")]
            socket.set_reuse_port(true)?;
            socket.bind(&SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, MDNS_PORT, 0, 0).into())?;
            socket.set_multicast_loop_v6(true)?;

            // Find the interface index for the advertised IPv6 address
            let if_addrs = if_addrs::get_if_addrs()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            let if_index = get_interface_index(&advertised_ip, &if_addrs).unwrap_or(0);
            socket.join_multicast_v6(&MDNS_IPV6_ADDR, if_index)?;
            log::debug!(
                "Joined mDNS IPv6 multicast group on interface {} with index {}",
                ipv6,
                if_index
            );

            // Set multicast interface to the interface index
            socket.set_multicast_if_v6(if_index)?;
            log::debug!("Set multicast IPv6 interface to index {}", if_index);

            socket.set_nonblocking(true)?;
            let socket = std::net::UdpSocket::from(socket);
            Ok(Arc::new(UdpSocket::from_std(socket)?))
        }
    }
}

/// Sends mDNS service announcement using only the specified advertised IP address.
///
/// This function creates an mDNS response message with the advertised IP address
/// and sends it via the appropriate multicast interface.
///
/// # Arguments
/// * `socket` - An `AsyncPktInfoUdpSocket` used for sending the data.
/// * `instance_name` - The fully qualified name of the service instance.
/// * `port` - The port number where the service is hosted.
/// * `advertised_ip` - The IP address to advertise (used in A/AAAA records).
///
/// # Returns
/// A `Result` containing the number of bytes sent, or an error if sending fails.
pub async fn send_mdns_announcement(
    socket: &UdpSocket,
    instance_name: &Name,
    port: u16,
    advertised_ip: &Arc<RwLock<IpAddr>>,
) -> Result<usize, hickory_proto::ProtoError> {
    let ip = *advertised_ip.read().await;
    let response_message = create_mdns_response_message(instance_name, ip, port);
    let bytes = response_message.to_bytes()?;

    // Determine multicast address based on IP family
    let multicast_addr: SocketAddr = match ip {
        IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(MDNS_IPV4_ADDR), MDNS_PORT),
        IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(MDNS_IPV6_ADDR), MDNS_PORT),
    };

    match socket.send_to(&bytes, multicast_addr).await {
        Ok(n) => {
            log::debug!(
                "Sent mDNS announcement for {} with IP {} ({} bytes)",
                instance_name,
                ip,
                n
            );
            Ok(n)
        }
        Err(e) => {
            log::warn!(
                "Failed to send mDNS announcement for {} with IP {}: {}",
                instance_name,
                ip,
                e
            );
            Ok(0)
        }
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
pub fn create_mdns_response_message(
    instance_name: &Name,
    interface_ip: IpAddr,
    port: u16,
) -> Message {
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
