use std::time::Duration;

use tokio;
use vrchat_osc::{
    models::OscRootNode,
    rosc::{OscMessage, OscPacket},
    Error, ServiceType, VRChatOSC,
};

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    // Initialize VRChatOSC instance
    let vrchat_osc = VRChatOSC::new().await?;

    let cloned_vrchat_osc = vrchat_osc.clone();
    vrchat_osc
        .on_connect(move |res| match res {
            ServiceType::Osc(name, addr) => {
                log::info!("Connected to OSC server: {} at {}", name, addr);
                let vrchat_osc = cloned_vrchat_osc.clone();
                // Send a message to the OSC server
                tokio::spawn(async move {
                    vrchat_osc
                        .send_to_addr(
                            OscPacket::Message(OscMessage {
                                addr: "/avatar/parameters/VRChatOSC".to_string(),
                                args: vec![rosc::OscType::String("Connected".to_string())],
                            }),
                            addr,
                        )
                        .await
                        .unwrap();
                    log::info!("Sent message to OSC server.");
                });
            }
            ServiceType::OscQuery(name, addr) => {
                log::info!("Connected to OSCQuery server: {} at {}", name, addr);
                let vrchat_osc = cloned_vrchat_osc.clone();
                // Get parameters from the OSCQuery server
                tokio::spawn(async move {
                    const RETRY_COUNT: u8 = 30;
                    let mut counter = 0;
                    // Valid values may not be returned immediately after VRChat starts, as avatars might still be loading.
                    let params = loop {
                        if counter == RETRY_COUNT {
                            panic!("failed to get parameters after {} tries", RETRY_COUNT);
                        }
                        counter += 1;

                        match vrchat_osc
                            .get_parameter_from_addr("/avatar/parameters", addr)
                            .await
                        {
                            Ok(v) => break v,
                            Err(_) => {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                            }
                        }
                    };
                    log::info!("Received parameters: {:?}", params);
                });
            }
        })
        .await;

    // Register a test service
    let root_node = OscRootNode::new().with_avatar();
    vrchat_osc
        .register("test_service", root_node, |packet| {
            if let OscPacket::Message(msg) = packet {
                log::info!("Received OSC message: {:?}", msg);
            }
        })
        .await?;
    log::info!("Service registered.");

    // Wait for the service to be registered
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Send a test message to the registered service
    vrchat_osc
        .send(
            OscPacket::Message(OscMessage {
                addr: "/chatbox/input".to_string(),
                args: vec![
                    rosc::OscType::String("Hello, VRChat!".to_string()),
                    rosc::OscType::Bool(true),
                ],
            }),
            "VRChat-Client-*",
        )
        .await?;
    log::info!("Test message sent to VRChat-Client-*.");

    // Get parameters from the registered service
    let params = vrchat_osc
        .get_parameter("/avatar/parameters", "VRChat-Client-*")
        .await?;
    log::info!("Received parameters: {:?}", params);

    // Keep the program running to handle incoming messages
    log::info!("Press Ctrl+C to exit.");
    tokio::signal::ctrl_c().await?;

    // Shutdown the VRChatOSC instance
    vrchat_osc.shutdown().await?;
    Ok(())
}
