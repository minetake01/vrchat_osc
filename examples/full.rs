use vrchat_osc::{models::OscRootNode, VRChatOSC, Error};
use rosc::{OscMessage, OscPacket};
use tokio;

#[tokio::main]
async fn main() -> Result<(), Error> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .init();

    // Initialize VRChatOSC instance
    let mut vrchat_osc = VRChatOSC::new().await?;

    vrchat_osc.on_connect(|name, addr| {
        println!("Connected to VRChat OSC server: {:?} at {:?}", name, addr);
    }).await;

    // Register a test service
    let root_node = OscRootNode::new().with_avatar();
    vrchat_osc.register("test_service", root_node, |packet| {
        if let OscPacket::Message(msg) = packet {
            println!("Received OSC message: {:?}", msg);
        }
    }).await?;
    println!("Service registered.");

    // Send a test OSC message
    let msg = OscMessage {
        addr: "/avatar/parameters/TestParam".to_string(),
        args: vec![rosc::OscType::Float(1.0)],
    };
    vrchat_osc.send(OscPacket::Message(msg), "VRChat-Client-*").await?;
    println!("Test OSC message sent.");

    // Get current parameters
    let params = vrchat_osc.get_parameter("/avatar/parameters", "VRChat-Client-*").await?;
    for (name, node) in params {
        println!("Parameter: {:?}, Node: {:?}", name, node);
    }

    // Keep the program running to handle incoming messages
    println!("Press Ctrl+C to exit.");
    tokio::signal::ctrl_c().await?;

    Ok(())
}
