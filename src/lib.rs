mod fetch;
mod mdns;
mod oscquery;

/// Exposed because [`Error::HickoryError`] and [`MdnsError`] carry these types.
pub use hickory_proto;
pub use oscquery::models;
pub use rosc;

pub use fetch::Error as FetchError;
pub use mdns::Error as MdnsError;
pub use oscquery::Error as OscQueryError;

use crate::fetch::fetch;
use crate::oscquery::OscQuery;

use futures::{stream, StreamExt};
use hickory_proto::rr::{Label, Name};
use oscquery::models::{HostInfo, OscNode, OscRootNode};
use rosc::OscPacket;
use std::str::FromStr;
use std::{
    collections::HashMap,
    fmt,
    net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
};
use tokio::{
    net::UdpSocket,
    sync::{mpsc, RwLock},
    task::JoinHandle,
};
use wildmatch::WildMatch;

const OSC_PACKET_BUFFER_SIZE: usize = 65535; // Max UDP packet size

/// mDNS service type of OSC servers, which receive OSC packets over UDP.
const OSC_SERVICE_TYPE: &str = "_osc._udp.local.";
/// mDNS service type of OSCQuery servers, which expose the address space over HTTP.
const OSCQUERY_SERVICE_TYPE: &str = "_oscjson._tcp.local.";

/// Defines the possible errors that can occur within the VRChatOsc library.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("OSC error: {0}")]
    OscError(#[from] rosc::OscError),
    #[error("OSCQuery error: {0}")]
    OscQueryError(#[from] oscquery::Error),
    #[error("mDNS error: {0}")]
    MdnsError(#[from] mdns::Error),
    #[error("Hickory DNS protocol error: {0}")]
    HickoryError(#[from] hickory_proto::ProtoError),
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Fetch error: {0}")]
    FetchError(#[from] fetch::Error),
    #[error("No valid network interface found: no non-loopback IPv4 address available")]
    NoValidInterface,
    #[error("A service is already registered under the mDNS instance name {0}")]
    ServiceAlreadyRegistered(String),
}

/// Holds handles related to a registered OSC service.
struct ServiceHandle {
    /// Join handle for the OSC listening task.
    osc: JoinHandle<()>,
    /// The OSCQuery server instance.
    osc_query: OscQuery,
    /// The mDNS name the OSC server is announced under.
    osc_instance: Name,
    /// The mDNS name the OSCQuery server is announced under.
    oscquery_instance: Name,
}

/// A service discovered on the local network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    name: String,
    addr: SocketAddr,
}

impl Service {
    /// Creates a service from its instance name and address.
    pub(crate) fn new(name: impl Into<String>, addr: SocketAddr) -> Self {
        Self {
            name: name.into(),
            addr,
        }
    }

    /// The instance name of the service, without the mDNS service type suffix
    /// (for example `VRChat-Client-1234`).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The address the service listens on.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

impl fmt::Display for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.name, self.addr)
    }
}

/// Represents the type of OSC service discovered.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServiceType {
    /// OSC service type.
    Osc(Service),
    /// OSCQuery service type.
    OscQuery(Service),
}

/// Main struct for managing VRChat OSC communication and service discovery.
pub struct VRChatOsc {
    /// Socket for sending OSC messages.
    send_socket: UdpSocket,
    /// The destination IP address for OSC messages (VRChat's IP address).
    osc_ip: Arc<RwLock<IpAddr>>,
    /// mDNS client instance for service discovery.
    mdns: mdns::Mdns,
    /// Stores registered service handles, mapping service name to its handle.
    service_handles: Arc<RwLock<HashMap<String, ServiceHandle>>>,
    /// Callback function to be executed when a new mDNS service is discovered.
    on_service_discovered_callback:
        Arc<RwLock<Option<Arc<dyn Fn(ServiceType) + Send + Sync + 'static>>>>,
}

