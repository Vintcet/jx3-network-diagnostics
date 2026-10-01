use crate::model::*;
use std::{
    collections::HashMap,
    mem::{size_of, zeroed},
    net::{Ipv4Addr, Ipv6Addr},
    ptr::{null, null_mut},
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::*,
    NetworkManagement::{IpHelper::*, WiFi::*},
    Networking::WinSock::*,
    System::{Diagnostics::ToolHelp::*, ProcessStatus::*, Threading::*},
};

fn wide(v: &[u16]) -> String {
    String::from_utf16_lossy(&v[..v.iter().position(|c| *c == 0).unwrap_or(v.len())])
}
fn ft(t: FILETIME) -> u64 {
    ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64
}
fn ip4(v: u32) -> String {
    Ipv4Addr::from(v.to_ne_bytes()).to_string()
}
fn port(v: u32) -> u16 {
    u16::from_be(v as u16)
}
fn state(v: u32) -> String {
    match v {
        1 => "关闭",
        2 => "监听",
        3 => "连接中",
        4 => "握手中",
        5 => "已连接",
        6 | 7 => "关闭中",
        8 => "等待关闭",
        9 => "最后确认",
        10 => "关闭等待",
        11 => "等待回收",
        12 => "已删除",
        _ => "未知",
    }
    .into()
}
pub fn process(pid: u32, fallback: &str) -> (ProcessInfo, Option<u64>) {
    let mut p = ProcessInfo {
        pid,
        name: fallback.to_owned(),
        ..Default::default()
    };
    let mut cpu = None;
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if !h.is_null() {
            let mut path = vec![0u16; 32768];
            let mut n = path.len() as u32;
            if QueryFullProcessImageNameW(h, 0, path.as_mut_ptr(), &mut n) != 0 {
                let s = String::from_utf16_lossy(&path[..n as usize]);
                p.name = s.rsplit('\\').next().unwrap_or(&s).into();
                p.path = Some(s);
            }
            let (mut c, mut e, mut k, mut u) = (zeroed(), zeroed(), zeroed(), zeroed());
            if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) != 0 {
                p.started = Some(ft(c).to_string());
                cpu = Some(ft(k) + ft(u));
            }
            let mut mem: PROCESS_MEMORY_COUNTERS = zeroed();
            mem.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            if K32GetProcessMemoryInfo(h, &mut mem, mem.cb) != 0 {
                p.memory_bytes = Some(mem.WorkingSetSize as u64);
            }
            CloseHandle(h);
        }
    }
    if p.name.is_empty() {
        p.name = format!("进程 {}", pid);
    }
    let name = p.name.to_lowercase();
    p.is_game = ["jx3", "jx3client", "jx3clientx64"]
        .iter()
        .any(|s| name.starts_with(s))
        && !name.contains("network-diagnostics");
    (p, cpu)
}
pub fn processes() -> Vec<ProcessInfo> {
    let mut out = vec![];
    unsafe {
        let h = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if h == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(h, &mut entry);
        while ok != 0 {
            if entry.th32ProcessID > 0 {
                out.push(process(entry.th32ProcessID, &wide(&entry.szExeFile)).0);
            }
            ok = Process32NextW(h, &mut entry);
        }
        CloseHandle(h);
    }
    out.sort_by(|a, b| {
        b.is_game
            .cmp(&a.is_game)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}
unsafe fn table(tcp: bool, family: u32) -> Result<Vec<usize>, String> {
    let mut bytes = 0;
    let call = |p: *mut std::ffi::c_void, n: &mut u32| {
        if tcp {
            GetExtendedTcpTable(p, n, 0, family, TCP_TABLE_OWNER_PID_ALL, 0)
        } else {
            GetExtendedUdpTable(p, n, 0, family, UDP_TABLE_OWNER_PID, 0)
        }
    };
    call(null_mut(), &mut bytes);
    for _ in 0..3 {
        if !(4..=32_000_000).contains(&bytes) {
            return Err("连接表大小异常".into());
        }
        let mut buffer = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
        let result = call(buffer.as_mut_ptr().cast(), &mut bytes);
        if result == 0 {
            return Ok(buffer);
        }
        if result != ERROR_INSUFFICIENT_BUFFER {
            return Err(format!("连接表读取错误 {result}"));
        }
    }
    Err("连接变化过快，连接表读取重试失败".into())
}
pub fn connections() -> (Vec<Connection>, Vec<String>) {
    let mut out = vec![];
    let mut errors = vec![];
    unsafe {
        for (tcp, family) in [
            (true, AF_INET),
            (true, AF_INET6),
            (false, AF_INET),
            (false, AF_INET6),
        ] {
            let buffer = match table(tcp, family as u32) {
                Ok(v) => v,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
            let ptr = buffer.as_ptr().cast::<u8>();
            let count = std::ptr::read_unaligned(ptr.cast::<u32>()) as usize;
            let row_size = match (tcp, family) {
                (true, AF_INET) => size_of::<MIB_TCPROW_OWNER_PID>(),
                (true, _) => size_of::<MIB_TCP6ROW_OWNER_PID>(),
                (false, AF_INET) => size_of::<MIB_UDPROW_OWNER_PID>(),
                _ => size_of::<MIB_UDP6ROW_OWNER_PID>(),
            };
            if 4 + count * row_size > buffer.len() * size_of::<usize>() {
                errors.push("连接表长度不匹配".into());
                continue;
            }
            for i in 0..count {
                let row = ptr.add(4 + row_size * i);
                let (pid, local_ip, local_port, remote_ip, remote_port, status) =
                    match (tcp, family) {
                        (true, AF_INET) => {
                            let r = std::ptr::read_unaligned(row.cast::<MIB_TCPROW_OWNER_PID>());
                            (
                                r.dwOwningPid,
                                ip4(r.dwLocalAddr),
                                port(r.dwLocalPort),
                                Some(ip4(r.dwRemoteAddr)),
                                Some(port(r.dwRemotePort)),
                                state(r.dwState),
                            )
                        }
                        (true, _) => {
                            let r = std::ptr::read_unaligned(row.cast::<MIB_TCP6ROW_OWNER_PID>());
                            (
                                r.dwOwningPid,
                                Ipv6Addr::from(r.ucLocalAddr).to_string(),
                                port(r.dwLocalPort),
                                Some(Ipv6Addr::from(r.ucRemoteAddr).to_string()),
                                Some(port(r.dwRemotePort)),
                                state(r.dwState),
                            )
                        }
                        (false, AF_INET) => {
                            let r = std::ptr::read_unaligned(row.cast::<MIB_UDPROW_OWNER_PID>());
                            (
                                r.dwOwningPid,
                                ip4(r.dwLocalAddr),
                                port(r.dwLocalPort),
                                None,
                                None,
                                "本地端点".into(),
                            )
                        }
                        _ => {
                            let r = std::ptr::read_unaligned(row.cast::<MIB_UDP6ROW_OWNER_PID>());
                            (
                                r.dwOwningPid,
                                Ipv6Addr::from(r.ucLocalAddr).to_string(),
                                port(r.dwLocalPort),
                                None,
                                None,
                                "本地端点".into(),
                            )
                        }
                    };
                let endpoint = |ip: &str, p: u16| {
                    if ip.contains(':') {
                        format!("[{ip}]:{p}")
                    } else {
                        format!("{ip}:{p}")
                    }
                };
                let remote = remote_ip
                    .as_ref()
                    .zip(remote_port)
                    .map(|(ip, p)| endpoint(ip, p));
                out.push(Connection {
                    pid,
                    protocol: format!(
                        "{}{}",
                        if tcp { "TCP" } else { "UDP" },
                        if family == AF_INET6 { "6" } else { "4" }
                    ),
                    local: endpoint(&local_ip, local_port),
                    remote,
                    remote_ip,
                    remote_port,
                    state: status,
                });
            }
        }
    }
    (out, errors)
}
unsafe fn sock_ip(s: SOCKET_ADDRESS) -> Option<String> {
    if s.lpSockaddr.is_null() {
        return None;
    }
    match (*s.lpSockaddr).sa_family {
        AF_INET => Some(ip4((*s.lpSockaddr.cast::<SOCKADDR_IN>())
            .sin_addr
            .S_un
            .S_addr)),
        AF_INET6 => Some(
            Ipv6Addr::from((*s.lpSockaddr.cast::<SOCKADDR_IN6>()).sin6_addr.u.Byte).to_string(),
        ),
        _ => None,
    }
}
type AdapterAddresses = (Vec<String>, Vec<String>, Vec<String>);
fn adapter_addresses() -> HashMap<u32, AdapterAddresses> {
    let mut out = HashMap::new();
    unsafe {
        let mut n = 15000u32;
        for _ in 0..3 {
            let mut buf = vec![0usize; (n as usize).div_ceil(size_of::<usize>())];
            let result = GetAdaptersAddresses(
                AF_UNSPEC as u32,
                GAA_FLAG_INCLUDE_GATEWAYS,
                null(),
                buf.as_mut_ptr().cast(),
                &mut n,
            );
            if result == ERROR_BUFFER_OVERFLOW {
                continue;
            }
            if result != 0 {
                break;
            }
            let mut p = buf.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
            while !p.is_null() {
                let a = &*p;
                let mut ips = vec![];
                let mut gateways = vec![];
                let mut dns = vec![];
                let mut u = a.FirstUnicastAddress;
                while !u.is_null() {
                    if let Some(s) = sock_ip((*u).Address) {
                        ips.push(s);
                    }
                    u = (*u).Next;
                }
                let mut g = a.FirstGatewayAddress;
                while !g.is_null() {
                    if let Some(s) = sock_ip((*g).Address) {
                        gateways.push(s);
                    }
                    g = (*g).Next;
                }
                let mut d = a.FirstDnsServerAddress;
                while !d.is_null() {
                    if let Some(s) = sock_ip((*d).Address) {
                        dns.push(s);
                    }
                    d = (*d).Next;
                }
                out.insert(a.Anonymous1.Anonymous.IfIndex, (ips, gateways, dns));
                p = a.Next;
            }
            break;
        }
    }
    out
}
pub fn environment(target: Option<&str>) -> Environment {
    let mut env = Environment::default();
    let addresses = adapter_addresses();
    unsafe {
        if let Some(ip) = target.and_then(|s| s.parse::<Ipv4Addr>().ok()) {
            let mut index = 0;
            if GetBestInterface(u32::from_ne_bytes(ip.octets()), &mut index) == 0 {
                env.selected_interface = Some(index);
            }
        }
        let mut table_ptr = null_mut();
        let result = GetIfTable2(&mut table_ptr);
        if result != 0 {
            env.notes
                .push(format!("网卡统计不可用：Windows 错误 {result}"));
            return env;
        }
        for r in std::slice::from_raw_parts(
            (*table_ptr).Table.as_ptr(),
            (*table_ptr).NumEntries as usize,
        ) {
            if r.Type == 24 {
                continue;
            }
            let (ips, gateways, dns) = addresses
                .get(&r.InterfaceIndex)
                .cloned()
                .unwrap_or_default();
            env.adapters.push(Adapter {
                index: r.InterfaceIndex,
                name: wide(&r.Alias),
                description: wide(&r.Description),
                kind: match r.Type {
                    71 => "Wi-Fi",
                    6 => "以太网",
                    131 => "隧道",
                    _ => "其他／虚拟",
                }
                .into(),
                status: if r.OperStatus == 1 {
                    "已连接"
                } else {
                    "未连接"
                }
                .into(),
                addresses: ips,
                gateways,
                dns,
                received: r.InOctets,
                sent: r.OutOctets,
                in_errors: r.InErrors,
                out_errors: r.OutErrors,
                in_discards: r.InDiscards,
                out_discards: r.OutDiscards,
                link_speed: r.TransmitLinkSpeed,
                ..Default::default()
            });
        }
        FreeMibTable(table_ptr.cast());
    }
    env.adapters.sort_by_key(|a| {
        (
            Some(a.index) != env.selected_interface,
            a.status != "已连接",
            a.index,
        )
    });
    if env.adapters.iter().any(|a| {
        let s = format!("{} {}", a.name, a.description).to_lowercase();
        ["vpn", "tap", "tun", "wintun", "wireguard", "加速"]
            .iter()
            .any(|v| s.contains(v))
            && a.status == "已连接"
    }) {
        env.notes.push(
            "发现可能的虚拟／加速网卡，主动探测未必经过游戏加速线路；各网卡流量不相加。".into(),
        );
    }
    env
}
pub fn wifi() -> Vec<String> {
    let mut out = vec![];
    unsafe {
        let mut version = 0;
        let mut h = null_mut();
        let result = WlanOpenHandle(2, null(), &mut version, &mut h);
        if result != 0 {
            return vec![format!("无线信息不可用（{result}），可能未启用无线服务")];
        }
        let mut list = null_mut();
        let result = WlanEnumInterfaces(h, null(), &mut list);
        if result == 0 {
            for item in std::slice::from_raw_parts(
                (*list).InterfaceInfo.as_ptr(),
                (*list).dwNumberOfItems as usize,
            ) {
                let mut n = 0;
                let mut data = null_mut();
                let mut value_type = 0;
                let result = WlanQueryInterface(
                    h,
                    &item.InterfaceGuid,
                    wlan_intf_opcode_current_connection,
                    null(),
                    &mut n,
                    &mut data,
                    &mut value_type,
                );
                if result == 0
                    && !data.is_null()
                    && n as usize >= size_of::<WLAN_CONNECTION_ATTRIBUTES>()
                {
                    let a = &*data.cast::<WLAN_CONNECTION_ATTRIBUTES>();
                    out.push(format!(
                        "{}：信号 {}%，接收链路 {} Mbps，发送链路 {} Mbps",
                        wide(&item.strInterfaceDescription),
                        a.wlanAssociationAttributes.wlanSignalQuality,
                        a.wlanAssociationAttributes.ulRxRate / 1000,
                        a.wlanAssociationAttributes.ulTxRate / 1000
                    ));
                } else {
                    out.push(format!(
                        "{}：无线详情不可读（{result}；可能需要 Windows 位置权限）",
                        wide(&item.strInterfaceDescription)
                    ));
                }
                if !data.is_null() {
                    WlanFreeMemory(data);
                }
            }
            WlanFreeMemory(list.cast());
        }
        WlanCloseHandle(h, null());
    }
    out
}

pub struct CpuSampler {
    previous: HashMap<(u32, String), (u64, Instant)>,
    cores: f64,
}
impl CpuSampler {
    pub fn new() -> Self {
        Self {
            previous: HashMap::new(),
            cores: std::thread::available_parallelism()
                .map(|n| n.get() as f64)
                .unwrap_or(1.0),
        }
    }
    pub fn sample(&mut self, pid: u32) -> ProcessInfo {
        let (mut p, cpu) = process(pid, "");
        if let (Some(start), Some(cpu)) = (p.started.clone(), cpu) {
            let time = Instant::now();
            if let Some((last, at)) = self.previous.insert((pid, start), (cpu, time)) {
                let secs = time.duration_since(at).as_secs_f64();
                p.cpu_percent = Some(
                    ((cpu.saturating_sub(last) as f64 / 10_000_000.0) / secs / self.cores * 100.0)
                        .clamp(0.0, 100.0),
                );
            }
        }
        p
    }
}
