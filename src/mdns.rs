mod if_monitor;
mod task;
mod utils;

use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use hickory_proto::{
    op::{Message, Query},
    rr::{Name, RecordType},
    serialize::binary::BinEncodable,
};
use socket_pktinfo::AsyncPktInfoUdpSocket;
use task::server_task;
use tokio::{
    sync::{mpsc, RwLock},
    task::JoinHandle,
};
use utils::{send_mdns_announcement, send_to_mdns};

use crate::mdns::utils::setup_multicast_socket;

const MDNS_PORT: u16 = 5353;
const MDNS_IPV4_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_IPV6_ADDR: Ipv6Addr = Ipv6Addr::new(0xFF02, 0, 0, 0, 0, 0, 0, 0xFB);

/// Maximum number of attempts to send a multicast message.
const MAX_SEND_ATTEMPTS: usize = 3;

/// Error types that can occur during mDNS operations.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("DNS Protocol Error: {0}")]
    MdnsProtoError(#[from] hickory_proto::ProtoError),
    #[error("I/O Error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to bind any sockets for mDNS.")]
    AnySocketBindError,
}

/// Represents a task associated with a UDP socket for mDNS operations.
struct MdnsTask {
    socket: Arc<AsyncPktInfoUdpSocket>,
    handle: JoinHandle<()>,
}

/// Main structure for handling mDNS service discovery and advertisement
/// across all available network interfaces.
pub struct Mdns {
    /// List of active mDNS tasks, typically one for IPv4 and one for IPv6,
    /// each operating on all relevant interfaces.
    tasks: Vec<MdnsTask>,

    /// Network interface monitor to track interface changes.
    if_monitor: Arc<if_monitor::IfMonitor>,

    /// A map of registered services.
    /// The first key is the service type name (e.g., `_oscjson._tcp.local.`).
    /// The second key is the service instance name (e.g., `MyInstance._oscjson._tcp.local.`),
    /// mapped to its `SocketAddr`.
    registered_services: Arc<RwLock<HashMap<Name, HashMap<Name, u16>>>>,

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
    pub async fn new(notifier_tx: mpsc::Sender<(Name, SocketAddr)>) -> Result<Self, Error> {
        let registered_services = Arc::new(RwLock::new(HashMap::new()));
        let service_cache = Arc::new(RwLock::new(HashMap::new()));
        let follow_services = Arc::new(RwLock::new(HashSet::new()));
        let mut tasks = Vec::new();

        // Attempt to bind a UDP socket for multicast
        let if_monitor = if_monitor::IfMonitor::new()?;
        let if_addrs = if_monitor.get_interfaces().await;
        let sockets = setup_multicast_socket(if_addrs).await?;

        let if_monitor = Arc::new(if_monitor);

        let sockets_clone = sockets.clone();
        if_monitor.on_added(move |iface| match iface.ip() {
            IpAddr::V4(ip) => {
                if let Err(e) = sockets_clone[0].join_multicast_v4(&MDNS_IPV4_ADDR, &ip) {
                    log::warn!(
                        "Failed to join IPv4 mDNS multicast group {} on interface {}: {}",
                        MDNS_IPV4_ADDR,
                        ip,
                        e
                    );
                }
            }
            IpAddr::V6(_) => {
                let Some(if_index) = iface.index else {
                    return;
                };
                if let Err(e) = sockets_clone[1].join_multicast_v6(&MDNS_IPV6_ADDR, if_index) {
                    log::warn!(
                        "Failed to join IPv6 mDNS multicast group {} on interface index {}: {}",
                        MDNS_IPV6_ADDR,
                        if_index,
                        e
                    );
                }
            }
        });
        let sockets_clone = sockets.clone();
        if_monitor.on_removed(move |iface| match iface.ip() {
            IpAddr::V4(ip) => {
                if let Err(e) = sockets_clone[0].leave_multicast_v4(&MDNS_IPV4_ADDR, &ip) {
                    log::warn!(
                        "Failed to leave IPv4 mDNS multicast group {} on interface {}: {}",
                        MDNS_IPV4_ADDR,
                        ip,
                        e
                    );
                }
            }
            IpAddr::V6(_) => {
                let Some(if_index) = iface.index else {
                    return;
                };
                if let Err(e) = sockets_clone[1].leave_multicast_v6(&MDNS_IPV6_ADDR, if_index) {
                    log::warn!(
                        "Failed to leave IPv6 mDNS multicast group {} on interface index {}: {}",
                        MDNS_IPV6_ADDR,
                        if_index,
                        e
                    );
                }
            }
        });

        for socket in sockets {
            log::debug!(
                "Successfully bound to multicast socket: {:?}",
                socket.local_addr()
            );
            tasks.push(MdnsTask {
                socket: socket.clone(),
                handle: tokio::spawn(server_task(
                    socket,
                    notifier_tx.clone(),
                    registered_services.clone(),
                    service_cache.clone(),
                    follow_services.clone(),
                    if_monitor.clone(),
                )),
            });
        }

        if tasks.is_empty() {
            return Err(Error::AnySocketBindError);
        }

        Ok(Mdns {
            tasks,
            if_monitor,
            registered_services,
            service_cache,
            follow_services,
        })
    }

