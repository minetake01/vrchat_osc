use std::{
    collections::HashMap, 
    net::{IpAddr, Ipv4Addr, SocketAddrV4}, 
    sync::Arc, 
    time::Duration
};

use hickory_proto::{
    multicast::MDNS_IPV4, 
    op::{Message, MessageType, Query}, 
    rr::{rdata::{A, PTR, SRV}, Name, RData, Record, RecordType}, 
    serialize::binary::{BinDecodable, BinEncodable}
};
use tokio::{
    net::UdpSocket, 
    sync::{mpsc, RwLock}, 
    task::JoinHandle, 
    time::{timeout, Instant}
};

// Standard mDNS port and message buffer size
const MDNS_PORT: u16 = 5353;
const BUFFER_SIZE: usize = 4096; // Increased buffer size for larger mDNS messages
const DISCOVERY_TIMEOUT_MS: u64 = 1500; // Increased timeout for better discovery

/// Error types that can occur during mDNS operations
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Errors from the underlying DNS library
    #[error("DNS Protocol Error: {0}")]
    MdnsError(#[from] hickory_proto::ProtoError),
    
    /// I/O errors during network operations
    #[error("I/O Error: {0}")]
    IoError(#[from] std::io::Error),
    
    /// This error should theoretically never happen
    #[error("Invalid IP Version - Expected IPv4")]
    NeverError,
    
    /// Timeout error during service discovery
    #[error("Timeout while discovering services")]
    DiscoveryTimeout,
}

/// Main structure for handling mDNS service discovery and advertisement
pub struct Mdns {
    /// Handle to the background server task
    handle: Option<JoinHandle<()>>,
    
    /// UDP socket for multicast communication
    socket: Arc<UdpSocket>,
    
    /// Channel for sending discovered service information
    discover_tx: Arc<RwLock<Option<mpsc::Sender<(Name, SocketAddrV4)>>>>,
    
    /// A map of registered services.
    /// The first key is the service name (e.g., _oscjson._tcp.local.)
    /// The second key is the instance name (e.g., vrchat._oscjson._tcp.local.)
    registered_services: Arc<RwLock<HashMap<Name, HashMap<Name, SocketAddrV4>>>>,
    
    /// Cache of discovered services to reduce network traffic
    service_cache: Arc<RwLock<HashMap<Name, (HashMap<Name, SocketAddrV4>, Instant)>>>,
}

impl Mdns {
    /// Create a new mDNS instance.
    /// This function binds a UDP socket to the mDNS multicast address and port.
    pub async fn new() -> Result<Self, Error> {
        let registered_services = Arc::new(RwLock::new(HashMap::new()));
        let service_cache = Arc::new(RwLock::new(HashMap::new()));
        
        // Set up the UDP socket for multicast
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await?;
        
        // Ensure the multicast address is IPv4
        let IpAddr::V4(mdns_ip) = MDNS_IPV4.ip() else { 
            return Err(Error::NeverError); 
        };
        
        // Configure socket for multicast operation
        socket.join_multicast_v4(mdns_ip, Ipv4Addr::UNSPECIFIED)?;
        socket.set_multicast_loop_v4(true)?;
        
        // Set socket options for better performance
        socket.set_ttl(4)?; // Reasonable TTL for local network

        Ok(Mdns {
            handle: None,
            socket: Arc::new(socket),
            discover_tx: Default::default(),
            registered_services,
            service_cache,
        })
    }

    /// Start the mDNS server.
    /// This launches a background task that processes incoming mDNS messages.
    pub fn start(&mut self) -> Result<(), Error> {
        if self.handle.is_some() {
            // Server is already running
            return Ok(());
        }
        
        let discover_tx = self.discover_tx.clone();
        let registered_services = self.registered_services.clone();
        let service_cache = self.service_cache.clone();
        let socket = self.socket.clone();
        
        // Start the server task
        self.handle = Some(tokio::spawn(server_task(
            socket, 
            discover_tx, 
            registered_services,
            service_cache
        )));

        Ok(())
    }

    /// Register a service with mDNS.
    /// This adds the service to the internal registry and advertises it on the network.
    pub async fn register(&self, name: Name, addr: SocketAddrV4) -> Result<(), Error> {
        let mut registered_services = self.registered_services.write().await;
        
        // Get or create the map for this service type
        let base_name = name.base_name();
        let instances = registered_services.entry(base_name).or_insert_with(HashMap::new);
        
        // Add or update the instance
        instances.insert(name.clone(), addr);
        
        // Advertise service on the network
        let response = convert_to_message(name, addr);
        let bytes = response.to_bytes()?;
        for _ in 0..3 {
            // Send the response multiple times for reliability
            self.socket.send_to(&bytes, *MDNS_IPV4).await?;
        }
        
        Ok(())
    }

    /// Remove a registered service.
    /// This removes the service from the internal registry.
    pub async fn remove_service(&self, name: Name) -> Result<(), Error> {
        let mut registered_services = self.registered_services.write().await;
        
        // Remove the service instance from its base service type
        if let Some(map) = registered_services.get_mut(&name.base_name()) {
            map.remove(&name);
            
            // If this was the last instance, remove the service type entry
            if map.is_empty() {
                registered_services.remove(&name.base_name());
            }
        }

        Ok(())
    }

    /// Discover services using mDNS.
    /// This sends a query for services of the specified type and collects responses.
    pub async fn discover(&mut self, name: Name) -> Result<HashMap<Name, SocketAddrV4>, Error> {
        // Check cache first
        {
            let cache = self.service_cache.read().await;
            if let Some((services, timestamp)) = cache.get(&name) {
                // If cache is fresh (less than 30 seconds old)
                if timestamp.elapsed() < Duration::from_secs(30) {
                    return Ok(services.clone());
                }
            }
        }
        
        // Send a query for the service
        let mut message = Message::new();
        message.add_query(Query::query(name.clone(), RecordType::PTR));
        let bytes = message.to_bytes()?;
        self.socket.send_to(&bytes, *MDNS_IPV4).await?;

        // Create a channel to receive discovered services
        let (tx, mut rx) = mpsc::channel(10); // Increased channel capacity
        self.discover_tx.write().await.replace(tx);

        // Collect discovered services with timeout
        let mut services = HashMap::new();
        let discovery_start = Instant::now();
        
        while discovery_start.elapsed() < Duration::from_millis(DISCOVERY_TIMEOUT_MS) {
            let Ok(Some((name, addr))) = timeout(Duration::from_millis(100), rx.recv()).await else {
                break;  // Timeout waiting for responses
            };
            services.insert(name, addr);
        }

        // Clear the discover channel
        self.discover_tx.write().await.take();
        
        // Update cache
        if !services.is_empty() {
            let mut cache = self.service_cache.write().await;
            cache.insert(name, (services.clone(), Instant::now()));
        }

        Ok(services)
    }
    
    /// Stop the mDNS server.
    /// This aborts the background task if it's running.
    pub fn stop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

impl Drop for Mdns {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Background task to handle mDNS server operations.
async fn server_task(
    socket: Arc<UdpSocket>,
    discover_tx: Arc<RwLock<Option<mpsc::Sender<(Name, SocketAddrV4)>>>>,
    registered_services: Arc<RwLock<HashMap<Name, HashMap<Name, SocketAddrV4>>>>,
    service_cache: Arc<RwLock<HashMap<Name, (HashMap<Name, SocketAddrV4>, Instant)>>>,
) {
    let mut buf = [0; BUFFER_SIZE];
    
    loop {
        // Receive data from the socket
        let recv_result = socket.recv(&mut buf).await;
        let Ok(len) = recv_result else {
            log::error!("Failed to receive data on socket: {}", recv_result.unwrap_err());
            continue;
        };
        
        // Parse the received data into an mDNS message
        let parse_result = Message::from_bytes(&buf[..len]);
        let Ok(message) = parse_result else {
            log::error!("Failed to parse message from bytes: {}", parse_result.unwrap_err());
            continue;
        };
        
        // Process the message based on its type
        match message.message_type() {
            MessageType::Query => {
                handle_query(&message, &socket, &registered_services).await;
            },
            MessageType::Response => {
                handle_response(&message, &discover_tx, &service_cache).await;
            },
        }
    }
}

/// Handle incoming mDNS queries.
async fn handle_query(
    message: &Message,
    socket: &UdpSocket,
    registered_services: &Arc<RwLock<HashMap<Name, HashMap<Name, SocketAddrV4>>>>,
) {
    // Process each query in the message
    for query in message.queries() {
        log::debug!("Received query for service: {}", query.name());

        let registered_services = registered_services.read().await;
        if let Some(instances) = registered_services.get(&query.name()) {
            for (instance_name, addr) in instances {
                log::info!("Responding to query for service: {} at {}", instance_name, addr);

                // Create and send response
                let response = convert_to_message(instance_name.clone(), *addr);
                if let Ok(bytes) = response.to_bytes() {
                    if let Err(e) = socket.send_to(&bytes, *MDNS_IPV4).await {
                        log::error!("Failed to send response: {}", e);
                    }
                } else {
                    log::error!("Failed to serialize response");
                }
            }
        }
    }
}

/// Handle incoming mDNS responses.
async fn handle_response(
    message: &Message,
    discover_tx: &Arc<RwLock<Option<mpsc::Sender<(Name, SocketAddrV4)>>>>,
    service_cache: &Arc<RwLock<HashMap<Name, (HashMap<Name, SocketAddrV4>, Instant)>>>,
) {
    // Extract service information from the message
    if let Some((name, addr)) = extract_service_info(message) {
        log::info!("Discovered service: {} at {}", name, addr);

        // Update cache
        let service_type = name.base_name();
        {
            let mut cache = service_cache.write().await;
            if let Some((services, timestamp)) = cache.get_mut(&service_type) {
                services.insert(name.clone(), addr);
                *timestamp = Instant::now();
            }
        }

        // Notify listeners if there are any
        if let Some(tx) = discover_tx.read().await.as_ref() {
            if let Err(e) = tx.send((name, addr)).await {
                log::error!("Failed to send discovered service: {}", e);
            }
        }
    }
}

/// Extract service information from an mDNS message.
fn extract_service_info(message: &Message) -> Option<(Name, SocketAddrV4)> {
    let mut name = None;
    let mut ip = None;
    let mut port = None;

    // Check answer records first
    for answer in message.answers() {
        match answer.data() {
            RData::PTR(ptr) => {
                // PTR records point to service instances
                name = Some(ptr.0.clone());
            },
            RData::A(addr) => {
                // A records contain IPv4 addresses
                ip = Some(addr.0);
            },
            RData::SRV(srv) => {
                // SRV records contain service target and port
                name = Some(srv.target().clone());
                port = Some(srv.port());
            },
            _ => {}
        }
    }

    // Check additional records if needed
    if name.is_none() || ip.is_none() || port.is_none() {
        for additional in message.additionals() {
            match additional.data() {
                RData::A(addr) if ip.is_none() => {
                    ip = Some(addr.0);
                },
                RData::SRV(srv) if name.is_none() || port.is_none() => {
                    name = Some(srv.target().clone());
                    port = Some(srv.port());
                },
                _ => {}
            }
        }
    }

    // Construct socket address if all components are available
    if let (Some(name), Some(ip), Some(port)) = (name, ip, port) {
        Some((name, SocketAddrV4::new(ip, port)))
    } else {
        None
    }
}

/// Converts service information into an mDNS message.
fn convert_to_message(name: Name, addr: SocketAddrV4) -> Message {
    let mut message = Message::new();
    
    // Set message as a response
    message.set_message_type(MessageType::Response);
    
    // Add PTR record to the answer section
    message.add_answer(Record::from_rdata(
        name.base_name(),
        120, // TTL in seconds
        RData::PTR(PTR(name.clone()))
    ));
    
    // Add service records to additional section
    message.add_additionals([
        // A record with the IP address
        Record::from_rdata(
            name.clone(), 
            120, 
            RData::A(A(*addr.ip()))
        ),
        // SRV record with target and port
        Record::from_rdata(
            name.clone(), 
            120, 
            RData::SRV(SRV::new(0, 0, addr.port(), name.clone()))
        ),
    ]);
    
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_discover() {
        println!("This test assumes that VRChat with OSC enabled has been activated beforehand.");
        println!("If you are using a different application, please change the service name to match the one you are using.");

        let mut mdns = Mdns::new().await.unwrap();
        mdns.start().unwrap();

        let name = Name::from_ascii("_oscjson._tcp.local.").unwrap();
        let services = mdns.discover(name).await.unwrap();

        for (service_name, addr) in &services {
            println!("Discovered service: {} at {}", service_name, addr);
        }
        assert!(!services.is_empty(), "No services discovered");
    }

    #[tokio::test]
    async fn test_register() {
        // 1. サーバーの初期化
        let mut mdns = Mdns::new().await.unwrap();
        mdns.start().unwrap();
        
        // 2. テスト用のサービス名と情報を設定
        let service_type = Name::from_ascii("_test-service._tcp.local.").unwrap();
        let instance_name = Name::from_ascii("test-instance._test-service._tcp.local.").unwrap();
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 12345);
        
        // 3. サービス登録
        mdns.register(instance_name.clone(), addr).await.unwrap();
        
        // 登録の反映を少し待つ
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // 4. 登録されたことを確認
        let registered_services = mdns.registered_services.read().await;
        assert!(registered_services.contains_key(&service_type));
        
        let instances = registered_services.get(&service_type).unwrap();
        assert!(instances.contains_key(&instance_name));
        assert_eq!(instances.get(&instance_name).unwrap(), &addr);
    }
    
    #[tokio::test]
    async fn test_register_and_remove() {
        // 1. サーバーの初期化
        let mut mdns = Mdns::new().await.unwrap();
        mdns.start().unwrap();
        
        // 2. テスト用のサービス名と情報を設定
        let service_type = Name::from_ascii("_test-service._tcp.local.").unwrap();
        let instance_name = Name::from_ascii("test-instance._test-service._tcp.local.").unwrap();
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 12345);
        
        // 3. サービス登録
        mdns.register(instance_name.clone(), addr).await.unwrap();
        
        // 登録されたことを確認
        {
            let registered_services = mdns.registered_services.read().await;
            assert!(registered_services.contains_key(&service_type));
        }
        
        // 4. サービス削除
        mdns.remove_service(instance_name.clone()).await.unwrap();
        
        // 削除されたことを確認
        {
            let registered_services = mdns.registered_services.read().await;
            assert!(!registered_services.contains_key(&service_type));
        }
    }
    
    #[tokio::test]
    async fn test_register_multiple_instances() {
        // 1. サーバーの初期化
        let mut mdns = Mdns::new().await.unwrap();
        mdns.start().unwrap();
        
        // 2. テスト用のサービスタイプ
        let service_type = Name::from_ascii("_test-service._tcp.local.").unwrap();
        
        // 3. 複数インスタンスを登録
        let instance1 = Name::from_ascii("test-instance1._test-service._tcp.local.").unwrap();
        let addr1 = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 12345);
        mdns.register(instance1.clone(), addr1).await.unwrap();
        
        let instance2 = Name::from_ascii("test-instance2._test-service._tcp.local.").unwrap();
        let addr2 = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 12346);
        mdns.register(instance2.clone(), addr2).await.unwrap();
        
        // 登録されたことを確認
        let registered_services = mdns.registered_services.read().await;
        assert!(registered_services.contains_key(&service_type));
        
        let instances = registered_services.get(&service_type).unwrap();
        assert_eq!(instances.len(), 2);
        assert!(instances.contains_key(&instance1));
        assert!(instances.contains_key(&instance2));
    }
}
