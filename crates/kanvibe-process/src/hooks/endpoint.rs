//! hook 서버 포트와 hook 스크립트가 통보할 서버 주소(TS `hookEndpoint.ts`).
//! TS는 포트를 전역 값으로 두었지만, 여기서는 런타임 상태를 전역에 두지 않도록 `HookServerEndpoint` 값으로 들고 다닌다.
//! 원격 주소는 SSH 연결이 실제로 나가는 로컬 IPv4를 먼저 쓰고, 찾지 못하면 내부망 IPv4로 물러선다.

use crate::ssh_config::SshConfigReader;
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, ToSocketAddrs, UdpSocket};

pub const KANVIBE_HOOK_SERVER_PORT: u16 = 9736;
pub const KANVIBE_DEV_HOOK_SERVER_PORT: u16 = 19736;
/// 모바일 전용 TLS 포트는 hook 포트에서 이만큼 떨어진 곳에 연다
pub const MOBILE_TLS_PORT_OFFSET: u16 = 10;

const DEFAULT_SSH_PORT: u16 = 22;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HookEndpointError {
    #[error("원격 환경에서 로컬 Hook 서버에 접근할 수 있는 경로를 찾지 못했습니다.")]
    RemoteRouteNotFound,
}

/// `~/.ssh/config`에서 읽은 호스트의 실제 접속 주소와 포트
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SshRouteTarget {
    pub hostname: Option<String>,
    pub port: Option<u16>,
}

/// Node `os.networkInterfaces()`의 주소 하나
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkInterfaceAddress {
    pub address: String,
    pub is_ipv4: bool,
    pub is_internal: bool,
}

/// 원격 hook 주소를 고르는 데 필요한 네트워크 조회. 테스트는 가짜 구현으로 바꾼다
pub trait HookNetworkRoutes {
    fn ssh_route_target(&self, ssh_host: &str) -> Option<SshRouteTarget>;
    fn resolve_ipv4(&self, hostname: &str) -> Option<Ipv4Addr>;
    /// UDP 소켓을 원격 주소로 connect했을 때 고른 로컬 주소. 패킷은 보내지 않는다
    fn outbound_ipv4_address(&self, remote_address: Ipv4Addr, remote_port: u16) -> Option<String>;
    /// 인터페이스 이름별로 묶인 순서의 주소 목록
    fn network_interface_addresses(&self) -> Vec<NetworkInterfaceAddress>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookServerEndpoint {
    port: u16,
}

impl Default for HookServerEndpoint {
    fn default() -> Self {
        Self::new(KANVIBE_HOOK_SERVER_PORT)
    }
}

impl HookServerEndpoint {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn mobile_tls_port(&self) -> u16 {
        self.port + MOBILE_TLS_PORT_OFFSET
    }

    pub fn local_url(&self) -> String {
        format!("http://localhost:{}", self.port)
    }

    pub fn remote_url(
        &self,
        ssh_host: Option<&str>,
        network: &dyn HookNetworkRoutes,
    ) -> Result<String, HookEndpointError> {
        let preferred_host = ssh_route_ipv4_address(ssh_host, network)
            .filter(|address| !address.is_empty())
            .or_else(|| preferred_ipv4_address(&network.network_interface_addresses()))
            .ok_or(HookEndpointError::RemoteRouteNotFound)?;
        Ok(format!("http://{preferred_host}:{}", self.port))
    }

    /// ssh_host가 있으면 원격에서 닿는 주소, 없으면 localhost 주소
    pub fn url(
        &self,
        ssh_host: Option<&str>,
        network: &dyn HookNetworkRoutes,
    ) -> Result<String, HookEndpointError> {
        match ssh_host.filter(|host| !host.is_empty()) {
            Some(host) => self.remote_url(Some(host), network),
            None => Ok(self.local_url()),
        }
    }
}

fn ssh_route_ipv4_address(
    ssh_host: Option<&str>,
    network: &dyn HookNetworkRoutes,
) -> Option<String> {
    let ssh_host = ssh_host.filter(|host| !host.is_empty())?;
    let target = network.ssh_route_target(ssh_host).unwrap_or_default();
    let remote_hostname = target
        .hostname
        .filter(|hostname| !hostname.is_empty())
        .unwrap_or_else(|| ssh_host.to_string());
    let remote_port = target
        .port
        .filter(|port| *port != 0)
        .unwrap_or(DEFAULT_SSH_PORT);
    let remote_address = match remote_hostname.parse::<Ipv4Addr>() {
        Ok(address) => address,
        Err(_) => network.resolve_ipv4(&remote_hostname)?,
    };
    network.outbound_ipv4_address(remote_address, remote_port)
}

fn is_private_ipv4(address: &str) -> bool {
    let is_private_172 = address
        .strip_prefix("172.")
        .and_then(|rest| rest.split('.').next())
        .and_then(|second| second.parse::<u8>().ok().filter(|_| second.len() == 2))
        .is_some_and(|second| (16..=31).contains(&second));
    address.starts_with("10.") || address.starts_with("192.168.") || is_private_172
}

/// 내부망 IPv4를 먼저, 없으면 첫 외부 IPv4를 고른다
fn preferred_ipv4_address(addresses: &[NetworkInterfaceAddress]) -> Option<String> {
    let mut external_addresses = addresses
        .iter()
        .filter(|address| address.is_ipv4 && !address.is_internal);
    let private_address = external_addresses
        .clone()
        .find(|address| is_private_ipv4(&address.address));
    private_address
        .or_else(|| external_addresses.next())
        .map(|address| address.address.clone())
}

/// 실제 DNS·UDP·인터페이스와 `~/.ssh/config`를 조회하는 구현
pub struct SystemHookNetworkRoutes {
    ssh_config: SshConfigReader,
}

impl SystemHookNetworkRoutes {
    pub fn new(ssh_config: SshConfigReader) -> Self {
        Self { ssh_config }
    }
}

impl HookNetworkRoutes for SystemHookNetworkRoutes {
    fn ssh_route_target(&self, ssh_host: &str) -> Option<SshRouteTarget> {
        let configs = self.ssh_config.parse_ssh_config();
        let config = configs.into_iter().find(|config| config.host == ssh_host)?;
        Some(SshRouteTarget {
            hostname: Some(config.hostname),
            port: u16::try_from(config.port).ok(),
        })
    }

