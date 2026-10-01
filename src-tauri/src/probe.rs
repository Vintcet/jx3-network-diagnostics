use crate::model::{now, Hop, Probe};
use std::{
    net::{Ipv4Addr, SocketAddr, TcpStream},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{GetLastError, INVALID_HANDLE_VALUE},
    NetworkManagement::IpHelper::*,
};

pub fn echo(
    ip: Ipv4Addr,
    ttl: u8,
    timeout: u32,
) -> (String, Option<f64>, Option<String>, Option<String>) {
    unsafe {
        let h = IcmpCreateFile();
        if h == INVALID_HANDLE_VALUE {
            return (
                "error".into(),
                None,
                None,
                Some(format!("ICMP 句柄错误 {}", GetLastError())),
            );
        }
        let payload = [0u8; 24];
        let mut reply = vec![0u64; 64];
        let options = IP_OPTION_INFORMATION {
            Ttl: ttl,
            Tos: 0,
            Flags: 0,
            OptionsSize: 0,
            OptionsData: std::ptr::null_mut(),
        };
        let count = IcmpSendEcho(
            h,
            u32::from_ne_bytes(ip.octets()),
            payload.as_ptr().cast(),
            payload.len() as u16,
            &options,
            reply.as_mut_ptr().cast(),
            (reply.len() * 8) as u32,
            timeout,
        );
        let (code, ms, address) = if count > 0 {
            let r = &*reply.as_ptr().cast::<ICMP_ECHO_REPLY>();
            (
                r.Status,
                Some(r.RoundTripTime as f64),
                Some(Ipv4Addr::from(r.Address.to_ne_bytes()).to_string()),
            )
        } else {
            (GetLastError(), None, None)
        };
        IcmpCloseHandle(h);
        let status = match code {
            IP_SUCCESS => "ok",
            IP_REQ_TIMED_OUT => "timeout",
            IP_TTL_EXPIRED_TRANSIT => "hop",
            _ => "error",
        }
        .to_string();
        let detail = if matches!(code, IP_SUCCESS | IP_REQ_TIMED_OUT | IP_TTL_EXPIRED_TRANSIT) {
            None
        } else {
            Some(format!("ICMP 状态 {code}"))
        };
        (status, ms, address, detail)
    }
}
pub fn icmp(ip: &str, label: &str, role: &str, elapsed: f64) -> Probe {
    let at = now();
    let started = Instant::now();
    let (mut status, mut ms, _, mut detail) = match ip.parse() {
        Ok(ip) => echo(ip, 64, 800),
        Err(_) => (
            "unsupported".into(),
            None,
            None,
            Some("当前主动 ICMP 探测支持 IPv4；IPv6 连接仍被记录".into()),
        ),
    };
    if started.elapsed() > Duration::from_millis(2300) {
        status = "sampling_gap".into();
        ms = None;
        detail = Some("探测调用经历调度停顿，不计为网络超时".into());
    }
    Probe {
        at,
        elapsed,
        target: ip.into(),
        label: label.into(),
        role: role.into(),
        method: "ICMP".into(),
        status,
        ms,
        detail,
    }
}
pub fn tcp(ip: &str, port: u16, label: &str, elapsed: f64) -> Probe {
    let at = now();
    let start = Instant::now();
    let result = ip
        .parse()
        .map(|ip| SocketAddr::new(ip, port))
        .map_err(|_| "地址无效".to_owned())
        .and_then(|addr| {
            TcpStream::connect_timeout(&addr, Duration::from_millis(900)).map_err(|e| {
                match e.kind() {
                    std::io::ErrorKind::TimedOut => "timeout".into(),
                    std::io::ErrorKind::ConnectionRefused => "refused".into(),
                    _ => e.to_string(),
                }
            })
        });
    let (mut status, mut ms, mut detail) = match result {
        Ok(stream) => {
            drop(stream);
            (
                "ok".into(),
                Some(start.elapsed().as_secs_f64() * 1000.0),
                None,
            )
        }
        Err(e) => {
            let status = if e == "timeout" {
                "timeout"
            } else if e == "refused" {
                "refused"
            } else {
                "error"
            };
            (status.into(), None, Some(e))
        }
    };
    if start.elapsed() > Duration::from_millis(2400) {
        status = "sampling_gap".into();
        ms = None;
        detail = Some("建连调用经历调度停顿，不计为网络超时".into());
    }
    Probe {
        at,
        elapsed,
        target: format!("{ip}:{port}"),
        label: label.into(),
        role: "server".into(),
        method: "TCP".into(),
        status,
        ms,
        detail,
    }
}
pub fn trace(ip: &str, stop: &std::sync::atomic::AtomicBool, mut emit: impl FnMut(Hop)) {
    let Ok(ip) = ip.parse::<Ipv4Addr>() else {
        return;
    };
    for ttl in 1..=20 {
        if stop.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        let (status, ms, address, _) = echo(ip, ttl, 500);
        let done = status == "ok";
        emit(Hop {
            target: ip.to_string(),
            ttl,
            address,
            ms,
            status,
        });
        if done {
            break;
        }
    }
}
