use thiserror::Error;

#[derive(Error, Debug)]
pub enum VRChatOSCError {
    #[error("{0}")]
    MdnsError(#[from] mdns_sd::Error),
    #[error("{0}")]
    OscError(#[from] rosc::OscError),
    #[error("{0}")]
    OscQueryError(#[from] oscquery::OscQueryError),
    #[error("{0}")]
    HyperError(#[from] hyper::Error),
    #[error("{0}")]
    HttpError(#[from] hyper::http::Error),
    #[error("{0}")]
    SerdeError(#[from] serde_json::Error),
    #[error("{0}")]
    IoError(#[from] std::io::Error),
    #[error("{0}")]
    RecvError(#[from] tokio::sync::watch::error::RecvError),
    #[error("{0}")]
    JoinError(#[from] tokio::task::JoinError),
}