/// Finds the first non-loopback IPv4 address among the local interfaces.
///
/// The interfaces are scanned in the order the OS reports them and the first
/// IPv4 address that is not a loopback address is returned. Link state is not
/// inspected, so on a host with several interfaces (VPNs, virtual adapters) the
/// result may not be the interface that reaches VRChat. Pass `Some(ip)` to
/// [`VRChatOsc::new`] when that matters.
///
/// # Returns
/// A non-loopback IPv4 address, or None if no suitable interface is found.
fn find_non_loopback_ipv4() -> Option<IpAddr> {
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for iface in interfaces {
            if let std::net::IpAddr::V4(ipv4) = iface.addr.ip() {
                if !ipv4.is_loopback() {
                    return Some(IpAddr::V4(ipv4));
                }
            }
        }
    }
    None
}

/// Finds the local IP address that can reach the given destination IP.
///
/// This function connects a temporary UDP socket to the destination to let the
/// OS routing table determine the appropriate interface, then returns the local address.
///
/// # Arguments
/// * `dest_ip` - The destination IP address to reach.
///
/// # Returns
/// The local IP address on the interface that can reach the destination.
fn find_local_ip_for_destination(dest_ip: IpAddr) -> Result<IpAddr, Error> {
    // If the destination IP is a loopback address, return it directly.
    if dest_ip.is_loopback() {
        return Ok(dest_ip);
    }

    // Check if the destination IP is one of our local interface addresses.
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for iface in interfaces {
            if iface.addr.ip() == dest_ip {
                return Ok(dest_ip);
            }
        }
    }

    // Create a UDP socket and connect to the destination (without sending data).
    // The OS will determine the correct interface based on routing table.
    let socket = match dest_ip {
        IpAddr::V4(_) => std::net::UdpSocket::bind("0.0.0.0:0")?,
        IpAddr::V6(_) => std::net::UdpSocket::bind("[::]:0")?,
    };

    socket.connect((dest_ip, 0))?;
    Ok(socket.local_addr()?.ip())
}

/// Returns the instance name of a discovered service, that is, the label before
/// the service type (`VRChat-Client-1234._osc._udp.local.` -> `VRChat-Client-1234`).
fn instance_name(name: &Name) -> String {
    name.iter()
        .next()
        .and_then(|label| Label::from_raw_bytes(label).ok())
        .map(|label| label.to_utf8())
        .unwrap_or_default()
}

/// Returns whether a discovered service belongs to the given service type.
/// DNS names compare case-insensitively.
fn is_service_type(name: &Name, service_type: &str) -> bool {
    name.trim_to(3).to_utf8().eq_ignore_ascii_case(service_type)
}

/// Returns whether a discovered service is of the given service type and its
/// instance name matches a glob pattern.
fn matches_service(name: &Name, service_type: &str, pattern: &str) -> bool {
    is_service_type(name, service_type) && WildMatch::new(pattern).matches(&instance_name(name))
}

/// Builds the discovered service from its mDNS name, or `None` when the name
/// does not belong to a service type this crate follows.
fn discovered_service(name: &Name, addr: SocketAddr) -> Option<ServiceType> {
    let service = Service::new(instance_name(name), addr);

    if is_service_type(name, OSC_SERVICE_TYPE) {
        Some(ServiceType::Osc(service))
    } else if is_service_type(name, OSCQUERY_SERVICE_TYPE) {
        Some(ServiceType::OscQuery(service))
    } else {
        None
    }
}

/// Builds the mDNS name a local service is announced under.
fn local_service_instance(service_name: &str, service_type: &str) -> Result<Name, Error> {
    Ok(Name::from_str(&format!(
        "{}.{}",
        sanitize_service_name(service_name),
        service_type
    ))?)
}

/// Sanitizes the service name to be compatible with mDNS and VRChat requirements.
///
/// Rules:
/// - Keep ASCII alphanumerics, hyphens, and all non-ASCII characters.
/// - Replace any other ASCII characters (including spaces and symbols) with '-'.
/// - Replace Unicode control characters with '-'.
/// - Replace uppercase ASCII characters with lowercase.
fn sanitize_service_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if (c.is_ascii() && !c.is_ascii_alphanumeric()) || c.is_control() {
                '-'
            } else if c.is_ascii_uppercase() {
                c.to_ascii_lowercase()
            } else {
                c
            }
        })
        .collect()
}