    /// Registers a service with mDNS.
    ///
    /// This adds the service to an internal registry and advertises it on the network
    /// across all active interfaces (via the wildcard-bound sockets). The service is identified
    /// by its instance name and its port number.
    ///
    /// # Arguments
    /// * `instance_name` - The unique name of the service instance (e.g., `VRChat-Client-1234._oscjson._tcp.local.`).
    /// * `port` - The port number where the service is hosted.
    ///
    /// # Returns
    /// A `Result` indicating success or an `Error` if registration fails.
    pub async fn register(&self, instance_name: Name, port: u16) -> Result<(), Error> {
        let base_service_name = instance_name.trim_to(3);

        {
            let mut services_guard = self.registered_services.write().await;
            let instances = services_guard.entry(base_service_name.clone()).or_default();
            instances.insert(instance_name.clone(), port);
        }

        log::info!("Registered service: {} at {}", instance_name, port);

        for _ in 0..MAX_SEND_ATTEMPTS {
            for task in &self.tasks {
                if let Err(e) =
                    send_mdns_announcement(&task.socket, &instance_name, port, &self.if_monitor)
                        .await
                {
                    log::error!(
                        "Failed to send registration announcement for {} via {:?}: {}",
                        instance_name,
                        task.socket.local_addr().ok(),
                        e
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
                    log::debug!(
                        "Removed service type from registry as no instances remain: {}",
                        base_service_name
                    );
                }
            }
        }

        if !removed {
            log::warn!(
                "Attempted to unregister a non-existent service instance: {}",
                instance_name
            );
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

        log::debug!("Now following service type: {}", service_type_name);

        let mut query_message = Message::new();
        query_message.add_query(Query::query(service_type_name.clone(), RecordType::ANY));
        let bytes = query_message.to_bytes()?;

        for task in self.tasks.iter() {
            if let Err(e) = send_to_mdns(&task.socket, &bytes, &self.if_monitor).await {
                log::error!(
                    "Failed to send follow query for {} via {:?}: {}",
                    service_type_name,
                    task.socket.local_addr().ok(),
                    e
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
            log::debug!("Stopped following service type: {}", service_type_name);
        } else {
            log::debug!(
                "Attempted to unfollow a service type not being followed: {}",
                service_type_name
            );
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
        cache_guard
            .get(instance_name)
            .map(|addr| (instance_name.clone(), *addr))
    }
}

impl Drop for Mdns {
    fn drop(&mut self) {
        for task in &mut self.tasks {
            task.handle.abort();
        }
        log::debug!("All mDNS tasks have been cleaned up.");
    }
}
