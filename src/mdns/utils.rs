use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use hickory_proto::{
    op::{Message, MessageType, OpCode},
    rr::{
        rdata::{A, AAAA, PTR, SRV},
        Name, RData, Record,
    },
};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;

const MDNS_PORT: u16 = 5353;
const MDNS_IPV4_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_IPV6_ADDR: Ipv6Addr = Ipv6Addr::new(0xFF02, 0, 0, 0, 0, 0, 0, 0xFB);
/// TTL (Time to Live) for mDNS records in seconds.
const RECORD_TTL: u32 = 120;

pub async fn setup_multicast_socket_v4() -> Result<UdpSocket, std::io::Error> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(target_family = "unix")]
    socket.set_reuse_port(true)?;
    socket.set_multicast_loop_v4(true)?;
    socket.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, MDNS_PORT).into())?;
    socket.set_nonblocking(true)?;
    let socket = UdpSocket::from_std(socket.into())?;
    socket.join_multicast_v4(MDNS_IPV4_ADDR, Ipv4Addr::UNSPECIFIED)?;
    Ok(socket)
}

pub async fn setup_multicast_socket_v6() -> Result<UdpSocket, std::io::Error> {
    let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(target_family = "unix")]
    socket.set_reuse_port(true)?;
    socket.set_multicast_loop_v6(true)?;
    socket.bind(&SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, MDNS_PORT, 0, 0).into())?;
    socket.set_nonblocking(true)?;
    let socket = UdpSocket::from_std(socket.into())?;
    socket.join_multicast_v6(&MDNS_IPV6_ADDR, 0)?;
    Ok(socket)
}

/// Sends byte data to the appropriate mDNS multicast address (IPv4 or IPv6)
/// based on the local address of the provided UDP socket.
///
/// # Arguments
/// * `socket` - An `Arc<UdpSocket>` used for sending the data. The socket's local address
///   determines whether to use the IPv4 or IPv6 mDNS multicast address.
/// * `bytes` - A slice of bytes representing the mDNS message to send.
///
/// # Returns
/// A `Result` containing the number of bytes sent, or an `std::io::Error` if sending fails.
pub async fn send_to_mdns(socket: &UdpSocket, bytes: &[u8]) -> Result<usize, std::io::Error> {
    let local_addr = socket.local_addr()?; // Get the local address of the socket

    // Determine the mDNS multicast address based on the IP version of the local socket.
    if local_addr.is_ipv4() {
        socket.send_to(bytes, (MDNS_IPV4_ADDR, MDNS_PORT)).await
    } else if local_addr.is_ipv6() {
        socket.send_to(bytes, (MDNS_IPV6_ADDR, MDNS_PORT)).await
    } else {
        // This case should ideally not be reached if sockets are bound correctly to IPv4 or IPv6.
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
/// * `addr` - The `SocketAddr` (IP address and port) where the service instance is hosted.
///
/// # Returns
/// An mDNS `Message` configured as a response, ready to be serialized and sent.
pub fn create_mdns_response_message(instance_name: &Name, addr: SocketAddr) -> Message {
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
        RData::SRV(SRV::new(0, 0, addr.port(), instance_name.clone())),
    ));

    // --- A or AAAA Record (Additional Section) ---
    // Provides the IP address for the target host specified in the SRV record (here, `instance_name`).
    match addr.ip() {
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
            log::debug!(
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
