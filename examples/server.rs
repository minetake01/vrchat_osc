use oscquery::node::{AccessMode, HostInfo, OscNode, OscTypeTag, OscValue, RangeInfo};
use oscquery::server::OscQueryServer;
use std::collections::HashMap;
use std::sync::mpsc::channel;

#[tokio::main]
async fn main() {
    let mut server = OscQueryServer::new(HostInfo::default());

    // Add nodes to the server
    server
        .add_node(
            "/foo",
            OscNode::new("/foo")
                .with_type(OscTypeTag::from_tag("f").unwrap())
                .with_description("A float parameter")
                .with_access(AccessMode::ReadWrite)
                .with_value(vec![OscValue::Float(0.5)])
                .with_range(vec![RangeInfo {
                    min: Some(OscValue::Float(0.0)),
                    max: Some(OscValue::Float(1.0)),
                    vals: None,
                }]),
        )
        .await
        .unwrap();

    server
        .add_node(
            "/bar",
            OscNode::new("/bar")
                .with_type(OscTypeTag::from_tag("ii").unwrap())
                .with_description("Two integer parameters")
                .with_access(AccessMode::ReadWrite)
                .with_value(vec![OscValue::Int(0), OscValue::Int(100)])
                .with_range(vec![
                    RangeInfo {
                        min: Some(OscValue::Int(0)),
                        max: Some(OscValue::Int(100)),
                        vals: None,
                    },
                    RangeInfo {
                        min: Some(OscValue::Int(0)),
                        max: Some(OscValue::Int(1000)),
                        vals: None,
                    },
                ]),
        )
        .await
        .unwrap();

    // Add a container node
    let mut container_contents = HashMap::new();
    container_contents.insert(
        "child1".to_string(),
        OscNode::new("/container/child1")
            .with_type(OscTypeTag::from_tag("s").unwrap())
            .with_description("A string parameter")
            .with_access(AccessMode::ReadOnly)
            .with_value(vec![OscValue::String("Hello".to_string())]),
    );
    container_contents.insert(
        "child2".to_string(),
        OscNode::new("/container/child2")
            .with_type(OscTypeTag::from_tag("s").unwrap())
            .with_description("A boolean parameter")
            .with_access(AccessMode::ReadWrite)
            .with_value(vec![OscValue::Bool(true)]),
    );

    server
        .add_node(
            "/container",
            OscNode::new("/container")
                .with_description("A container node")
                .with_contents(container_contents),
        )
        .await
        .unwrap();

    // Start the server
    let (tx, rx) = channel();

    ctrlc::set_handler(move || {
        println!("Received Ctrl-C, shutting down");
        tx.send(()).unwrap()
    }).expect("Error setting Ctrl-C handler");

    let join = server.serve().await;
    while let None = rx.try_recv().ok() {
        println!("Updating /foo");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        server
            .map_node("/foo", |node| {
                node.set_value(vec![OscValue::Float(rand::random::<f64>())]);
            })
            .await;
    }
    server.shutdown();
    join.await.unwrap().unwrap();
}