impl VRChatOsc {
    /// Creates a new `VRChatOsc` instance.
    ///
    /// # Arguments
    /// * `osc_ip` - Optional destination IP address for OSC messages.
    ///   If `None`, it will attempt to automatically find a suitable network interface.
    pub async fn new(osc_ip: Option<IpAddr>) -> Result<Self, Error> {
        let osc_ip = match osc_ip {
            Some(ip) => ip,
            None => find_non_loopback_ipv4().ok_or(Error::NoValidInterface)?,
        };
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await?;

        // Create an mpsc channel for notifying about discovered mDNS services.
        let (discover_notifier_tx, mut discover_notifier_rx) = mpsc::channel(8);

        // Derive the advertised IP (our local interface IP) from the OSC destination IP.
        // This finds the local IP address on the same network as the destination.
        let advertised_ip = find_local_ip_for_destination(osc_ip)?;

        // Initialize the mDNS client with our local interface IP for announcements.
        let mdns_client = mdns::Mdns::new(discover_notifier_tx, advertised_ip).await?;

        // Start following OSC services and OSCQuery JSON services on the local network.
        let _ = mdns_client
            .follow(Name::from_ascii(OSC_SERVICE_TYPE)?)
            .await;
        let _ = mdns_client
            .follow(Name::from_ascii(OSCQUERY_SERVICE_TYPE)?)
            .await;

        // Prepare a shared storage for the service discovered callback.
        let on_service_discovered_callback = Arc::new(RwLock::new(
            None::<Arc<dyn Fn(ServiceType) + Send + Sync + 'static>>,
        ));
        let callback_arc_clone = on_service_discovered_callback.clone();

        // Spawn a new asynchronous task to listen for service discovery notifications.
        // This task will own the `discover_notifier_rx` (receiver end of the mpsc channel).
        tokio::spawn(async move {
            // Receive notifications until the mDNS task closes the channel, which
            // happens when it is dropped, and then let this task end instead of
            // spinning on `None`.
            while let Some((service_name, socket_addr)) = discover_notifier_rx.recv().await {
                let Some(service_type) = discovered_service(&service_name, socket_addr) else {
                    continue;
                };

                let callback_guard = callback_arc_clone.read().await;
                // If a callback is registered, invoke it with the discovered service.
                if let Some(callback) = callback_guard.as_ref() {
                    callback(service_type);
                }
            }
        });

        Ok(Self {
            send_socket: socket,
            osc_ip: Arc::new(RwLock::new(osc_ip)),
            mdns: mdns_client,
            service_handles: Arc::new(RwLock::new(HashMap::new())),
            on_service_discovered_callback,
        })
    }

    /// Sets the destination IP address for OSC messages.
    ///
    /// # Arguments
    /// * `ip` - The new destination IP address. If `None`, it will attempt to automatically find a suitable network interface.
    pub async fn set_osc_ip(&self, ip: Option<IpAddr>) -> Result<(), Error> {
        let ip = match ip {
            Some(ip) => ip,
            None => find_non_loopback_ipv4().ok_or(Error::NoValidInterface)?,
        };
        *self.osc_ip.write().await = ip;
        let advertised_ip = find_local_ip_for_destination(ip)?;
        self.mdns.set_advertised_ip(advertised_ip).await;
        Ok(())
    }

    /// Gets the currently configured OSC destination IP address.
    pub async fn get_osc_ip(&self) -> IpAddr {
        *self.osc_ip.read().await
    }

    /// Registers a callback to be invoked when a new OSC service is discovered on the network.
    ///
    /// # Arguments
    /// * `callback` - A function or closure called with the discovered service details.
    pub async fn on_connect<F>(&self, callback: F)
    where
        F: Fn(ServiceType) + Send + Sync + 'static,
    {
        let mut callback_guard = self.on_service_discovered_callback.write().await;
        *callback_guard = Some(Arc::new(callback));
    }

