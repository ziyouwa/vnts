use std::io;
use std::sync::Arc;

use tokio::net::{TcpListener, UdpSocket};

use crate::config::ConfigInfo;
use crate::core::service::PacketHandler;

mod tcp;
mod udp;

#[cfg(feature = "web")]
mod web;

pub async fn start(
    udp: std::net::UdpSocket,
    tcp: std::net::TcpListener,
    #[cfg(feature = "web")] http: Option<std::net::TcpListener>,
    config: &ConfigInfo,
) -> io::Result<()> {
    let udp = Arc::new(UdpSocket::from_std(udp)?);

    let handler = PacketHandler::new(config, udp.clone());
    let tcp_handle = tokio::spawn(tcp::start(TcpListener::from_std(tcp)?, handler.clone()));
    let udp_handle = tokio::spawn(udp::start(udp, handler.clone()));
    #[cfg(feature = "web")]
    if let Some(http) = http {
        if let Err(e) = web::start(http, config).await {
            log::error!("{:?}", e);
        }
    }
    let _ = tokio::try_join!(tcp_handle, udp_handle);
    Ok(())
}
