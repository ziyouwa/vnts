use std::io;
use std::sync::Arc;

use tokio::net::{TcpListener, UdpSocket};

use crate::cipher::RsaCipher;
use crate::config::ConfigInfo;
use crate::core::service::PacketHandler;
use crate::core::store::cache::AppCache;

mod tcp;
mod udp;
#[cfg(feature = "web")]
mod web;

pub async fn start(
    udp: std::net::UdpSocket,
    tcp: std::net::TcpListener,
    #[cfg(feature = "web")] 
    http: Option<std::net::TcpListener>,
    config: ConfigInfo
) -> io::Result<()> {
    let udp = Arc::new(UdpSocket::from_std(udp)?);
    let cache = AppCache::new();
    
    let rsa = match RsaCipher::new() {
        Ok(rsa) => {
            println!("密钥指纹: {}", rsa.finger());
            Some(rsa)
        }
        Err(e) => {
            log::error!("获取密钥错误：{:?}", e);
            panic!("获取密钥错误:{}", e);
        }
    };

    let handler = PacketHandler::new(
        cache.clone(),
        config.clone(),
        rsa,
        udp.clone(),
    );
    
    let tcp_handle = tokio::spawn(tcp::start(TcpListener::from_std(tcp)?, handler.clone()));
    let udp_handle = tokio::spawn(udp::start(udp, handler.clone()));
    #[cfg(feature = "web")]
    if let Some(http) = http {
        if let Err(e) = web::start(http, cache, config).await {
            log::error!("{:?}", e);
        }
    }
    let _ = tokio::try_join!(tcp_handle, udp_handle);
    Ok(())
}
