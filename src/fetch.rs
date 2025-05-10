use std::net::SocketAddr;

use http_body_util::{BodyExt, Empty};
use hyper::body::Buf;
use hyper_util::rt::TokioIo;
use tokio::net::{TcpStream, ToSocketAddrs};

/// Custom error type for fetch operations.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Hyper HTTP client/server error: {0}")]
    Hyper(#[from] hyper::Error),
    #[error("HTTP library error: {0}")]
    Http(#[from] axum::http::Error), // Errors from constructing HTTP requests/responses.
    #[error("Serde JSON deserialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Asynchronously fetches and deserializes JSON data from a given address and path.
///
/// # Type Parameters
/// * `T`: A type that can be resolved into one or more `SocketAddr`s.
/// * `F`: The type to deserialize the JSON response into. Must implement `serde::de::DeserializeOwned`.
///
/// # Arguments
/// * `addrs`: The address(es) to connect to (e.g., "localhost:8080" or a `SocketAddr`).
/// * `path`: The HTTP path to request (e.g., "/info").
///
/// # Returns
/// A `Result` containing a tuple of the deserialized data (`F`) and the `SocketAddr` it connected to,
/// or a `Error` if any step fails.
pub(crate) async fn fetch<T, F>(addrs: T, path: &str) -> Result<(F, SocketAddr), Error>
where
    T: ToSocketAddrs, // Ensures `addrs` can be converted to socket addresses.
    F: serde::de::DeserializeOwned, // Ensures `F` can be created from deserialized data.
{
    // Establish a TCP connection to the first resolved address.
    let stream = TcpStream::connect(addrs).await?;
    // Get the peer's socket address after successful connection.
    let connected_addr = stream.peer_addr()?;
    
    // Perform an HTTP/1 handshake over the TCP stream.
    // `TokioIo` adapts the Tokio `TcpStream` for Hyper.
    // `sender` is used to send requests, `conn` represents the connection itself.
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    
    // Spawn a background task to drive the connection.
    // This task polls the connection for events (like incoming data or connection close)
    // and ensures the HTTP protocol is correctly handled.
    // If this task is not spawned, the `sender.send_request` call might hang.
    tokio::spawn(async move {
        if let Err(err) = conn.await {
            // Log an error if the connection encounters an issue.
            log::error!("HTTP connection failed: {}", err);
        }
    });

    // Construct the full URL for the request.
    // Using `connected_addr` ensures we use the specific IP and port we connected to.
    let url = format!("http://{}{}", connected_addr, path);

    // Build the HTTP GET request.
    let req = hyper::Request::builder()
        .uri(url)
        .header(hyper::header::HOST, connected_addr.to_string())
        .body(Empty::<hyper::body::Bytes>::new())?;

    // Send the request using the sender obtained from the handshake.
    let res = sender.send_request(req).await?;
    
    // Collect the entire response body.
    // `res.collect()` consumes the body and returns a future that resolves to the aggregated body.
    let body_bytes = res.collect().await?.aggregate(); // Aggregate chunks into a single buffer.
    
    // Deserialize the response body from JSON.
    // `body.reader()` provides a `BufRead` interface over the buffered body data.
    let data: F = serde_json::from_reader(body_bytes.reader())?;
    
    Ok((data, connected_addr))
}
