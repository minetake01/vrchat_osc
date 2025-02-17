use oscquery::models::{OscRootNode, OscValue};
use rosc::OscPacket;
use vrchat_osc::VRChatOSC;

#[tokio::main]
async fn main() {
    let mut vrchat_osc = VRChatOSC::new().unwrap();

    println!("Registering service...");
    vrchat_osc.register_service("MyTool", OscRootNode::avatar_parameters(), |packet| {
        if let OscPacket::Message(msg) = packet {
            if msg.addr == "/avatar/parameters/MuteSelf" {
                println!("Received: {:?}", msg.args);
            }
        }
    }).await.unwrap();
    
    println!("Getting parameters...");
    let Ok(parameters) = vrchat_osc.get_parameter("/avatar/change").await else {
        println!("Please run this example while connected to VRChat.");
        return;
    };
    let OscValue::String(value) = parameters.value.clone().unwrap()[0].clone() else { return; };
    println!("Your current avatar is: {}", value);

    println!("Sending message...");
    let osc_packet = rosc::OscPacket::Message(rosc::OscMessage {
        addr: "/chatbox/input".to_string(),
        args: vec![
            rosc::OscType::String("Hello, world!".to_string()),
            rosc::OscType::Bool(true),
        ],
    });
    vrchat_osc.send(osc_packet).await.unwrap();

    vrchat_osc.join().await.unwrap();
}
