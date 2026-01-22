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
use utils::send_mdns_announcement;

use crate::mdns::utils::{get_interface_index, setup_multicast_socket};

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

/// Main structure for handling mDNS service discovery and advertisement
/// using a single advertised IP address.
pub struct Mdns {
    /// The UDP socket for mDNS operations, matching the advertised IP family.
    socket: Arc<AsyncPktInfoUdpSocket>,

    /// Handle to the background task processing mDNS messages.
    task_handle: JoinHandle<()>,

    /// The IP address to advertise in mDNS responses.
    /// Only the interface matching this IP will be used for announcements.
    advertised_ip: Arc<RwLock<IpAddr>>,

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
    /// Creates a new mDNS instance that operates using the specified advertised IP.
    ///
    /// This function binds a UDP socket to a wildcard address for the IP family
    /// matching the advertised IP. Announcements and responses are sent only via
    /// the interface matching the advertised IP.
    ///
    /// # Arguments
    /// * `notifier_tx` - An `mpsc::Sender` to send notifications of discovered services.
    /// * `advertised_ip` - The IP address to advertise in mDNS responses. This address serves as the
    ///   destination for OSC messages. Specifying this is necessary to use only the interface matching
    ///   this IP for service announcements and query responses, avoiding duplicate service discovery
    ///   issues in VRChat.
    ///
    /// # Returns
    /// A `Result` containing the new `Mdns` instance or an `Error` if initialization fails
    /// (e.g., if no sockets could be bound).
    pub async fn new(
        notifier_tx: mpsc::Sender<(Name, SocketAddr)>,
        advertised_ip_val: IpAddr,
    ) -> Result<Self, Error> {
        let advertised_ip = Arc::new(RwLock::new(advertised_ip_val));
        let registered_services = Arc::new(RwLock::new(HashMap::new()));
        let service_cache = Arc::new(RwLock::new(HashMap::new()));
        let follow_services = Arc::new(RwLock::new(HashSet::new()));

        // Get interface addresses for multicast group join
        let if_addrs = if_addrs::get_if_addrs()?;
        let socket = setup_multicast_socket(&if_addrs, advertised_ip_val).await?;

        log::debug!(
            "Successfully bound to multicast socket: {:?}",
            socket.local_addr()
        );

        let task_handle = tokio::spawn(server_task(
            socket.clone(),
            notifier_tx,
            registered_services.clone(),
            service_cache.clone(),
            follow_services.clone(),
            advertised_ip.clone(),
        ));

        Ok(Mdns {
            socket,
            task_handle,
            advertised_ip,
            registered_services,
            service_cache,
            follow_services,
        })
    }

    /// Sets the advertised IP address for mDNS responses.
    ///
    /// This changes the IP address that will be included in A/AAAA records
    /// when responding to mDNS queries. Also updates the multicast interface.
    ///
    /// # Arguments
    /// * `ip` - The new IP address to advertise.
    ///
    /// # Note
    /// The new IP must be of the same family (IPv4/IPv6) as the original.
    /// Changing between IPv4 and IPv6 is not supported.
    pub async fn set_advertised_ip(&self, ip: IpAddr) {
        // Update the multicast interface
        match ip {
            IpAddr::V4(ipv4) => {
                if let Err(e) = self.socket.set_multicast_if_v4(&ipv4) {
                    log::error!("Failed to set multicast IPv4 interface to {}: {}", ipv4, e);
                } else {
                    log::debug!("Set multicast IPv4 interface to {}", ipv4);
                }
            }
            IpAddr::V6(_) => {
                // Get current interface addresses for IPv6 interface lookup
                if let Ok(if_addrs) = if_addrs::get_if_addrs() {
                    let if_index = get_interface_index(&ip, &if_addrs).unwrap_or(0);
                    if let Err(e) = self.socket.set_multicast_if_v6(if_index) {
                        log::error!(
                            "Failed to set multicast IPv6 interface to index {}: {}",
                            if_index,
                            e
                        );
                    } else {
                        log::debug!("Set multicast IPv6 interface to index {}", if_index);
                    }
                } else {
                    log::error!("Failed to get interface addresses for IPv6 multicast setup");
                }
            }
        }

        let mut guard = self.advertised_ip.write().await;
        *guard = ip;
        log::info!("Updated advertised IP to: {}", ip);
    }

    /// Gets the currently advertised IP address.
    pub async fn get_advertised_ip(&self) -> IpAddr {
        *self.advertised_ip.read().await
    }

    /// Registers a service with mDNS.
    ///
    /// This adds the service to an internal registry and advertises it on the network
    /// using the configured advertised IP address. The service is identified
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

        // Clone necessary data before spawning to satisfy 'static lifetime requirement
        let socket = self.socket.clone();
        let advertised_ip = self.advertised_ip.clone();

        tokio::spawn(async move {
            for _ in 0..MAX_SEND_ATTEMPTS {
                if let Err(e) =
                    send_mdns_announcement(&socket, &instance_name, port, &advertised_ip).await
                {
                    log::error!(
                        "Failed to send registration announcement for {} via {:?}: {}",
                        instance_name,
                        socket.local_addr().ok(),
                        e
                    );
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
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
    /// Queries for this service type will be sent out on the active interface.
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

        // Determine multicast address based on advertised IP family
        let ip = *self.advertised_ip.read().await;
        let multicast_addr: SocketAddr = match ip {
            IpAddr::V4(_) => SocketAddr::new(IpAddr::V4(MDNS_IPV4_ADDR), MDNS_PORT),
            IpAddr::V6(_) => SocketAddr::new(IpAddr::V6(MDNS_IPV6_ADDR), MDNS_PORT),
        };

        if let Err(e) = self.socket.send_to(&bytes, multicast_addr).await {
            log::error!(
                "Failed to send follow query for {} via {:?}: {}",
                service_type_name,
                self.socket.local_addr().ok(),
                e
            );
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
        self.task_handle.abort();
        log::debug!("mDNS task has been cleaned up.");
    }
}
