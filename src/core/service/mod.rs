use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::UdpSocket;
use tokio::sync::mpsc::Sender;

use crate::cipher::RsaCipher;
use crate::config::ConfigInfo;
use crate::core::service::client::ClientPacketHandler;
use crate::core::service::server::ServerPacketHandler;
use crate::core::store::cache::AppCache;
use crate::{app_root, error::*};
use crate::protocol::NetPacket;

pub mod client;
pub mod server;

#[derive(Clone)]
pub struct PacketHandler {
    client: ClientPacketHandler,
    server: ServerPacketHandler,
}

impl PacketHandler {
    pub fn new(cache: AppCache, config: ConfigInfo, udp: Arc<UdpSocket>) -> Self {
        let rsa = match RsaCipher::new(app_root()) {
            Ok(rsa) => {
                println!("密钥指纹: {}", rsa.finger());
                Some(rsa)
            }
            Err(e) => {
                log::error!("获取密钥错误：{:?}", e);
                panic!("获取密钥错误:{}", e);
            }
        };

        let client = ClientPacketHandler::new(
            cache.clone(),
            config.clone(),
            rsa.clone(),
            udp.clone(),
        );
        let server =
            ServerPacketHandler::new(cache.clone(), config.clone(), rsa, udp);
        Self { client, server }
    }
}

impl PacketHandler {
    pub async fn handle<B: AsRef<[u8]> + AsMut<[u8]>>(
        &self,
        net_packet: NetPacket<B>,
        addr: SocketAddr,
        tcp_sender: &Option<Sender<Vec<u8>>>,
    ) -> Option<NetPacket<Vec<u8>>> {
        self.handle0(net_packet, addr, tcp_sender)
            .await
            .unwrap_or_else(|e| {
                log::error!("addr={},{:?}", addr, e);
                None
            })
    }
    async fn handle0<B: AsRef<[u8]> + AsMut<[u8]>>(
        &self,
        net_packet: NetPacket<B>,
        addr: SocketAddr,
        tcp_sender: &Option<Sender<Vec<u8>>>,
    ) -> Result<Option<NetPacket<Vec<u8>>>> {
        if net_packet.is_gateway() {
            self.server.handle(net_packet, addr, tcp_sender).await
        } else {
            self.client.handle(net_packet, addr)?;
            Ok(None)
        }
    }
}
