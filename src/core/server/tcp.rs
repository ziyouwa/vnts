use crate::core::service::PacketHandler;
use crate::protocol::NetPacket;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::select;
use tokio::sync::mpsc::{channel, Sender};
use tokio::sync::Notify;
use tokio::signal;

pub async fn start(tcp: TcpListener, handler: PacketHandler) -> io::Result<()> {
    let state = Arc::new((AtomicUsize::new(0), Notify::new()));

    loop {
        let (stream, addr) = tcp.accept().await?;
        let _ = stream.set_nodelay(true);
        stream_handle(stream, addr, handler.clone()).await;
    }
}

async fn stream_handle(stream: TcpStream, addr: SocketAddr, handler: PacketHandler) {
    let (r, mut w) = stream.into_split();

    let (sender, mut receiver) = channel::<Vec<u8>>(100);
    tokio::spawn(async move {
        while let Some(data) = receiver.recv().await {
            let len = data.len();
            if let Err(e) = w
                .write_all(&[0, 0, (len >> 8) as u8, (len & 0xFF) as u8])
                .await
            {
                log::info!("发送失败,链接终止:{:?},{:?}", addr, e);
                break;
            }
            if let Err(e) = w.write_all(&data).await {
                log::info!("发送失败,链接终止:{:?},{:?}", addr, e);
                break;
            }
        }
        let _ = w.shutdown().await;
    });
    tokio::spawn(async move {
        if let Err(e) = tcp_read(r, addr, sender, handler).await {
            log::warn!("tcp_read {:?}", e)
        }
    });
}

async fn tcp_read(
    mut read: OwnedReadHalf,
    addr: SocketAddr,
    sender: Sender<Vec<u8>>,
    handler: PacketHandler,
) -> io::Result<()> {
    let mut head = [0; 4];
    let mut buf = [0; 65536];
    let sender = Some(sender);
    loop {
        read.read_exact(&mut head).await?;
        let len = ((head[2] as usize) << 8) | head[3] as usize;
        if len < 12 || len > buf.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "length overflow",
            ));
        }
        read.read_exact(&mut buf[..len]).await?;
        let packet = NetPacket::new0(len, &mut buf)?;
        if let Some(rs) = handler.handle(packet, addr, &sender).await {
            if sender
                .as_ref()
                .unwrap()
                .send(rs.buffer().to_vec())
                .await
                .is_err()
            {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "send error"));
            }
        }
    }
}