    fn resolve_ipv4(&self, hostname: &str) -> Option<Ipv4Addr> {
        let addresses = (hostname, 0).to_socket_addrs().ok()?;
        addresses
            .filter_map(|address| match address.ip() {
                std::net::IpAddr::V4(address) => Some(address),
                std::net::IpAddr::V6(_) => None,
            })
            .next()
    }

    fn outbound_ipv4_address(&self, remote_address: Ipv4Addr, remote_port: u16) -> Option<String> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
        socket.connect((remote_address, remote_port)).ok()?;
        Some(socket.local_addr().ok()?.ip().to_string())
    }

    fn network_interface_addresses(&self) -> Vec<NetworkInterfaceAddress> {
        system_network_interface_addresses()
    }
}

/// getifaddrs 결과를 Node처럼 인터페이스 이름이 처음 나온 순서로 묶는다. IPv6는 순서만 지키면 되므로 주소를 비워 둔다
fn system_network_interface_addresses() -> Vec<NetworkInterfaceAddress> {
    let mut grouped: BTreeMap<usize, Vec<NetworkInterfaceAddress>> = BTreeMap::new();
    let mut interface_order: Vec<String> = Vec::new();
    for (name, address) in read_interface_addresses() {
        let position = interface_order
            .iter()
            .position(|known| *known == name)
            .unwrap_or_else(|| {
                interface_order.push(name);
                interface_order.len() - 1
            });
        grouped.entry(position).or_default().push(address);
    }
    grouped.into_values().flatten().collect()
}

fn read_interface_addresses() -> Vec<(String, NetworkInterfaceAddress)> {
    let mut addresses = Vec::new();
    let mut interfaces: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs가 채운 연결 목록을 읽기만 하고, 끝나면 freeifaddrs로 한 번 해제한다
    unsafe {
        if libc::getifaddrs(&mut interfaces) != 0 {
            return addresses;
        }
        let mut cursor = interfaces;
        while !cursor.is_null() {
            if let Some(entry) = interface_address_entry(&*cursor) {
                addresses.push(entry);
            }
            cursor = (*cursor).ifa_next;
        }
        libc::freeifaddrs(interfaces);
    }
    addresses
}

