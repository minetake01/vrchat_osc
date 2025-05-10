mod task;
mod utils;

use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc, time::Duration
};

use hickory_proto::{
    multicast::{MDNS_IPV4, MDNS_IPV6},
    op::{Message, Query},
    rr::{Name, RecordType},
    serialize::binary::BinEncodable,
};
use socket2::{Domain, Protocol, Socket, Type};
use task::server_task;
use tokio::{
    net::UdpSocket,
    sync::{mpsc, RwLock},
    task::JoinHandle,
};
use utils::{convert_to_message, send_to_mdns};

/// Maximum number of attempts to send a multicast message.
const MAX_SEND_ATTEMPTS: usize = 3;
/// Default TTL for mDNS packets.
const MDNS_TTL: u32 = 255;

/// Error types that can occur during mDNS operations.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Errors from the underlying DNS library (hickory-proto).
    #[error("DNS Protocol Error: {0}")]
    MdnsProtoError(#[from] hickory_proto::ProtoError),

    /// I/O errors during network operations.
    #[error("I/O Error: {0}")]
    IoError(#[from] std::io::Error),

    /// This error indicates an unexpected internal state, typically an invalid IP version
    /// where IPv4 or IPv6 was expected but something else was encountered.
    #[error("Invalid IP Version - Expected IPv4 or IPv6 based on context")]
    InvalidIpVersion,
}

/// Represents a task associated with a UDP socket for mDNS operations.
struct MdnsTask {
    socket: Arc<UdpSocket>,
    handle: JoinHandle<()>,
}

/// Main structure for handling mDNS service discovery and advertisement
/// across all available network interfaces.
pub struct Mdns {
    /// List of active mDNS tasks, typically one for IPv4 and one for IPv6,
    /// each operating on all relevant interfaces.
    tasks: Arc<RwLock<Vec<MdnsTask>>>,

    /// A map of registered services.
    /// The first key is the service type name (e.g., `_oscjson._tcp.local.`).
    /// The second key is the service instance name (e.g., `MyInstance._oscjson._tcp.local.`),
    /// mapped to its `SocketAddr`.
    registered_services: Arc<RwLock<HashMap<Name, HashMap<Name, SocketAddr>>>>,

    /// Cache of discovered services.
    /// Maps service instance name to its `SocketAddr`.
    service_cache: Arc<RwLock<HashMap<Name, SocketAddr>>>,

    /// Set of service type names that are actively being followed (i.e., interested in discovering).
    follow_services: Arc<RwLock<HashSet<Name>>>,
}

impl Mdns {
    /// Creates a new mDNS instance that operates on all available network interfaces.
    ///
    /// This function attempts to bind UDP sockets to wildcard addresses for IPv4 (`0.0.0.0`)
    /// and IPv6 (`[::]`) to listen for and send mDNS packets across all relevant interfaces.
    /// Background tasks are spawned for each successfully bound socket (typically one for IPv4, one for IPv6).
    ///
    /// # Arguments
    /// * `notifier_tx` - An `mpsc::Sender` to send notifications of discovered services.
    ///
    /// # Returns
    /// A `Result` containing the new `Mdns` instance or an `Error` if initialization fails
    /// (e.g., if no sockets could be bound).
    pub async fn new(
        notifier_tx: mpsc::Sender<(Name, SocketAddr)>,
    ) -> Result<Self, Error> {
        let registered_services = Arc::new(RwLock::new(HashMap::new()));
        let service_cache = Arc::new(RwLock::new(HashMap::new()));
        let follow_services = Arc::new(RwLock::new(HashSet::new()));
        let mut tasks_vec = Vec::new();

        // Attempt to set up a socket for IPv4 on all interfaces
        let ipv4_socket_ip = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
        match Self::setup_multicast_socket(ipv4_socket_ip).await {
            Ok(socket) => {
                let task_handle = tokio::spawn(server_task(
                    socket.clone(),
                    notifier_tx.clone(),
                    registered_services.clone(),
                    service_cache.clone(),
                    follow_services.clone(),
                ));
                tasks_vec.push(MdnsTask { socket, handle: task_handle });
                log::info!("Successfully set up mDNS for IPv4 on all interfaces (bound to {}).", ipv4_socket_ip);
            }
            Err(e) => {
                log::warn!("Failed to set up mDNS for IPv4 on all interfaces (bind attempt to {}): {}. IPv4 mDNS might be unavailable.", ipv4_socket_ip, e);
            }
        }

        // Attempt to set up a socket for IPv6 on all interfaces
        let ipv6_socket_ip = IpAddr::V6(Ipv6Addr::UNSPECIFIED);
        match Self::setup_multicast_socket(ipv6_socket_ip).await {
            Ok(socket) => {
                let task_handle = tokio::spawn(server_task(
                    socket.clone(),
                    notifier_tx.clone(),
                    registered_services.clone(),
                    service_cache.clone(),
                    follow_services.clone(),
                ));
                tasks_vec.push(MdnsTask { socket, handle: task_handle });
                log::info!("Successfully set up mDNS for IPv6 on all interfaces (bound to {}).", ipv6_socket_ip);
            }
            Err(e) => {
                log::warn!("Failed to set up mDNS for IPv6 on all interfaces (bind attempt to {}): {}. IPv6 mDNS might be unavailable.", ipv6_socket_ip, e);
            }
        }

        if tasks_vec.is_empty() {
            log::error!("No mDNS tasks started. Failed to bind to any wildcard address. mDNS will not function.");
            return Err(Error::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Failed to initialize mDNS on any interface family (IPv4 or IPv6).",
            )));
        }

        Ok(Mdns {
            tasks: Arc::new(RwLock::new(tasks_vec)),
            registered_services,
            service_cache,
            follow_services,
        })
    }

    /// Sets up a UDP socket for mDNS communication, attempting to use the specified IP address
    /// (typically a wildcard address like `0.0.0.0` or `[::]`) for broad interface coverage.
    /// Configures multicast join, loopback, and TTL.
    ///
    /// # Arguments
    /// * `bind_ip` - The IP address to bind the socket to. For "all interfaces",
    ///   this will be `Ipv4Addr::UNSPECIFIED` or `Ipv6Addr::UNSPECIFIED`.
    ///
    /// # Returns
    /// A `Result` containing the `Arc<UdpSocket>` or an `Error`.
    async fn setup_multicast_socket(bind_ip: IpAddr) -> Result<Arc<UdpSocket>, Error> {
        let bind_addr = SocketAddr::new(bind_ip, MDNS_IPV4.port()); // mDNS uses port 5353 for both v4/v6

        let std_socket = Socket::new(Domain::for_address(bind_addr), Type::DGRAM, Some(Protocol::UDP))?;
        std_socket.set_reuse_address(true)?;
        std_socket.bind(&bind_addr.into())?;
        let socket = UdpSocket::from_std(std_socket.into())?;
        log::debug!("Socket bound to {}", bind_addr);

        // Configure multicast settings based on IP version.
        if bind_ip.is_ipv4() {
            let IpAddr::V4(mdns_multicast_ipv4) = MDNS_IPV4.ip() else {
                return Err(Error::InvalidIpVersion);
            };
            let interface_to_join_on = Ipv4Addr::UNSPECIFIED;
            socket.join_multicast_v4(mdns_multicast_ipv4, interface_to_join_on)?;
            socket.set_multicast_ttl_v4(MDNS_TTL)?;
            log::debug!("Joined IPv4 multicast group {} on interface {}", mdns_multicast_ipv4, interface_to_join_on);
        } else if bind_ip.is_ipv6() {
            let IpAddr::V6(mdns_multicast_ipv6) = MDNS_IPV6.ip() else {
                return Err(Error::InvalidIpVersion);
            };
            let interface_index_to_join_on: u32 = 0;
            socket.join_multicast_v6(&mdns_multicast_ipv6, interface_index_to_join_on)?;
            log::debug!("Joined IPv6 multicast group {} on interface index {}", mdns_multicast_ipv6, interface_index_to_join_on);
        } else {
            return Err(Error::InvalidIpVersion);
        }
        Ok(Arc::new(socket))
    }

    /// Registers a service with mDNS.
    ///
    /// This adds the service to an internal registry and advertises it on the network
    /// across all active interfaces (via the wildcard-bound sockets). The service is identified
    /// by its instance name and its network address.
    ///
    /// # Arguments
    /// * `instance_name` - The unique name of the service instance (e.g., `VRChat-Client-1234._oscjson._tcp.local.`).
    /// * `addr` - The `SocketAddr` (IP address and port) where the service is hosted.
    ///
    /// # Returns
    /// A `Result` indicating success or an `Error` if registration fails.
    pub async fn register(&self, instance_name: Name, addr: SocketAddr) -> Result<(), Error> {
        let base_service_name = instance_name.trim_to(3);

        {
            let mut services_guard = self.registered_services.write().await;
            let instances = services_guard.entry(base_service_name.clone()).or_default();
            instances.insert(instance_name.clone(), addr);
        }

        log::info!("Registered service: {} at {}", instance_name, addr);

        let response_message = convert_to_message(&instance_name, addr);
        let bytes = response_message.to_bytes()?;

        let tasks_guard = self.tasks.read().await;

        for _ in 0..MAX_SEND_ATTEMPTS {
            for task in tasks_guard.iter() {
                if let Err(e) = send_to_mdns(&task.socket, &bytes).await {
                    log::error!(
                        "Failed to send registration announcement for {} via {:?}: {}",
                        instance_name, task.socket.local_addr().ok(), e
                    );
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(())
    }

    /// Unregisters a previously registered service.
    ///
    /// This removes the service from the internal registry.
    /// (Note: Sending a "goodbye" packet is not implemented here yet).
    ///
    /// # Arguments
    /// * `instance_name` - The name of the service instance to unregister.
    ///
    /// # Returns
    /// A `Result` indicating success or an `Error`.
    pub async fn unregister(&self, instance_name: Name) -> Result<(), Error> {
        let base_service_name = instance_name.trim_to(3);

        let mut services_guard = self.registered_services.write().await;
        let mut removed = false;
        if let Some(instances) = services_guard.get_mut(&base_service_name) {
            if instances.remove(&instance_name).is_some() {
                log::info!("Unregistered service instance: {}", instance_name);
                removed = true;
                if instances.is_empty() {
                    services_guard.remove(&base_service_name);
                    log::info!("Removed service type from registry as no instances remain: {}", base_service_name);
                }
            }
        }

        if !removed {
            log::warn!("Attempted to unregister a non-existent service instance: {}", instance_name);
        }
        Ok(())
    }

    /// Starts following a specific service type.
    /// Queries for this service type will be sent out on all active interfaces.
    ///
    /// # Arguments
    /// * `service_type_name` - The name of the service type to follow.
    ///
    /// # Returns
    /// A `Result` indicating success or an `Error`.
    pub async fn follow(&self, service_type_name: Name) -> Result<(), Error> {
        {
            let mut follow_guard = self.follow_services.write().await;
            if !follow_guard.insert(service_type_name.clone()) {
                log::debug!("Already following service type: {}", service_type_name);
                return Ok(());
            }
        }

        log::info!("Now following service type: {}", service_type_name);

        let mut query_message = Message::new();
        query_message.add_query(Query::query(service_type_name.clone(), RecordType::ANY));
        let bytes = query_message.to_bytes()?;

        let tasks_guard = self.tasks.read().await;
        if tasks_guard.is_empty() {
            log::warn!("No active mDNS tasks to send follow query for {}.", service_type_name);
        }
        for task in tasks_guard.iter() {
            if let Err(e) = send_to_mdns(&task.socket, &bytes).await {
                log::error!(
                    "Failed to send follow query for {} via {:?}: {}",
                    service_type_name, task.socket.local_addr().ok(), e
                );
            }
        }
        Ok(())
    }

    /// Stops following a specific service type.
    ///
    /// # Arguments
    /// * `service_type_name` - The name of the service type to unfollow.
    pub async fn unfollow(&self, service_type_name: Name) {
        let mut follow_guard = self.follow_services.write().await;
        if follow_guard.remove(&service_type_name) {
            log::info!("Stopped following service type: {}", service_type_name);
        } else {
            log::debug!("Attempted to unfollow a service type not being followed: {}", service_type_name);
        }
    }

    /// Finds discovered service instances that match a given predicate.
    ///
    /// # Arguments
    /// * `predicate` - A closure that takes a service instance name (`&Name`) and
    ///   `SocketAddr` (`&SocketAddr`) and returns `true` if the service matches.
    ///
    /// # Returns
    /// A `Vec` of `(Name, SocketAddr)` tuples for all matching services found in the cache.
    pub async fn find_service<P>(&self, predicate: P) -> Vec<(Name, SocketAddr)>
    where
        P: Fn(&Name, &SocketAddr) -> bool,
    {
        let cache_guard = self.service_cache.read().await;
        cache_guard
            .iter()
            .filter(|(name, addr)| predicate(name, addr))
            .map(|(name, addr)| (name.clone(), *addr))
            .collect()
    }

    /// Finds a single service instance by its exact name from the cache.
    ///
    /// # Arguments
    /// * `instance_name` - The exact instance name of the service to find.
    ///
    /// # Returns
    /// An `Option<(Name, SocketAddr)>` containing the service if found, otherwise `None`.
    pub async fn find_service_by_name(&self, instance_name: &Name) -> Option<(Name, SocketAddr)> {
        let cache_guard = self.service_cache.read().await;
        cache_guard.get(instance_name).map(|addr| (instance_name.clone(), *addr))
    }

    /// Clears the discovered services cache.
    pub async fn clear_cache(&self) {
        let mut cache_guard = self.service_cache.write().await;
        cache_guard.clear();
        log::info!("Cleared mDNS service cache.");
    }
}

impl Drop for Mdns {
    fn drop(&mut self) {
        if let Ok(mut tasks_guard) = self.tasks.try_write() {
            log::info!("Shutting down mDNS tasks...");
            for task in tasks_guard.iter_mut() {
                if !task.handle.is_finished() {
                    task.handle.abort();
                }
            }
            tasks_guard.clear();
            log::info!("All mDNS tasks signaled to abort.");
        } else {
            log::warn!("Could not acquire lock to shut down mDNS tasks in Drop. Tasks may not be cleanly terminated.");
        }
    }
}
