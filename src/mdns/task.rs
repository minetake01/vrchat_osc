use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc,
};

use hickory_proto::{
    op::{Message, MessageType, ResponseCode},
    rr::{Name, RecordType},
    serialize::binary::{BinDecodable, BinEncodable},
};
use tokio::{
    net::UdpSocket,
    sync::{mpsc, RwLock},
};

use super::utils::{create_mdns_response_message, extract_service_info, send_to_mdns};

/// Size of the buffer used for receiving UDP packets. 4KB is a common size.
const BUFFER_SIZE: usize = 4096;

/// Background Tokio task to handle incoming mDNS messages (queries and responses) on a single UDP socket.
///
/// This task continuously listens for mDNS packets on the provided `socket`.
/// When a packet is received, it's parsed into an mDNS `Message`.
/// - If it's a query, `handle_query` is called.
/// - If it's a response, `handle_response` is called.
///
/// # Arguments
/// * `socket` - An `Arc<UdpSocket>` for mDNS communication. This socket should already be
///   configured for multicast.
/// * `notifier_tx` - An `mpsc::Sender` to notify about discovered services.
/// * `registered_services` - An `Arc<RwLock<...>>` providing access to services registered
///   by this mDNS instance. Used to respond to queries.
/// * `service_cache` - An `Arc<RwLock<...>>` for storing and updating information about
///   discovered services from responses.
/// * `follow_services` - An `Arc<RwLock<...>>` containing the set of service types this
///   instance is actively interested in.
pub async fn server_task(
    socket: Arc<UdpSocket>,
    notifier_tx: mpsc::Sender<(Name, SocketAddr)>,
    registered_services: Arc<RwLock<HashMap<Name, HashMap<Name, SocketAddr>>>>,
    service_cache: Arc<RwLock<HashMap<Name, SocketAddr>>>,
    follow_services: Arc<RwLock<HashSet<Name>>>,
) {
    let mut buf = [0u8; BUFFER_SIZE]; // Initialize buffer

    loop {
        // Attempt to receive data from the UDP socket.
        let len = match socket.recv(&mut buf).await {
            Ok(len) => len,
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
        };

        // If no data is received, continue the loop.
        if len == 0 {
            continue;
        }

        // Attempt to parse the received bytes into an mDNS message.
        let message = match Message::from_bytes(&buf[..len]) {
            Ok(msg) => msg,
            Err(e) => {
                log::warn!("Failed to parse mDNS message from bytes ({} bytes received): {}. Source unknown without recv_from.", len, e);
                continue;
            }
        };

        // Ignore messages with error response codes.
        if message.response_code() != ResponseCode::NoError {
            log::debug!(
                "Ignoring mDNS message with response code: {:?}",
                message.response_code()
            );
            continue;
        }
        
        // Process the message based on whether it's a query or a response.
        match message.message_type() {
            MessageType::Query => {
                // Handle incoming mDNS queries.
                handle_query(
                    message,
                    socket.clone(),
                    &registered_services,
                )
                .await;
            }
            MessageType::Response => {
                // Handle incoming mDNS responses.
                handle_response(
                    message,
                    &notifier_tx,
                    &registered_services,
                    &service_cache,
                    &follow_services,
                )
                .await;
            }
        }
    }
}

