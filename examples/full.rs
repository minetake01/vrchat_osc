use vrchat_osc::{models::OscRootNode, Error, ServiceType, VRChatOSC};
use rosc::{OscMessage, OscPacket};
use tokio;

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    // Initialize VRChatOSC instance
    let vrchat_osc = VRChatOSC::new().await?;

    let cloned_vrchat_osc = vrchat_osc.clone();
    vrchat_osc.on_connect(move |res| match res {
        ServiceType::Osc(name, addr) => {
            log::info!("Connected to OSC server: {} at {}", name, addr);
            let vrchat_osc = cloned_vrchat_osc.clone();
            tokio::spawn(async move {
                vrchat_osc.send_to_addr(
                    OscPacket::Message(OscMessage {
                        addr: "/avatar/parameters/VRChatOSC".to_string(),
                        args: vec![rosc::OscType::String("Connected".to_string())],
                    }),
                    addr,
                ).await.unwrap();
                log::info!("Sent message to OSC server.");
            });
        }
        ServiceType::OscQuery(name, addr) => {
            log::info!("Connected to OSCQuery server: {} at {}", name, addr);
            let vrchat_osc = cloned_vrchat_osc.clone();
            tokio::spawn(async move {
                let params = vrchat_osc.get_parameter_from_addr("/avatar/parameters", addr).await.unwrap();
                log::info!("Received parameters: {:?}", params);
            });
        }
    }).await;

    // Register a test service
    let root_node = OscRootNode::new().with_avatar();
    vrchat_osc.register("test_service", root_node, |packet| {
        if let OscPacket::Message(msg) = packet {
            log::info!("Received OSC message: {:?}", msg);
        }
    }).await?;
    log::info!("Service registered.");

    // Keep the program running to handle incoming messages
    log::info!("Press Ctrl+C to exit.");
    tokio::signal::ctrl_c().await?;

    vrchat_osc.shutdown().await?;
    Ok(())
}
