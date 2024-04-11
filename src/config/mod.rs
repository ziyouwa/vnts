use std::{collections::HashSet, net::Ipv4Addr};

use clap::Parser;

#[derive(Debug, Clone, clap::Parser)]
#[command(author, version, about = "虚拟网络工具(Virtual Network Tool),简便高效的异地组网、内网穿透工具", long_about = None)]
#[command(help_template = "\
{before-help}{about-with-newline}
用法: vntserver [选项]

{all-args}{after-help}\
")]
#[clap(next_help_heading = "选项")]
#[command(disable_help_flag(true))]
#[command(disable_version_flag(true))]
pub struct Options {
    /// token白名单，例如 --white-token 1234 --white-token 123
    #[arg(short, long)]
    pub white_token: Option<Vec<String>>,

    /// 指定端口，默认值29872
    #[arg(short, long)]
    pub port: Option<u16>,

    /// 等待处理的请求队列长度, 默认值256
    #[arg(short, long)]
    pub backlog: Option<u16>,

    /// 网关，默认 --gateway 10.26.0.1
    #[arg(short, long)]
    pub gateway: Option<Ipv4Addr>,
    /// 子网掩码，默认 --netmask 255.255.255.0
    #[arg(short = 'm', long)]
    pub netmask: Option<Ipv4Addr>,

    ///开启后，仅指纹校验正确的客户端数据包才被转发，安全性增强，性能有一定下降
    #[arg(short, long, default_value_t = false)]
    pub finger: bool,

    /// log路径，默认为当前程序路径，为/dev/null时表示不输出log
    #[arg(short, long, default_value = "./log")]
    pub log_path: Option<String>,

    #[cfg(feature = "web")]
    #[command(flatten)]
    pub web_manager: Option<WebManager>,
    
    /// 显示此帮助信息并退出
    #[arg(action = clap::ArgAction::Help, short, long)]
    // #[arg(help = "打印帮助信息")]
    pub(crate) help: Option<bool>,

    /// 显示版本信息并退出
    #[arg(action = clap::ArgAction::Version, short, long)]
    // #[arg(help = "打印版本信息")]
    pub(crate) version: Option<bool>,
}

#[derive(Debug, Clone, clap::Parser)]
#[cfg(feature = "web")]
pub struct WebManager {
    /// web后台用户名
    #[arg(short = 'U', long, default_value = "admin")]
    pub username: String,
    /// web后台用户密码
    #[arg(short = 'W', long, default_value = "admin")]
    pub password: String,
    ///web后台端口，如果设置为0则表示不启动web后台
    #[arg(short = 'P', long, default_value = "29870")]
    pub web_port: u16,
}

#[derive(Debug, Clone)]
pub struct ConfigInfo {
    pub port: u16,
    pub backlog: u16,
    pub white_token: Option<HashSet<String>>,
    pub gateway: Ipv4Addr,
    pub broadcast: Ipv4Addr,
    pub netmask: Ipv4Addr,
    pub finger: bool,
    pub log_path: Option<String>,
    #[cfg(feature = "web")]
    pub web_manager: Option<WebManager>,
}

impl ConfigInfo {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }
    pub fn new_with_options() -> Self {
        let base = Self::default();
        let args = Options::parse();

        let gateway = if let Some(gateway) = args.gateway {
            if gateway.is_broadcast() || gateway.is_unspecified() || gateway.is_multicast() {
                log::error!(
                    "网关地址: {:?} 错误，不能为下列情况之一：广播地址、组播地址、无效地址",
                    gateway
                );
                panic!(
                    "网关地址: {gateway:?} 错误，不能为下列情况之一：广播地址、组播地址、无效地址"
                );
            }
            if !gateway.is_private() {
                log::warn!(
                    "Warning: {:?}不是一个私有地址，有可能和公网 ip 冲突",
                    gateway
                );
            }
            gateway
        } else {
            base.gateway
        };

        let netmask = if let Some(netmask) = args.netmask {
            if !is_valid_netmask(netmask) {
                log::error!("子网掩码: {:?} 错误", netmask);
                panic!("子网掩码: {:?} 错误", netmask);
            }
            netmask
        } else {
            base.netmask
        };

        if args.finger {
            log::warn!("转发时校验数据指纹，客户端必须增加--finger参数");
        }

        Self {
            port: if let Some(port) = args.port {
                port
            } else {
                base.port
            },
            backlog: if let Some(backlog) = args.backlog {
                backlog
            } else {
                base.backlog
            },
            white_token: args.white_token.map(HashSet::from_iter),
            gateway,
            netmask,
            broadcast: calculate_broadcast(gateway, netmask),
            finger: args.finger,
            #[cfg(feature = "web")]
            web_manager: if args.web_manager.is_some() && args.web_manager.clone().unwrap().web_port == 0 {
                None
            } else {
                base.web_manager
            },
            log_path: if args.log_path.is_some()  {
                args.log_path
            } else {
                base.log_path
            }
        }
    }

    // pub fn run_service(mut service: Box<dyn Service>, threads: usize) {
    //     let _ = mut service;
        
    // }
}

impl Default for ConfigInfo {
    fn default() -> Self {
        let gateway = Ipv4Addr::new(10, 26, 0, 1);
        let netmask = Ipv4Addr::new(255, 255, 255, 0);
        Self {
            port: 29872,
            backlog: 256,
            white_token: None,
            gateway,
            netmask,
            broadcast: calculate_broadcast(gateway, netmask),
            finger: false,
            #[cfg(feature = "web")]
            web_manager: Some(WebManager {
                username: "admin".to_string(),
                password: "admin".to_string(),
                web_port: 29870,
            }),
            log_path: Some("./log".to_string())
        }
    }
}

#[inline]
pub fn calculate_broadcast(ip: Ipv4Addr, netmask: Ipv4Addr) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(ip) | !u32::from(netmask))
}

#[inline]
pub fn is_valid_netmask(netmask: Ipv4Addr) -> bool {
    let x = u32::from(netmask);
    x.leading_ones() + x.trailing_zeros() == 32
}
