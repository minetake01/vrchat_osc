mod oscquery;
mod mdns;
mod fetch;

pub use oscquery::*;

use crate::fetch::fetch;

use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
};
use wildmatch::WildMatch;
use convert_case::{Case, Casing};
use futures::{stream, StreamExt};
use hickory_proto::rr::Name;
use oscquery::models::{HostInfo, OscNode, OscRootNode};
use rosc::OscPacket;
use tokio::{
    net::UdpSocket,
    sync::{mpsc, RwLock},
    task::JoinHandle,
};

/// Defines the possible errors that can occur within the VRChatOSC library.
#[derive(thiserror::Error, Debug)]
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
}

/// Holds handles related to a registered OSC service.
struct ServiceHandle {
    /// Join handle for the OSC listening task.
    osc: JoinHandle<()>,
    /// The OSCQuery server instance.
    osc_query: OscQuery,
}

/// Main struct for managing VRChat OSC services, discovery, and communication.
pub struct VRChatOSC {
    /// Arc-wrapped RwLock for thread-safe access to the mDNS client.
    mdns: Arc<RwLock<mdns::Mdns>>,
    /// Stores registered service handles, mapping service name to its handle.
    service_handles: HashMap<String, ServiceHandle>,
    /// Callback function to be executed when a new mDNS service is discovered.
    /// The Name is the service instance name, and SocketAddr is its resolved address.
    on_service_discovered_callback: Arc<RwLock<Option<Arc<dyn Fn(Name, SocketAddr) + Send + Sync + 'static>>>>,
}