    /// Registers a local OSC service to be discoverable by other clients.
    ///
    /// # Arguments
    /// * `service_name` - The name of the service to register.
    /// * `parameters` - The OSC address space and parameters for this service.
    /// * `handler` - A function called when an OSC packet is received for this service.
    pub async fn register<F>(
        &self,
        service_name: &str,
        parameters: OscRootNode,
        handler: F,
    ) -> Result<(), Error>
    where
        F: Fn(OscPacket) + Send + 'static,
    {
        // Every step that can fail runs before anything is spawned or announced,
        // so an error leaves nothing behind to roll back. The write lock is held
        // throughout so that a concurrent registration cannot claim the same name.
        let osc_instance = local_service_instance(service_name, OSC_SERVICE_TYPE)?;
        let oscquery_instance = local_service_instance(service_name, OSCQUERY_SERVICE_TYPE)?;

        let mut handles = self.service_handles.write().await;
        if handles
            .values()
            .any(|handle| handle.osc_instance == osc_instance)
        {
            return Err(Error::ServiceAlreadyRegistered(osc_instance.to_utf8()));
        }

        // Bind the OSC server (UDP listener) to an ephemeral port on all interfaces.
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await?;
        let osc_local_addr = socket.local_addr()?; // Get the actual address it bound to.

        // Start OSCQuery server (HTTP server)
        let host_info = HostInfo::new(
            service_name.to_string(),
            osc_local_addr.ip(),   // Use the IP of the OSC server.
            osc_local_addr.port(), // Use the port of the OSC server.
        );
        let mut osc_query = OscQuery::new(host_info, parameters);
        // Serve OSCQuery on an ephemeral port on all interfaces.
        let osc_query_local_addr = osc_query
            .serve(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
            .await?;

        // Spawn a task to handle incoming OSC packets.
        let osc_handle = tokio::spawn(async move {
            let mut buf = [0; OSC_PACKET_BUFFER_SIZE]; // Buffer for receiving OSC packets.
            loop {
                // Wait to receive data on the socket.
                match socket.recv_from(&mut buf).await {
                    Ok((len, addr)) => {
                        // Decode the received UDP data into an OSC packet.
                        if let Ok((_, packet)) = rosc::decoder::decode_udp(&buf[..len]) {
                            handler(packet); // Call the provided handler with the decoded packet.
                        } else {
                            log::debug!("Failed to decode OSC packet from {}", addr);
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::ConnectionReset
                            || e.kind() == std::io::ErrorKind::BrokenPipe
                        {
                            log::warn!("Socket connection error ({}). Task for {:?} might need to be restarted or interface is down.", e, socket.local_addr().ok());
                            break;
                        } else {
                            log::warn!(
                                "Failed to receive data on OSC socket {:?}: {}",
                                socket.local_addr().ok(),
                                e
                            );
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        continue;
                    }
                }
            }
        });

        // Announce the OSC and OSCQuery services with mDNS.
        self.mdns
            .register(osc_instance.clone(), osc_local_addr.port())
            .await;
        self.mdns
            .register(oscquery_instance.clone(), osc_query_local_addr.port())
            .await;

        handles.insert(
            service_name.to_string(),
            ServiceHandle {
                osc: osc_handle,
                osc_query,
                osc_instance,
                oscquery_instance,
            },
        );
        Ok(())
    }

    /// Unregisters an OSC service and stops its servers.
    ///
    /// # Arguments
    /// * `service_name` - The name the service was registered with.
    pub async fn unregister(&self, service_name: &str) {
        let mut handles = self.service_handles.write().await;
        if let Some(handle) = handles.remove(service_name) {
            self.stop_service(handle).await;
        }
    }

    /// Withdraws a service from mDNS and stops its servers.
    ///
    /// Callers hold the `service_handles` write lock until this returns. mDNS
    /// withdraws by name alone, so releasing the lock earlier would let a
    /// registration of the same name complete in between and be withdrawn here.
    async fn stop_service(&self, mut handle: ServiceHandle) {
        self.mdns.unregister(&handle.osc_instance).await;
        self.mdns.unregister(&handle.oscquery_instance).await;
        handle.osc.abort();
        handle.osc_query.shutdown();
    }

    /// Sends an OSC packet to services matching a name pattern.
    ///
    /// # Arguments
    /// * `packet` - The `OscPacket` to send.
    /// * `to` - A glob pattern (e.g., "VRChat-Client-*") matched against the service instance name.
    ///
    /// # Returns
    /// The number of discovered services the packet was addressed to.
    pub async fn send(&self, packet: OscPacket, to: &str) -> Result<usize, Error> {
        // OSC packets go to OSC servers only; the OSCQuery server of the same
        // instance listens on a different port and speaks HTTP.
        let services = self
            .mdns
            .find_service(|name, _| matches_service(name, OSC_SERVICE_TYPE, to))
            .await;
        if services.is_empty() {
            log::info!("No mDNS services found matching the expression: {}", to);
            return Ok(0);
        }

        // Encode the OSC packet into bytes.
        let msg_buf = rosc::encoder::encode(&packet)?;
        // Send the packet to all found services.
        let send_futs = services
            .into_iter()
            .map(|(_, addr)| self.send_socket.send_to(&msg_buf, addr));
        let results = futures::future::join_all(send_futs).await;

        let mut addressed = 0;
        for res in results {
            res?;
            addressed += 1;
        }

        Ok(addressed)
    }

    /// Sends an OSC packet to a specific socket address.
    ///
    /// # Arguments
    /// * `packet` - The `OscPacket` to send.
    /// * `addr` - The `SocketAddr` to send the packet to.
    pub async fn send_to_addr(&self, packet: OscPacket, addr: SocketAddr) -> Result<(), Error> {
        let msg_buf = rosc::encoder::encode(&packet)?;
        self.send_socket.send_to(&msg_buf, addr).await?;
        Ok(())
    }

    /// Retrieves a specific parameter from services matching a name pattern.
    ///
    /// # Arguments
    /// * `method` - The OSC path of the parameter (e.g., "/avatar/parameters/MyParam").
    /// * `from` - A glob pattern (e.g., "VRChat-Client-*") matched against the service instance name.
    ///
    /// # Returns
    /// One entry per matching service, pairing the service with the result of
    /// fetching the parameter from it. A service that could not be reached, or that
    /// does not expose `method`, is reported as `Err`.
    pub async fn get_parameter(
        &self,
        method: &str,
        from: &str,
    ) -> Vec<(Service, Result<OscNode, Error>)> {
        // Parameters are fetched from OSCQuery servers only.
        let services = self
            .mdns
            .find_service(|name, _| matches_service(name, OSCQUERY_SERVICE_TYPE, from))
            .await;

        // Asynchronously fetch the parameter from all matching services.
        // `stream::iter` creates a stream from the services.
        // `map` transforms each service into a future that fetches the parameter.
        // `buffer_unordered(3)` allows up to 3 fetches to run concurrently.
        // `collect` keeps every result, so callers can tell which service failed.
        stream::iter(services)
            .map(|(name, addr)| async move {
                let parameter = fetch::<_, OscNode>(addr, method)
                    .await
                    .map(|(parameter, _)| parameter)
                    .map_err(Error::from);

                (Service::new(instance_name(&name), addr), parameter)
            })
            .buffer_unordered(3)
            .collect()
            .await
    }

    /// Retrieves a specific parameter from a specific service address.
    ///
    /// # Arguments
    /// * `method` - The OSC path of the parameter (e.g., "/avatar/parameters/MyParam").
    /// * `addr` - The address of the service.
    ///
    /// # Returns
    /// The fetched parameter value.
    pub async fn get_parameter_from_addr(
        &self,
        method: &str,
        addr: SocketAddr,
    ) -> Result<OscNode, Error> {
        let (param, _url) = fetch::<_, OscNode>(addr, method).await?;
        Ok(param)
    }

    /// Shuts down all registered services and cleans up resources.
    /// This method should be called before the VRChatOsc instance is dropped
    /// to ensure graceful shutdown of asynchronous tasks and network services.
    pub async fn shutdown(&self) {
        let mut handles = self.service_handles.write().await;
        for (_, handle) in handles.drain() {
            self.stop_service(handle).await;
        }
    }

    /// Lists the names of all currently registered services.
    pub async fn list_services(&self) -> Vec<String> {
        let handles = self.service_handles.read().await;
        handles.keys().cloned().collect()
    }
}

impl Drop for VRChatOsc {
    fn drop(&mut self) {
        // Best-effort synchronous cleanup.
        // For robust cleanup, especially of async tasks and network resources,
        // the asynchronous `shutdown` method should be called explicitly.
        if let Ok(mut handles) = self.service_handles.try_write() {
            let service_names: Vec<String> = handles.keys().cloned().collect();
            for name in service_names {
                if let Some(mut service_handle) = handles.remove(&name) {
                    // mDNS unregistration cannot be reliably called here due to async and potential blocking.
                    service_handle.osc.abort();
                    // OscQuery::shutdown() is assumed to be synchronous or non-blocking here.
                    // If it's async, it cannot be .await-ed in drop.
                    service_handle.osc_query.shutdown();
                }
            }
        } else {
            // This might happen if the lock is poisoned or contended in a way not suitable for drop.
            // In a real application, this should be logged or handled appropriately.
            // Using log::error! or eprintln! here might be appropriate.
            // For now, we acknowledge that proper async shutdown is preferred.
            if !std::thread::panicking() {
                // Avoid double panic if already panicking
                log::warn!("VRChatOsc: Could not acquire lock on service_handles during drop. Explicitly call shutdown() for robust cleanup.");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr() -> SocketAddr {
        "192.168.1.20:9000".parse().unwrap()
    }

    #[test]
    fn discovered_service_strips_the_service_type() {
        let osc = Name::from_ascii("VRChat-Client-1234._osc._udp.local.").unwrap();
        assert_eq!(
            discovered_service(&osc, addr()),
            Some(ServiceType::Osc(Service::new("VRChat-Client-1234", addr())))
        );

        let oscjson = Name::from_ascii("VRChat-Client-1234._oscjson._tcp.local.").unwrap();
        assert_eq!(
            discovered_service(&oscjson, addr()),
            Some(ServiceType::OscQuery(Service::new(
                "VRChat-Client-1234",
                addr()
            )))
        );

        let other = Name::from_ascii("printer._http._tcp.local.").unwrap();
        assert_eq!(discovered_service(&other, addr()), None);
    }

    #[test]
    fn patterns_match_the_instance_name() {
        let name = Name::from_ascii("VRChat-Client-1234._osc._udp.local.").unwrap();

        assert!(matches_service(&name, OSC_SERVICE_TYPE, "VRChat-Client-*"));
        assert!(matches_service(
            &name,
            OSC_SERVICE_TYPE,
            "VRChat-Client-1234"
        ));
        assert!(!matches_service(&name, OSC_SERVICE_TYPE, "Other-*"));
    }

    #[test]
    fn patterns_select_only_the_requested_service_type() {
        // VRChat announces an OSC and an OSCQuery service under the same instance name.
        let osc = Name::from_ascii("VRChat-Client-1234._osc._udp.local.").unwrap();
        let oscquery = Name::from_ascii("VRChat-Client-1234._oscjson._tcp.local.").unwrap();

        assert!(matches_service(&osc, OSC_SERVICE_TYPE, "VRChat-Client-*"));
        assert!(!matches_service(
            &oscquery,
            OSC_SERVICE_TYPE,
            "VRChat-Client-*"
        ));

        assert!(matches_service(
            &oscquery,
            OSCQUERY_SERVICE_TYPE,
            "VRChat-Client-*"
        ));
        assert!(!matches_service(
            &osc,
            OSCQUERY_SERVICE_TYPE,
            "VRChat-Client-*"
        ));
    }

    #[test]
    fn service_types_compare_case_insensitively() {
        let name = Name::from_ascii("VRChat-Client-1234._OSC._UDP.local.").unwrap();

        assert!(matches_service(&name, OSC_SERVICE_TYPE, "VRChat-Client-*"));
        assert_eq!(
            discovered_service(&name, addr()),
            Some(ServiceType::Osc(Service::new("VRChat-Client-1234", addr())))
        );
    }

    #[tokio::test]
    async fn registering_a_taken_name_fails_without_replacing_the_service() {
        let vrchat_osc = VRChatOsc::new(Some(IpAddr::V4(Ipv4Addr::LOCALHOST)))
            .await
            .unwrap();
        vrchat_osc
            .register("Test Service", OscRootNode::new(), |_| {})
            .await
            .unwrap();

        // Both names sanitize to the same mDNS instance name.
        let result = vrchat_osc
            .register("test-service", OscRootNode::new(), |_| {})
            .await;

        assert!(matches!(result, Err(Error::ServiceAlreadyRegistered(_))));
        assert_eq!(vrchat_osc.list_services().await, vec!["Test Service"]);
        vrchat_osc.shutdown().await;
    }
}
