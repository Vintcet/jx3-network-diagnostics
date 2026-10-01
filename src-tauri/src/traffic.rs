//! ETW metadata only. No packet payload is read or persisted.
use crate::model::TrafficStatus;
use std::{
    collections::HashMap,
    mem::size_of,
    net::{Ipv4Addr, Ipv6Addr},
    ptr::null,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};
use windows_sys::{
    core::{w, GUID},
    Win32::System::Diagnostics::Etw::*,
};

#[derive(Default, Clone)]
pub struct Bytes {
    pub received: u64,
    pub sent: u64,
    pub earliest: u64,
}
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameFlow {
    pub pid: u32,
    pub protocol: String,
    pub local: String,
    pub ip: String,
    pub port: u16,
    pub sent: u64,
    pub received: u64,
}
#[derive(Default)]
struct Shared {
    bytes: HashMap<u32, Bytes>,
    events: u64,
    unparsed: u64,
    watched: HashMap<u32, u64>,
    flows: HashMap<FlowKey, GameFlow>,
    endpoint_events: u64,
    endpoint_errors: u64,
}
type FlowKey = (u32, String, String, u16, String);
pub struct TrafficMonitor {
    control: CONTROLTRACE_HANDLE,
    consumer: PROCESSTRACE_HANDLE,
    properties: Vec<usize>,
    name: Vec<u16>,
    shared: Arc<Mutex<Shared>>,
    worker: Option<JoinHandle<()>>,
    error: Option<String>,
}