/// Handles an incoming mDNS query message.
///
/// It iterates through the questions in the query. If any question matches a service
/// registered by this mDNS instance, a response is formulated and sent back via the
/// provided `socket`.
///
/// # Arguments
/// * `query_message` - The parsed `Message` object representing the mDNS query.
/// * `socket` - The `Arc<UdpSocket>` to send responses from.
/// * `registered_services` - Read-only access to the map of locally registered services.
async fn handle_query(
    query_message: Message,
    socket: Arc<UdpSocket>,
    registered_services: &Arc<RwLock<HashMap<Name, HashMap<Name, SocketAddr>>>>,
) {
    // Iterate over each query in the mDNS message.
    for query in query_message.queries() {
        log::debug!(
            "Received mDNS query for service name: {}, type: {:?}, class: {:?}",
            query.name(),
            query.query_type(),
            query.query_class()
        );

        // The query name could be a service type (e.g., "_http._tcp.local")
        // or a specific instance name (e.g., "MyWebServer._http._tcp.local").
        let query_name_str = query.name().to_utf8(); // For logging

        let services_guard = registered_services.read().await;

        // Case 1: Query is for a service type (e.g., PTR query for "_http._tcp.local.")
        // We should respond with PTR records for all instances of that service type.
        // The `query.name()` here is the service type.
        if query.query_type() == RecordType::PTR || query.query_type() == RecordType::ANY {
            // `query.name()` is the service type, e.g. `_oscjson._tcp.local.`
            if let Some(instances_map) = services_guard.get(query.name()) {
                for (instance_name, &addr) in instances_map.iter() {
                    log::info!(
                        "Responding to PTR/ANY query for service type {} with instance: {} at {}",
                        query_name_str,
                        instance_name,
                        addr
                    );
                    let response = create_mdns_response_message(instance_name, addr);
                    match response.to_bytes() {
                        Ok(bytes) => {
                            if let Err(e) = send_to_mdns(&socket, &bytes).await {
                                log::error!("Failed to send response for instance {}: {}", instance_name, e);
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to serialize response for instance {}: {}", instance_name, e);
                        }
                    }
                }
            }
        }

        // Case 2: Query is for a specific service instance name (e.g., ANY/SRV/A query for "MyInstance._http._tcp.local.")
        // We need to find the service type part of the instance name to look up in `registered_services`.
        // The `query.name()` here is the instance name.
        // Example: query_name = "Instance.Type.Proto.Domain."
        // We need "Type.Proto.Domain." as the key for `services_guard`.
        if query.name().num_labels() > 3 {
            let service_type_key = query.name().trim_to(3); // e.g. _oscjson._tcp.local.

            if let Some(instances_map) = services_guard.get(&service_type_key) {
                // Now check if the specific instance `query.name()` is in this map.
                if let Some(&addr) = instances_map.get(query.name()) {
                     log::info!(
                        "Responding to specific query for registered service instance: {} at {}",
                        query_name_str,
                        addr
                    );
                    let response = create_mdns_response_message(query.name(), addr); // Use query.name() as it's the instance name
                    match response.to_bytes() {
                        Ok(bytes) => {
                            if let Err(e) = send_to_mdns(&socket, &bytes).await {
                                log::error!("Failed to send response for instance {}: {}", query.name(), e);
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to serialize response for instance {}: {}", query.name(), e);
                        }
                    }
                }
            }
        }
    }
}

/// Handles an incoming mDNS response message.
///
/// It extracts service information (instance name, address, port) from PTR, SRV,
/// and A/AAAA records in the response. If the service is one that this instance
/// is `follow`ing, the information is added to the `service_cache` and a
/// notification is sent via `notifier_tx`.
///
/// # Arguments
/// * `response_message` - The parsed `Message` object representing the mDNS response.
/// * `notifier_tx` - The sender channel to notify about newly discovered/updated services.
/// * `service_cache` - Writable access to the cache of discovered services.
/// * `follow_services` - Read-only access to the set of service types being followed.
async fn handle_response(
    response_message: Message,
    notifier_tx: &mpsc::Sender<(Name, SocketAddr)>,
    registered_services: &Arc<RwLock<HashMap<Name, HashMap<Name, SocketAddr>>>>,
    service_cache: &Arc<RwLock<HashMap<Name, SocketAddr>>>,
    follow_services: &Arc<RwLock<HashSet<Name>>>,
) {
    // Extract service information (name, IP, port) from the message.
    // `extract_service_info` is expected to look at PTR, SRV, A/AAAA records.
    // The custom packet format might mean `extract_service_info` needs to be robust.
    if let Some((discovered_instance_name, discovered_addr)) = extract_service_info(&response_message) {
        log::debug!(
            "Potential service discovered in response: {} at {}",
            discovered_instance_name,
            discovered_addr
        );

        // Check if we are following the service type of the discovered instance.
        // The `discovered_instance_name` is like "MyInstance._type._proto.local."
        // We need to get the service type part, e.g., "_type._proto.local."
        let service_type_name = discovered_instance_name.trim_to(3);

        let is_following: bool = {
            let follow_guard = follow_services.read().await;
            let registered_services_guard = registered_services.read().await;
            follow_guard.contains(&service_type_name)
                && !registered_services_guard.get(&service_type_name).map_or(false, |instances| {
                    instances.contains_key(&discovered_instance_name)   // Check if the instance is owned by us
                })
        };

        if is_following {
            // Update the service cache.
            let mut cache_guard = service_cache.write().await;
            // Insert or update the entry. `insert` returns the old value if any.
            let old_value = cache_guard.insert(discovered_instance_name.clone(), discovered_addr);

            if old_value.map_or(true, |old_addr| old_addr != discovered_addr) {
                // If it's a new service or its address changed.
                log::info!(
                    "Service cache updated for: {} at {} (was {:?})",
                    discovered_instance_name, discovered_addr, old_value
                );

                // If the service was newly added or updated, and we are following its type, notify listeners.
                if let Err(e) = notifier_tx.send((discovered_instance_name.clone(), discovered_addr)).await {
                    log::error!(
                        "Failed to send notification for discovered service {}: {}",
                        discovered_instance_name, e
                    );
                } else {
                    log::debug!("Sent notification for service: {}", discovered_instance_name);
                }
            } else {
                log::debug!(
                    "Service cache already up-to-date for: {} at {}",
                    discovered_instance_name, discovered_addr
                );
            }
        } else {
            log::trace!(
                "Ignoring discovered service {} of type {} because not followed.",
                discovered_instance_name, service_type_name
            );
        }
    } else {
        // This means the response message did not contain the expected set of records
        // to form a complete service announcement (PTR, SRV, A/AAAA). This is normal for
        // other types of mDNS responses (e.g., goodbye packets, or answers to specific non-service queries).
        log::trace!("Response message did not contain complete service info or was not a service announcement.");
    }
}
