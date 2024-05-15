use std::{io::Write, path::PathBuf};

use cipher::RsaCipher;
use config::ConfigInfo;

use std::net::{TcpListener, UdpSocket};

use anyhow::Result;

mod cipher;
mod config;
mod core;
mod error;
mod proto;
mod protocol;

fn log_init(root_path: PathBuf, log_path: &Option<String>) {
    let log_path = match log_path {
        None => root_path.join("log"),
        Some(log_path) => {
            if log_path == "/dev/null" {
                return;
            }
            PathBuf::from(log_path)
        }
    };
    if !log_path.exists() {
        let _ = std::fs::create_dir(&log_path);
    }

    let log_config = log_path.join("log4rs.yaml");
    if !log_config.exists() {
        if let Ok(mut f) = std::fs::File::create(&log_config) {
            let log_path = log_path.to_str().unwrap();
            let c = format!(
                "refresh_rate: 30 seconds
appenders:
  rolling_file:
    kind: rolling_file
    path: {}/vnts.log
    append: true
    encoder:
      pattern: \"{{d}} [{{f}}:{{L}}] {{h({{l}})}} {{M}}:{{m}}{{n}}\"
    policy:
      kind: compound
      trigger:
        kind: size
        limit: 10 mb
      roller:
        kind: fixed_window
        pattern: {}/vnts.{{}}.log
        base: 1
        count: 5

root:
  level: info
  appenders:
    - rolling_file",
                log_path, log_path
            );
            let _ = f.write_all(c.as_bytes());
        }
    }
    let _ = log4rs::init_file(log_config, Default::default());
}

pub fn app_root() -> PathBuf {
    match std::env::current_exe() {
        Ok(path) => {
            if let Some(v) = path.as_path().parent() {
                v.to_path_buf()
            } else {
                log::warn!("current_exe parent none:{:?}", path);
                PathBuf::new()
            }
        }
        Err(e) => {
            log::warn!("current_exe err:{:?}", e);
            PathBuf::new()
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();

    let config = ConfigInfo::new_with_options();
    log_init(app_root(), &config.log_path);
    log::info!("Config: {:?}", config.clone());

    let udp = create_udp(config.port)?;
    log::info!("监听udp端口: {:?}", config.port);

    let tcp = create_tcp(config.port)?;
    log::info!("监听tcp端口: {:?}", config.port);

    #[cfg(feature = "web")]
    let http = config.web_manager.as_ref().map(|web| {
        log::info!("监听http端口: {:?}", web.web_port);
        create_tcp(web.web_port).unwrap()
    });

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

    core::start(
        udp,
        tcp,
        #[cfg(feature = "web")]
        http,
        config,
        rsa,
    )
    .await
    .map_err(|e| anyhow::anyhow!(e))
}

fn create_tcp(port: u16) -> Result<TcpListener> {
    let address: std::net::SocketAddr = format!("[::]:{}", port)
        .parse()
        .map_err(|e| {
            log::error!(
                "端口错误，应在范围1025~65535内取值，您的 port={},e={}",
                port,
                e
            );
            format!(
                "端口错误，应在范围1025~65535内取值，您的 port={},e={}",
                port, e
            )
        })
        .map_err(|e| anyhow::anyhow!(e))?;

    let sock = socket2::Socket::new(socket2::Domain::IPV6, socket2::Type::STREAM, None)?;
    sock.set_reuse_address(true)?;
    sock.set_only_v6(false)?;
    sock.bind(&address.into())?;
    sock.listen(1024)?;
    Ok(sock.into())
}

fn create_udp(port: u16) -> Result<UdpSocket> {
    let address: std::net::SocketAddr = format!("[::]:{}", port)
        .parse()
        .map_err(|e| {
            log::error!(
                "端口错误，应在范围1025~65535内取值，您的 port={},e={}",
                port,
                e
            );
            format!(
                "端口错误，应在范围1025~65535内取值，您的 port={},e={}",
                port, e
            )
        })
        .map_err(|e| anyhow::anyhow!(e))?;

    let sock = socket2::Socket::new(socket2::Domain::IPV6, socket2::Type::DGRAM, None)?;
    sock.set_reuse_address(true)?;
    sock.set_only_v6(false)?;
    sock.set_nonblocking(true)?;
    sock.bind(&address.into())?;
    Ok(sock.into())
}
