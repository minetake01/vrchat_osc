use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use serde_json::json;

use crate::{node::{HostInfo, OscNode}, OscQuery};

#[tokio::test]
async fn test_serve() {
    let json = json!({
        "DESCRIPTION": "root node",
        "FULL_PATH": "/",
        "ACCESS": 0,
        "CONTENTS": {
            "foo": {
                "DESCRIPTION": "demonstrates a read-only OSC node- single float value",
                "FULL_PATH": "/foo",
                "ACCESS": 1,
                "TYPE": "f",
                "VALUE": [
                    0.5
                ]
            },
            "bar": {
                "DESCRIPTION": "demonstrates a read/write OSC node- two ints",
                "FULL_PATH": "/bar",
                "ACCESS": 3,
                "TYPE": "ii",
                "VALUE": [
                    4,
                    51
                ]
            },
            "baz": {
                "DESCRIPTION": "simple container node, with one method- qux",
                "FULL_PATH": "/baz",
                "ACCESS": 0,
                "CONTENTS": {
                    "qux":	{
                        "DESCRIPTION": "read/write OSC node- accepts one of several string-type inputs",
                        "FULL_PATH": "/baz/qux",
                        "ACCESS": 3,
                        "TYPE": "s",
                        "VALUE": [
                            "half-full"
                        ]
                    }
                }
            }
        }
    });
    let node: OscNode = serde_json::from_value(json).unwrap();
    
    let host_info = HostInfo {
        name: Some("oscquery test".to_string()),
        extensions: None,
        osc_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        osc_port: Some(9000),
        osc_transport: None,
        ws_ip: None,
        ws_port: None,
    };

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1234);
    let mut osc_query = OscQuery::new(host_info.clone(), node.clone());
    osc_query.serve(addr).await;

    let res_host_info = reqwest::get("http://localhost:1234/?HOST_INFO").await.unwrap().json::<HostInfo>().await.unwrap();
    assert_eq!(res_host_info.name.unwrap(), "oscquery test");

    let res_root = reqwest::get("http://localhost:1234").await.unwrap().json::<OscNode>().await.unwrap();
    assert_eq!(res_root.full_path, "/");

    let res_foo = reqwest::get("http://localhost:1234/foo").await.unwrap().json::<OscNode>().await.unwrap();
    assert_eq!(res_foo.full_path, "/foo");

    let res_foo_type = reqwest::get("http://localhost:1234/foo?TYPE").await.unwrap().text().await.unwrap();
    assert_eq!(res_foo_type, "{ TYPE: \"f\" }");

    osc_query.shutdown();
}
