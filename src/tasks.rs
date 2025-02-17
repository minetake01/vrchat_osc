use std::net::SocketAddr;

use log::{error, info};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use oscquery::models::HostInfo;
use tokio::sync::watch;

use crate::{fetch, VRChatAddrs, VRChatOSCError};

pub(crate) async fn resolver_task(
    mdns: ServiceDaemon,
    tx: watch::Sender<Option<VRChatAddrs>>,
) -> Result<(), VRChatOSCError> {
    loop {
        // Browse for VRChat clients
        let receiver = match mdns.browse("_oscjson._tcp.local.") {
            Ok(receiver) => receiver,
            Err(mdns_sd::Error::Again) => {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                continue;
            }
            Err(err) => {
                break Err(VRChatOSCError::MdnsError(err));
            }
        };

        // Resolve VRChat clients
        'inner: loop{
            let Ok(event) = receiver.recv_async().await else { break 'inner; };

            match event {
                ServiceEvent::ServiceResolved(info) => {
                    if !info.get_fullname().starts_with("VRChat-Client-") {
                        continue;
                    }

                    let osc_query_addrs = info.get_addresses().iter().map(|ip| {
                        SocketAddr::new(*ip, info.get_port())
                    }).collect::<Vec<_>>();

                    // Fetch HOST_INFO
                    let (host_info, osc_query_addr) = match fetch::<_, HostInfo>(
                        osc_query_addrs.as_slice(),
                        "?HOST_INFO",
                    ).await {
                        Ok((host_info, osc_query_addr)) => (host_info, osc_query_addr),
                        Err(err) => {
                            error!("Failed to fetch host info: {}", err);
                            continue;
                        }
                    };
                    
                    info!("Found VRChat client: {}", host_info.name.clone());

                    let osc_addr = SocketAddr::new(
                        host_info.osc_ip,
                        host_info.osc_port,
                    );

                    tx.send(Some(VRChatAddrs {
                        osc: osc_addr,
                        osc_query: osc_query_addr,
                    })).ok();
                }
                _ => {}
            }
        };
    }
}