unsafe fn u32_property(record: *const EVENT_RECORD, name: *const u16) -> Option<u32> {
    let desc = PROPERTY_DATA_DESCRIPTOR {
        PropertyName: name as u64,
        ArrayIndex: u32::MAX,
        Reserved: 0,
    };
    let mut result = 0u32;
    if TdhGetProperty(
        record,
        0,
        null(),
        1,
        &desc,
        4,
        (&mut result as *mut u32).cast(),
    ) == 0
    {
        Some(result)
    } else {
        None
    }
}
unsafe fn property_bytes(
    record: *const EVENT_RECORD,
    name: *const u16,
    size: u32,
) -> Option<[u8; 16]> {
    let desc = PROPERTY_DATA_DESCRIPTOR {
        PropertyName: name as u64,
        ArrayIndex: u32::MAX,
        Reserved: 0,
    };
    let mut bytes = [0u8; 16];
    (TdhGetProperty(record, 0, null(), 1, &desc, size, bytes.as_mut_ptr()) == 0).then_some(bytes)
}
unsafe fn endpoint(record: *const EVENT_RECORD, outgoing: bool) -> Option<(String, u16, String)> {
    let v6 = matches!((*record).EventHeader.EventDescriptor.Id, 26 | 27 | 58 | 59);
    let size = if v6 { 16 } else { 4 };
    let source = property_bytes(record, w!("saddr"), size)?;
    let dest = property_bytes(record, w!("daddr"), size)?;
    let source_port = property_bytes(record, w!("sport"), 2)?;
    let dest_port = property_bytes(record, w!("dport"), 2)?;
    let address = |bytes: [u8; 16]| {
        if v6 {
            Ipv6Addr::from(bytes).to_string()
        } else {
            Ipv4Addr::from(<[u8; 4]>::try_from(&bytes[..4]).unwrap()).to_string()
        }
    };
    let (remote, port, local, local_port) = if outgoing {
        (dest, dest_port, source, source_port)
    } else {
        (source, source_port, dest, dest_port)
    };
    Some((
        address(remote),
        u16::from_be_bytes([port[0], port[1]]),
        format!(
            "{}:{}",
            address(local),
            u16::from_be_bytes([local_port[0], local_port[1]])
        ),
    ))
}
unsafe extern "system" fn callback(record: *mut EVENT_RECORD) {
    if record.is_null() || (*record).UserContext.is_null() {
        return;
    }
    let r = &*record;
    let opcode = r.EventHeader.EventDescriptor.Opcode;
    // Manifest opcodes cover both IPv4 and IPv6. Retransmit/copy are intentionally excluded.
    if !matches!(opcode, 10 | 11 | 42 | 43) {
        return;
    }
    let shared = &*(r.UserContext as *const Mutex<Shared>);
    let (pid, size) = (
        u32_property(record, w!("PID")),
        u32_property(record, w!("size")),
    );
    if let Ok(mut s) = shared.lock() {
        if let (Some(pid), Some(size)) = (pid, size) {
            s.events += 1;
            // Bound memory even under a burst of short-lived processes.
            if s.bytes.len() >= 8192 && !s.bytes.contains_key(&pid) {
                s.unparsed += 1;
                return;
            }
            let b = s.bytes.entry(pid).or_default();
            if matches!(opcode, 10 | 42) {
                b.sent += size as u64;
            } else {
                b.received += size as u64;
            }
            let at = r.EventHeader.TimeStamp as u64;
            b.earliest = if b.earliest == 0 {
                at
            } else {
                b.earliest.min(at)
            };
            if s.watched.get(&pid).is_some_and(|created| at >= *created) {
                let outgoing = matches!(opcode, 10 | 42);
                if let Some((ip, port, local)) = endpoint(record, outgoing) {
                    s.endpoint_events += 1;
                    let protocol = format!(
                        "{}{}",
                        if matches!(opcode, 42 | 43) {
                            "UDP"
                        } else {
                            "TCP"
                        },
                        if ip.contains(':') { "6" } else { "4" }
                    );
                    let key = (pid, protocol.clone(), ip.clone(), port, local.clone());
                    if s.flows.len() < 1024 || s.flows.contains_key(&key) {
                        let flow = s.flows.entry(key).or_insert_with(|| GameFlow {
                            pid,
                            protocol,
                            local,
                            ip,
                            port,
                            ..Default::default()
                        });
                        if outgoing {
                            flow.sent += size as u64;
                        } else {
                            flow.received += size as u64;
                        }
                    } else {
                        s.endpoint_errors += 1;
                    }
                } else {
                    s.endpoint_errors += 1;
                }
            }
        } else {
            s.unparsed += 1;
        }
    }
}
impl TrafficMonitor {
    pub fn set_processes(&mut self, identities: HashMap<u32, u64>) {
        let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        if s.watched != identities {
            let retained = s
                .watched
                .iter()
                .filter(|(pid, created)| identities.get(pid) == Some(created))
                .map(|(pid, _)| *pid)
                .collect::<std::collections::HashSet<_>>();
            s.flows.retain(|_, flow| retained.contains(&flow.pid));
            s.watched = identities;
        }
    }
    pub fn start() -> Self {
        let name: Vec<u16> = format!(
            "Jx3Net-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_micros()
        )
        .encode_utf16()
        .chain(Some(0))
        .collect();
        let bytes = size_of::<EVENT_TRACE_PROPERTIES>() + name.len() * 2;
        let mut m = Self {
            control: CONTROLTRACE_HANDLE { Value: 0 },
            consumer: PROCESSTRACE_HANDLE { Value: u64::MAX },
            properties: vec![0usize; bytes.div_ceil(size_of::<usize>())],
            name,
            shared: Arc::new(Mutex::new(Shared::default())),
            worker: None,
            error: None,
        };
        unsafe {
            let p = m.properties.as_mut_ptr().cast::<EVENT_TRACE_PROPERTIES>();
            (*p).Wnode.BufferSize = bytes as u32;
            (*p).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
            (*p).Wnode.ClientContext = 2;
            (*p).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
            (*p).BufferSize = 64;
            (*p).MinimumBuffers = 4;
            (*p).MaximumBuffers = 64;
            (*p).FlushTimer = 1;
            (*p).LoggerNameOffset = size_of::<EVENT_TRACE_PROPERTIES>() as u32;
            std::ptr::copy_nonoverlapping(
                m.name.as_ptr(),
                (p as *mut u8)
                    .add(size_of::<EVENT_TRACE_PROPERTIES>())
                    .cast(),
                m.name.len(),
            );
            let code = StartTraceW(&mut m.control, m.name.as_ptr(), p);
            if code != 0 {
                m.error = Some(format!("每进程流量不可用：Windows 错误 {code}。可尝试以管理员身份运行；连接表与网卡统计仍可使用。"));
                return m;
            }
            let provider = GUID::from_u128(0x7dd42a49_5329_4832_8dfd_43d979153a88);
            let code = EnableTraceEx2(
                m.control,
                &provider,
                EVENT_CONTROL_CODE_ENABLE_PROVIDER,
                4,
                0x30,
                0,
                0,
                null(),
            );
            if code != 0 {
                m.error = Some(format!("网络事件提供器不可用：{code}"));
                return m;
            }
            let context = Arc::into_raw(m.shared.clone());
            let mut log = EVENT_TRACE_LOGFILEW {
                LoggerName: m.name.as_mut_ptr(),
                ..Default::default()
            };
            log.Anonymous1.ProcessTraceMode =
                PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
            log.Anonymous2.EventRecordCallback = Some(callback);
            log.Context = context as *mut _;
            m.consumer = OpenTraceW(&mut log);
            if m.consumer.Value == u64::MAX {
                drop(Arc::from_raw(context));
                m.error = Some("无法打开网络事件实时流".into());
                return m;
            }
            let handle = m.consumer;
            let ptr = context as usize;
            m.worker = Some(std::thread::spawn(move || {
                ProcessTrace(&handle, 1, null(), null());
                drop(Arc::from_raw(ptr as *const Mutex<Shared>));
            }));
        }
        m
    }
    pub fn sample(&mut self) -> (HashMap<u32, Bytes>, Vec<GameFlow>, TrafficStatus) {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
            && self.error.is_none()
        {
            self.error = Some("网络事件实时流已结束，每进程流量无法继续测量。".into());
        }
        let mut lost = 0;
        if self.control.Value != 0 {
            unsafe {
                let p = self
                    .properties
                    .as_mut_ptr()
                    .cast::<EVENT_TRACE_PROPERTIES>();
                if ControlTraceW(
                    self.control,
                    self.name.as_ptr(),
                    p,
                    EVENT_TRACE_CONTROL_QUERY,
                ) == 0
                {
                    lost = (*p).EventsLost + (*p).RealTimeBuffersLost;
                }
            }
        }
        let mut s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        let available = self.error.is_none() && s.events > 0;
        let message = self.error.clone().unwrap_or_else(|| {
            if s.events == 0 {
                "事件采集已启动，等待有效网络事件；当前尚无每进程流量数据。".into()
            } else if lost > 0 || s.unparsed > 0 {
                "已采集网络事件，但存在事件丢失或无法解析；流量是部分观测值。".into()
            } else {
                "正在采集 TCP / UDP 网络事件；速率受约 1 秒事件交付延迟影响。".into()
            }
        });
        (
            std::mem::take(&mut s.bytes),
            std::mem::take(&mut s.flows).into_values().collect(),
            TrafficStatus {
                available,
                message,
                events: s.events,
                lost_events: lost,
                unparsed_events: s.unparsed,
                endpoint_events: s.endpoint_events,
                endpoint_errors: s.endpoint_errors,
            },
        )
    }
    pub fn flush(&mut self) {
        if self.control.Value != 0 {
            unsafe {
                ControlTraceW(
                    self.control,
                    self.name.as_ptr(),
                    self.properties.as_mut_ptr().cast(),
                    EVENT_TRACE_CONTROL_FLUSH,
                );
            }
        }
    }
}
impl Drop for TrafficMonitor {
    fn drop(&mut self) {
        unsafe {
            if self.control.Value != 0 {
                ControlTraceW(
                    self.control,
                    self.name.as_ptr(),
                    self.properties.as_mut_ptr().cast(),
                    EVENT_TRACE_CONTROL_STOP,
                );
            }
            if self.consumer.Value != u64::MAX {
                CloseTrace(self.consumer);
            }
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
