mod error;
mod tasks;

use std::{collections::HashMap, net::{Ipv4Addr, SocketAddr, SocketAddrV4}};

pub use oscquery;
pub use error::VRChatOSCError;

use convert_case::{Case, Casing};
use http_body_util::{BodyExt, Empty};
use hyper::body::{Buf, Bytes};
use hyper_util::rt::TokioIo;
use mdns_sd::{ServiceDaemon, ServiceInfo};
use oscquery::{models::{HostInfo, OscNode, OscRootNode}, OscQuery};
use rosc::OscPacket;
use tasks::resolver_task;
use tokio::{net::{TcpStream, ToSocketAddrs, UdpSocket}, sync::watch, task::JoinHandle};
use log::error;

#[derive(Debug, Clone)]
pub struct VRChatAddrs {
    pub osc: SocketAddr,
    pub osc_query: SocketAddr,
}

struct ServiceHandles {
    osc: JoinHandle<()>,
    osc_query: OscQuery,
    register_task_handle: JoinHandle<Result<(), VRChatOSCError>>,
}

pub struct VRChatOSC {
    mdns: ServiceDaemon,
    vrc_addrs: watch::Receiver<Option<VRChatAddrs>>,
    service_handles: HashMap<String, ServiceHandles>,
    resolution_task_handle: Option<JoinHandle<Result<(), VRChatOSCError>>>,
}

impl VRChatOSC {
    pub fn new() -> Result<VRChatOSC, VRChatOSCError> {
        let mdns = ServiceDaemon::new()?;
        let (tx, rx) = watch::channel(None);

        let resolution_task_handle = tokio::spawn(resolver_task(mdns.clone(), tx));
        
        Ok(VRChatOSC {
            mdns,
            vrc_addrs: rx,
            service_handles: HashMap::new(),
            resolution_task_handle: Some(resolution_task_handle),
        })
    }

    async fn addrs_resolution_wait(&mut self) -> Result<VRChatAddrs, VRChatOSCError> {
        self.vrc_addrs.wait_for(|addrs| addrs.is_some()).await?;
        Ok(self.vrc_addrs.borrow().as_ref().unwrap().clone())
    }
    
    pub async fn register_service<F>(&mut self, service_name: &str, parameters: OscRootNode, handler: F) -> Result<(), VRChatOSCError>
    where
        F: Fn(OscPacket) + Send + 'static,
    {
        // Start OSC server
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).await?;
        let osc_local_addr = socket.local_addr()?;
        let osc_handle = tokio::spawn(async move {
            let mut buf = [0; rosc::decoder::MTU];
            loop {
                let Ok(len) = socket.recv(&mut buf).await else { continue; };
                let Ok((_, packet)) = rosc::decoder::decode_udp(&buf[..len]) else { continue; };
                handler(packet);
            }
        });
        
        // Start OSCQuery server
        let host_info = HostInfo::new(
            service_name.to_string(),
            osc_local_addr.ip(),
            osc_local_addr.port(),
        );
        let mut osc_query = OscQuery::new(host_info, parameters);
        let osc_query_local_addr = osc_query.serve(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).await?;

        // Create mDNS services
        let service_name_upper_camel = service_name.to_case(Case::UpperCamel);
        let osc_service = ServiceInfo::new(
            "_osc._udp.local.",
            &service_name_upper_camel.clone(),
            &(service_name_upper_camel.clone() + ".osc.local."),
            osc_local_addr.ip(),
            osc_local_addr.port(),
            &[("txtvers", "1")][..],
        )?;
        let osc_query_service = ServiceInfo::new(
            "_oscjson._tcp.local.",
            &service_name_upper_camel.clone(),
            &(service_name_upper_camel + ".oscjson.local."),
            osc_query_local_addr.ip(),
            osc_query_local_addr.port(),
            &[("txtvers", "1")][..],
        )?;

        // Start registration task
        let mdns = self.mdns.clone();
        let mut vrc_addrs = self.vrc_addrs.clone();
        let register_task_handle = tokio::spawn(async move {
            loop {
                vrc_addrs.changed().await?;
                mdns.register(osc_service.clone())?;
                mdns.register(osc_query_service.clone())?;
            }
        });

        // Save service handles
        self.service_handles.insert(service_name.to_string(), ServiceHandles {
            osc: osc_handle,
            osc_query,
            register_task_handle,
        });
        Ok(())
    }

    pub fn unregister_service(&mut self, service_name: &str) -> Result<(), VRChatOSCError> {
        if let Some(mut service_handles) = self.service_handles.remove(service_name) {
            service_handles.register_task_handle.abort();
            self.mdns.unregister(&format!("{}._osc._udp.local.", service_name))?;
            self.mdns.unregister(&format!("{}._oscjson._tcp.local.", service_name))?;
            service_handles.osc.abort();
            service_handles.osc_query.shutdown();
        }
        Ok(())
    }

    pub async fn join(&mut self) -> Result<(), VRChatOSCError> {
        if let Some(handle) = self.resolution_task_handle.take() {
            handle.await??;
        }
        Ok(())
    }

    pub fn shutdown(&mut self) {
        for service_handles in self.service_handles.values_mut() {
            service_handles.osc.abort();
            service_handles.osc_query.shutdown();
            service_handles.register_task_handle.abort();
        }
        while let Err(err) = self.mdns.shutdown() {
            if let mdns_sd::Error::Again = err {
                continue;
            }
            error!("Failed to shutdown mdns daemon: {}", err);
            break;
        }
    }

    pub async fn send(&mut self, packet: OscPacket) -> Result<(), VRChatOSCError> {
        let msg_buf = rosc::encoder::encode(&packet)?;
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await?;

        let vrc_addrs = self.addrs_resolution_wait().await?;

        socket.send_to(&msg_buf, vrc_addrs.osc).await?;
        Ok(())
    }

    pub async fn get_parameter(&mut self, method: &str) -> Result<OscNode, VRChatOSCError> {
        let vrc_addrs = self.addrs_resolution_wait().await?;
        let (parameter, _) = fetch(vrc_addrs.osc_query, method).await?;
        Ok(parameter)
    }
}

impl Drop for VRChatOSC {
    fn drop(&mut self) {
        self.shutdown();
    }
}

async fn fetch<T, F>(addrs: T, path: &str) -> Result<(F, SocketAddr), VRChatOSCError>
where
    T: ToSocketAddrs,
    F: serde::de::DeserializeOwned,
{
    let stream = TcpStream::connect(addrs).await?;
    let connected_addr = stream.peer_addr()?;
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    tokio::spawn(async move {
        if let Err(err) = conn.await {
            error!("Failed to establish connection: {}", err);
        }
    });

    let url = format!("http://{}{}", connected_addr, path);

    let req = hyper::Request::builder()
        .uri(url)
        .header(hyper::header::HOST, connected_addr.to_string())
        .body(Empty::<Bytes>::new())?;
    let res = sender.send_request(req).await?;
    let body = res.collect().await?.aggregate();
    let data = serde_json::from_reader(body.reader())?;
    Ok((data, connected_addr))
}