impl VRChatOSC {
    /// Creates a new `VRChatOSC` instance.
    /// Initializes mDNS, sets up service discovery, and starts a listener task for mDNS service notifications.
    pub async fn new() -> Result<VRChatOSC, Error> {
        // Create an mpsc channel for notifying about discovered mDNS services.
        let (discover_notifier_tx, mut discover_notifier_rx) = mpsc::channel(8);
        
        // Initialize the mDNS client, passing the sender part of the notification channel.
        let mdns_client = mdns::Mdns::new(discover_notifier_tx).await?;
        
        // Start following OSC services and OSCQuery JSON services on the local network.
        let _ = mdns_client.follow(Name::from_ascii("_osc._udp.local.")?).await;
        let _ = mdns_client.follow(Name::from_ascii("_oscjson._tcp.local.")?).await;
        
        // Prepare a shared storage for the service discovered callback.
        let on_service_discovered_callback = Arc::new(RwLock::new(None::<Arc<dyn Fn(Name, SocketAddr) + Send + Sync + 'static>>));
        let callback_arc_clone = on_service_discovered_callback.clone();

        // Spawn a new asynchronous task to listen for service discovery notifications.
        // This task will own the `discover_notifier_rx` (receiver end of the mpsc channel).
        tokio::spawn(async move {
            // Continuously try to receive messages from the discovery notification channel.
            loop {
                if let Some((service_name, socket_addr)) = discover_notifier_rx.recv().await {
                    let callback_guard = callback_arc_clone.read().await;
                    // If a callback is registered, invoke it with the service name and address.
                    if let Some(callback) = callback_guard.as_ref() {
                        callback(service_name.clone(), socket_addr);
                    }
                }
            }
        });
        
        Ok(VRChatOSC {
            mdns: Arc::new(RwLock::new(mdns_client)),
            service_handles: Default::default(),
            on_service_discovered_callback,
        })
    }

    /// Registers a callback function to be invoked when an mDNS service is discovered.
    ///
    /// # Arguments
    /// * `callback` - A function or closure that takes the service `Name` and `SocketAddr`
    ///                as arguments. It must be `Send + Sync + 'static`.
    pub async fn on_connect<F>(&self, callback: F)
    where
        F: Fn(Name, SocketAddr) + Send + Sync + 'static,
    {
        let mut callback_guard = self.on_service_discovered_callback.write().await;
        *callback_guard = Some(Arc::new(callback));
    }
    
    /// Registers a new OSC service with the local mDNS daemon and starts listening for OSC messages.
    ///
    /// # Arguments
    /// * `service_name` - The name of the service to register (e.g., "MyAppOSC").
    /// * `parameters` - The root node of the OSC address space for this service.
    /// * `handler` - A function that will be called when an OSC packet is received for this service.
    ///               It must be `Fn(OscPacket) + Send + 'static`.
    pub async fn register<F>(&mut self, service_name: &str, parameters: OscRootNode, handler: F) -> Result<(), Error>
    where
        F: Fn(OscPacket) + Send + 'static,
    {
        // Start OSC server (UDP listener)
        // Bind to localhost on an OS-assigned port.
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).await?;
        let osc_local_addr = socket.local_addr()?; // Get the actual address it bound to.
        
        // Spawn a task to handle incoming OSC packets.
        let osc_handle = tokio::spawn(async move {
            let mut buf = [0; rosc::decoder::MTU]; // Buffer for receiving OSC packets.
            loop {
                // Wait to receive data on the socket.
                match socket.recv_from(&mut buf).await {
                    Ok((len, addr)) => {
                        // Decode the received UDP data into an OSC packet.
                        if let Ok((_, packet)) = rosc::decoder::decode_udp(&buf[..len]) {
                            handler(packet); // Call the provided handler with the decoded packet.
                        } else {
                            log::warn!("Failed to decode OSC packet from {}", addr);
                        }
                    }
                    Err(e) => {
                        if e.kind() == std::io::ErrorKind::ConnectionReset || e.kind() == std::io::ErrorKind::BrokenPipe {
                            log::warn!("Socket connection error ({}). Task for {:?} might need to be restarted or interface is down.", e, socket.local_addr().ok());
                            break;
                        } else {
                            log::error!("Failed to receive data on mDNS socket {:?}: {}", socket.local_addr().ok(), e);
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        continue;
                    }
                }
            }
        });
        
        // Start OSCQuery server (HTTP server)
        let host_info = HostInfo::new(
            service_name.to_string(),
            osc_local_addr.ip(), // Use the IP of the OSC server.
            osc_local_addr.port(), // Use the port of the OSC server.
        );
        let mut osc_query = OscQuery::new(host_info, parameters);
        // Serve OSCQuery on localhost with an OS-assigned port.
        let osc_query_local_addr = osc_query.serve(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).await?;

        // Create mDNS service announcements.
        // Lock the mDNS client for reading (as register might internally need to write, but here we only read its Arc).
        let mdns_guard = self.mdns.read().await;
        let service_name_upper_camel = service_name.to_case(Case::UpperCamel); // Convert service name case.
        
        // Register the OSC and OSCQuery services with mDNS.
        mdns_guard.register(
            Name::from_ascii(format!("{}._osc._udp.local.", service_name_upper_camel))?,
            osc_local_addr,
        ).await?;
        mdns_guard.register(
            Name::from_ascii(format!("{}._oscjson._tcp.local.", service_name_upper_camel))?,
            osc_query_local_addr,
        ).await?;
        // Drop the read guard explicitly to release the lock sooner.
        drop(mdns_guard);

        // Save service handles for later management (e.g., unregistering).
        self.service_handles.insert(service_name.to_string(), ServiceHandle {
            osc: osc_handle,
            osc_query,
        });
        Ok(())
    }

    /// Unregisters an OSC service.
    /// Stops the OSC and OSCQuery servers and removes mDNS announcements.
    ///
    /// # Arguments
    /// * `service_name` - The name of the service to unregister.
    pub async fn unregister(&mut self, service_name: &str) -> Result<(), Error> {
        let service_name_upper_camel = service_name.to_case(Case::UpperCamel);
        // Remove the service from our tracking.
        if let Some(mut service_handles) = self.service_handles.remove(service_name) {
            let mdns_guard = self.mdns.read().await;
            // Unregister from mDNS.
            mdns_guard.unregister(Name::from_ascii(format!("{}._osc._udp.local.", service_name_upper_camel))?).await?;
            mdns_guard.unregister(Name::from_ascii(format!("{}._oscjson._tcp.local.", service_name_upper_camel))?).await?;
            drop(mdns_guard);
            
            // Stop the associated tasks/servers.
            service_handles.osc.abort(); // Abort the OSC listening task.
            service_handles.osc_query.shutdown(); // Gracefully shutdown the OSCQuery server.
        }
        Ok(())
    }

    /// Sends an OSC packet to services matching a given pattern.
    ///
    /// # Arguments
    /// * `packet` - The `OscPacket` to send.
    /// * `to` - A glob pattern (e.g., "VRChat-Client-*") to match against service names.
    ///          This matches against the service instance name found via mDNS.
    pub async fn send(&self, packet: OscPacket, to: &str) -> Result<(), Error> {
        let mdns_guard = self.mdns.read().await;
        // Find services matching the pattern. The matching logic is within `find_service`.
        // The closure provided to `find_service` determines if a service (by its Name) matches.
        let services = mdns_guard.find_service(|name, _| {
            // `WildMatch` performs glob-style pattern matching.
            WildMatch::new(to).matches(&name.to_ascii())
        }).await;
        drop(mdns_guard);

        if services.is_empty() {
            log::warn!("No mDNS services found matching the expression: {}", to);
            return Ok(());
        }
        
        // Bind a UDP socket to send from. Using an unspecified port (0).
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await?;

        // Encode the OSC packet into bytes.
        let msg_buf = rosc::encoder::encode(&packet)?;
        // Send the packet to all found services.
        for (_, addr) in services {
            socket.send_to(&msg_buf, addr).await?;
        }

        Ok(())
    }

    /// Retrieves a specific OSC parameter (node) from services matching a pattern.
    ///
    /// # Arguments
    /// * `method` - The OSC path of the parameter to fetch (e.g., "/avatar/parameters/SomeParam").
    /// * `to` - A glob pattern (e.g., "VRChat-Client-*") to match against service names.
    ///          This matches against the service instance name found via mDNS.
    ///
    /// # Returns
    /// A `Vec` of tuples, where each tuple contains the service `Name` and the fetched `OscNode`.
    /// Returns an empty Vec if no services match or if fetching fails for all matched services.
    pub async fn get_parameter(&self, method: &str, to: &str) -> Result<Vec<(Name, OscNode)>, Error> {
        let mdns_guard = self.mdns.read().await;
        // Find services matching the pattern. The matching logic is within `find_service`.
        // The closure provided to `find_service` determines if a service (by its Name) matches.
        let services = mdns_guard.find_service(|name, _| {
            WildMatch::new(to).matches(&name.to_ascii())
        }).await;
        drop(mdns_guard);

        if services.is_empty() {
            log::warn!("No mDNS services found for get_parameter matching expression: {}", to);
            return Ok(Vec::new());
        }

        // Asynchronously fetch the parameter from all matching services.
        // `stream::iter` creates a stream from the services.
        // `map` transforms each service into a future that fetches the parameter.
        // `buffer_unordered(3)` allows up to 3 fetches to run concurrently.
        // `filter_map` discards any fetches that resulted in an error.
        // `collect` gathers all successful results into a Vec.
        let params = stream::iter(services)
            .map(|(name, addr)| async move {
                fetch::<_, OscNode>(addr, method).await.map(|(param, _)| (name.clone(), param))
            })
            .buffer_unordered(3)
            .filter_map(|res| async {
                if let Err(e) = &res {
                    log::warn!("Failed to fetch parameter: {:?}", e);
                }
                res.ok()
            })
            .collect::<Vec<_>>()
            .await;

        Ok(params)
    }
}

impl Drop for VRChatOSC {
    fn drop(&mut self) {
        // Perform synchronous cleanup
        let service_names: Vec<String> = self.service_handles.keys().cloned().collect();
        for name in service_names {
            if let Some(mut service_handles) = self.service_handles.remove(&name) {
                service_handles.osc.abort();
                service_handles.osc_query.shutdown();
            }
        }
    }
}