/// SAFETY 전제: `entry`는 getifaddrs가 돌려준 유효한 항목이다
unsafe fn interface_address_entry(
    entry: &libc::ifaddrs,
) -> Option<(String, NetworkInterfaceAddress)> {
    if entry.ifa_addr.is_null() || entry.ifa_name.is_null() {
        return None;
    }
    // SAFETY: 위에서 null이 아님을 확인한 getifaddrs 항목의 이름과 주소다
    let (name, family) = unsafe {
        (
            std::ffi::CStr::from_ptr(entry.ifa_name)
                .to_string_lossy()
                .into_owned(),
            i32::from((*entry.ifa_addr).sa_family),
        )
    };
    let is_internal = entry.ifa_flags & (libc::IFF_LOOPBACK as libc::c_uint) != 0;
    let address = match family {
        libc::AF_INET => {
            // SAFETY: AF_INET 항목의 주소는 sockaddr_in이다
            let socket_address = unsafe { &*(entry.ifa_addr as *const libc::sockaddr_in) };
            Ipv4Addr::from(u32::from_be(socket_address.sin_addr.s_addr)).to_string()
        }
        libc::AF_INET6 => String::new(),
        _ => return None,
    };
    let is_ipv4 = family == libc::AF_INET;
    Some((
        name,
        NetworkInterfaceAddress {
            address,
            is_ipv4,
            is_internal,
        },
    ))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// TS 테스트가 node:os·dns·dgram과 sshConfig를 mock하던 자리
    #[derive(Default)]
    struct FakeNetworkRoutes {
        ssh_targets: Vec<(String, SshRouteTarget)>,
        resolved_address: Option<Ipv4Addr>,
        outbound_address: Option<String>,
        interfaces: Vec<NetworkInterfaceAddress>,
        outbound_requests: RefCell<Vec<(Ipv4Addr, u16)>>,
    }

    impl HookNetworkRoutes for FakeNetworkRoutes {
        fn ssh_route_target(&self, ssh_host: &str) -> Option<SshRouteTarget> {
            self.ssh_targets
                .iter()
                .find(|(host, _)| host == ssh_host)
                .map(|(_, target)| target.clone())
        }

        fn resolve_ipv4(&self, _hostname: &str) -> Option<Ipv4Addr> {
            self.resolved_address
        }

        fn outbound_ipv4_address(
            &self,
            remote_address: Ipv4Addr,
            remote_port: u16,
        ) -> Option<String> {
            self.outbound_requests
                .borrow_mut()
                .push((remote_address, remote_port));
            self.outbound_address.clone()
        }

        fn network_interface_addresses(&self) -> Vec<NetworkInterfaceAddress> {
            self.interfaces.clone()
        }
    }

    fn devbox_routes() -> FakeNetworkRoutes {
        FakeNetworkRoutes {
            ssh_targets: vec![(
                "remote-devbox".into(),
                SshRouteTarget {
                    hostname: Some("ssh.example.com".into()),
                    port: Some(2202),
                },
            )],
            resolved_address: Some(Ipv4Addr::new(203, 0, 113, 20)),
            outbound_address: Some("10.0.0.8".into()),
            ..FakeNetworkRoutes::default()
        }
    }

    #[test]
    fn configured_hook_server_port_is_used_in_the_hook_url() {
        let endpoint = HookServerEndpoint::new(19736);

        let url = endpoint.url(None, &FakeNetworkRoutes::default());

        assert_eq!(url.unwrap(), "http://localhost:19736");
    }

    #[test]
    fn dev_hook_server_port_matches_the_dev_port() {
        assert_eq!(KANVIBE_DEV_HOOK_SERVER_PORT, 19736);
    }

    #[test]
    fn remote_hook_url_prefers_the_local_source_ip_of_the_ssh_route() {
        let routes = devbox_routes();

        let url = HookServerEndpoint::default().url(Some("remote-devbox"), &routes);

        assert_eq!(
            routes.outbound_requests.borrow().as_slice(),
            [(Ipv4Addr::new(203, 0, 113, 20), 2202)]
        );
        assert_eq!(url.unwrap(), "http://10.0.0.8:9736");
    }

    #[test]
    fn configured_port_is_used_in_the_remote_hook_url() {
        let url = HookServerEndpoint::new(19736).url(Some("remote-devbox"), &devbox_routes());

        assert_eq!(url.unwrap(), "http://10.0.0.8:19736");
    }

    #[test]
    fn unresolved_ssh_route_falls_back_to_a_private_ipv4_address() {
        let routes = FakeNetworkRoutes {
            resolved_address: Some(Ipv4Addr::new(203, 0, 113, 20)),
            outbound_address: Some(String::new()),
            interfaces: vec![
                NetworkInterfaceAddress {
                    address: "127.0.0.1".into(),
                    is_ipv4: true,
                    is_internal: true,
                },
                NetworkInterfaceAddress {
                    address: "192.168.1.20".into(),
                    is_ipv4: true,
                    is_internal: false,
                },
            ],
            ..FakeNetworkRoutes::default()
        };

        let url = HookServerEndpoint::default().url(Some("remote-devbox"), &routes);

        assert_eq!(url.unwrap(), "http://192.168.1.20:9736");
    }

    #[test]
    fn missing_ssh_host_uses_the_localhost_url() {
        let url = HookServerEndpoint::default().url(None, &FakeNetworkRoutes::default());

        assert_eq!(url.unwrap(), "http://localhost:9736");
    }

    #[test]
    fn public_address_is_used_when_no_private_address_exists() {
        let routes = FakeNetworkRoutes {
            interfaces: vec![
                NetworkInterfaceAddress {
                    address: "172.32.0.4".into(),
                    is_ipv4: true,
                    is_internal: false,
                },
                NetworkInterfaceAddress {
                    address: "172.16.0.4".into(),
                    is_ipv4: true,
                    is_internal: false,
                },
            ],
            ..FakeNetworkRoutes::default()
        };

        let url = HookServerEndpoint::default().remote_url(Some("unknown"), &routes);

        assert_eq!(url.unwrap(), "http://172.16.0.4:9736");
        assert_eq!(
            HookServerEndpoint::default()
                .remote_url(None, &FakeNetworkRoutes::default())
                .unwrap_err()
                .to_string(),
            "원격 환경에서 로컬 Hook 서버에 접근할 수 있는 경로를 찾지 못했습니다."
        );
        assert_eq!(HookServerEndpoint::default().mobile_tls_port(), 9746);
    }

    #[test]
    fn system_interfaces_include_the_loopback_as_internal() {
        let addresses = system_network_interface_addresses();

        assert!(
            addresses
                .iter()
                .any(|address| address.address == "127.0.0.1" && address.is_internal)
        );
    }
}
