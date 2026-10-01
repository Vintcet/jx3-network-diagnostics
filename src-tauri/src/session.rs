use crate::{
    analysis::{self, Activity},
    catalog,
    model::*,
    platform, probe,
    traffic::TrafficMonitor,
};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufWriter, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub struct Manager {
    pub root: PathBuf,
    pub view: Arc<Mutex<SessionView>>,
    stop: Arc<AtomicBool>,
    marker: Mutex<Option<mpsc::Sender<()>>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}
impl Manager {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            view: Arc::new(Mutex::new(SessionView {
                status: "idle".into(),
                ..Default::default()
            })),
            stop: Arc::new(AtomicBool::new(false)),
            marker: Mutex::new(None),
            worker: Mutex::new(None),
        }
    }
    pub fn start(&self, mut config: StartConfig, smoke: bool) -> Result<String, String> {
        let mut worker = self.worker.lock().map_err(|e| e.to_string())?;
        if worker.as_ref().is_some_and(|w| !w.is_finished()) {
            return Err("已有测试正在运行".into());
        }
        if let Some(w) = worker.take() {
            let _ = w.join();
        }
        let minimum = if smoke { 2 } else { 60 };
        if !(minimum..=10800).contains(&config.duration_seconds) {
            return Err("测试时长需在 1–180 分钟之间".into());
        }
        let c = catalog::load(&self.root, false);
        config.server = c
            .servers
            .into_iter()
            .find(|s| s.id == config.server.id)
            .ok_or("区服已变更，请重新选择")?;
        if let Some(pid) = config.game_pid {
            let p = platform::process(pid, "").0;
            if p.path.is_none() || p.started.is_none() || p.started != config.game_started {
                return Err("所选进程已退出或身份变化，请刷新进程列表后重新选择".into());
            }
        }
        let id = format!(
            "{}-{}",
            chrono::Local::now().format("%Y%m%d-%H%M%S-%3f"),
            std::process::id()
        );
        let dir = self.root.join("sessions").join(&id);
        fs::create_dir_all(&dir).map_err(|e| format!("无法创建日志目录：{e}"))?;
        let file = File::create(dir.join("samples.jsonl")).map_err(|e| e.to_string())?;
        let started_at = now();
        fs::write(dir.join("session.json"),serde_json::to_vec_pretty(&json!({"schemaVersion":1,"id":id,"startedAt":started_at,"config":config,"catalogSource":c.source,"catalogDate":c.fetched_at})).unwrap()).map_err(|e|e.to_string())?;
        self.stop.store(false, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        *self.marker.lock().unwrap() = Some(tx);
        *self.view.lock().unwrap() = SessionView {
            id: Some(id.clone()),
            status: "running".into(),
            duration_seconds: config.duration_seconds,
            log_dir: Some(dir.to_string_lossy().into()),
            ..Default::default()
        };
        let view = self.view.clone();
        let stop = self.stop.clone();
        let run_id = id.clone();
        *worker = Some(thread::spawn(move || {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run(
                    config,
                    (run_id, started_at),
                    dir,
                    file,
                    view.clone(),
                    stop.clone(),
                    rx,
                )
            }));
            let error = match outcome {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(e),
                Err(_) => Some("采集线程意外结束；已保存的日志仍可在历史记录中找到。".into()),
            };
            if let Some(e) = error {
                stop.store(true, Ordering::Relaxed);
                let mut v = view.lock().unwrap_or_else(|e| e.into_inner());
                v.status = "error".into();
                v.error = Some(e);
            }
        }));
        Ok(id)
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        let mut v = self.view.lock().unwrap();
        if v.status == "running" {
            v.status = "stopping".into();
        }
    }
    pub fn mark(&self) -> Result<(), String> {
        if self.view.lock().unwrap().status != "running" {
            return Err("请先开始测试".into());
        }
        self.marker
            .lock()
            .unwrap()
            .as_ref()
            .ok_or("没有活动测试")?
            .send(())
            .map_err(|e| e.to_string())
    }
    pub fn wait(&self) {
        if let Some(w) = self.worker.lock().unwrap().take() {
            let _ = w.join();
        }
    }
    pub fn directory(&self, id: &str) -> Result<PathBuf, String> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit() || c == '-') {
            return Err("记录标识无效".into());
        }
        let dir = self.root.join("sessions").join(id);
        if !dir.is_dir() {
            return Err("记录不存在".into());
        }
        Ok(dir)
    }
    pub fn history(&self) -> Vec<HistoryItem> {
        let Ok(entries) = fs::read_dir(self.root.join("sessions")) else {
            return vec![];
        };
        let mut out = vec![];
        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().into_owned();
            if self.directory(&id).is_err() {
                continue;
            }
            if let Ok(report) = self.report(&id) {
                out.push(HistoryItem {
                    id,
                    server_name: report.server.name,
                    started_at: report.started_at,
                    status: report.status,
                    duration_seconds: report.duration_seconds,
                });
            } else if let Ok(bytes) = fs::read(entry.path().join("session.json")) {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    out.push(HistoryItem {
                        id,
                        server_name: v["config"]["server"]["name"]
                            .as_str()
                            .unwrap_or("未知")
                            .into(),
                        started_at: v["startedAt"].as_str().unwrap_or("").into(),
                        status: "incomplete".into(),
                        duration_seconds: 0.0,
                    });
                }
            }
        }
        out.sort_by(|a, b| b.id.cmp(&a.id));
        out.truncate(200);
        out
    }
    pub fn report(&self, id: &str) -> Result<Report, String> {
        serde_json::from_slice(
            &fs::read(self.directory(id)?.join("report.json"))
                .map_err(|_| "此记录尚无完整报告，原始日志已保留")?,
        )
        .map_err(|e| e.to_string())
    }
}
impl Drop for Manager {
    fn drop(&mut self) {
        self.stop();
        self.wait();
    }
}
fn record(
    writer: &mut BufWriter<File>,
    kind: &str,
    data: &impl serde::Serialize,
) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, &json!({"type":kind,"data":data}))
        .map_err(|e| format!("日志写入失败：{e}"))?;
    writer
        .write_all(b"\n")
        .map_err(|e| format!("日志写入失败：{e}"))
}
fn event(
    writer: &mut BufWriter<File>,
    events: &mut Vec<Event>,
    start: Instant,
    kind: &str,
    level: &str,
    message: String,
) -> Result<(), String> {
    let e = Event {
        at: now(),
        elapsed: start.elapsed().as_secs_f64(),
        level: level.into(),
        kind: kind.into(),
        message,
    };
    record(writer, "event", &e)?;
    events.push(e);
    Ok(())
}
#[derive(Clone)]
struct Target {
    ip: String,
    label: String,
    role: String,
    port: Option<u16>,
}
enum Measurement {
    Probe(Probe),
    Hop(Hop),
}
struct ProbeWorkers {
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}
impl Drop for ProbeWorkers {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for w in self.threads.drain(..) {
            let _ = w.join();
        }
    }
}
fn run(
    config: StartConfig,
    identity: (String, String),
    dir: PathBuf,
    file: File,
    view: Arc<Mutex<SessionView>>,
    stop: Arc<AtomicBool>,
    markers: mpsc::Receiver<()>,
) -> Result<(), String> {
    let (id, started_at) = identity;
    let start = Instant::now();
    let mut writer = BufWriter::new(file);
    let mut events = vec![];
    let mut probes = vec![];
    let mut activities = vec![];
    let mut monitor = TrafficMonitor::start();
    let mut cpu = platform::CpuSampler::new();
    let server_ip = &config.server.endpoints[0].ip;
    let mut env = platform::environment(Some(server_ip));
    env.wifi = platform::wifi();
    record(&mut writer, "environment", &env)?;
    event(
        &mut writer,
        &mut events,
        start,
        "start",
        "info",
        format!(
            "开始测试 {}，计划 {} 秒。",
            config.server.name, config.duration_seconds
        ),
    )?;
    let mut game = config.game_pid.map(|pid| platform::process(pid, "").0);
    let remembered_path = game.as_ref().and_then(|p| p.path.clone());
    let mut game_pid = game.as_ref().map(|p| p.pid);
    let targets = Arc::new(Mutex::new(Vec::<Target>::new()));
    let (tx, rx) = mpsc::channel::<Measurement>();
    let probe_stop = Arc::new(AtomicBool::new(false));
    let route_stop = probe_stop.clone();
    let route_tx = tx.clone();
    let route_ip = server_ip.clone();
    let trace_thread = thread::spawn(move || {
        probe::trace(&route_ip, &route_stop, |h| {
            let _ = route_tx.send(Measurement::Hop(h));
        })
    });
    let active_targets = targets.clone();
    let timer_stop = probe_stop.clone();
    let timer_tx = tx;
    let duration = config.duration_seconds as f64;
    let timer_thread = thread::spawn(move || {
        let mut sweep = 0;
        while !timer_stop.load(Ordering::Relaxed) && start.elapsed().as_secs_f64() < duration {
            let at = Instant::now();
            let list = active_targets.lock().unwrap().clone();
            let mut pending = vec![];
            for t in list {
                let tx = timer_tx.clone();
                let elapsed = start.elapsed().as_secs_f64();
                if elapsed >= duration {
                    break;
                }
                let tcp_due = sweep % 3 == 0;
                pending.push(thread::spawn(move || {
                    let p = probe::icmp(&t.ip, &t.label, &t.role, elapsed);
                    let _ = tx.send(Measurement::Probe(p));
                    if tcp_due {
                        if let Some(port) = t.port {
                            let p =
                                probe::tcp(&t.ip, port, &t.label, start.elapsed().as_secs_f64());
                            let _ = tx.send(Measurement::Probe(p));
                        }
                    }
                }));
            }
            for t in pending {
                let _ = t.join();
            }
            sweep += 1;
            while at.elapsed() < Duration::from_secs(2) && !timer_stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(50));
            }
        }
    });
    let workers = ProbeWorkers {
        stop: probe_stop,
        threads: vec![trace_thread, timer_thread],
    };
    let mut prev_env: Option<(Instant, Environment)> = None;
    let mut totals = HashMap::<(u32, u64), (u64, u64)>::new();
    let mut latest_connections = vec![];
    let mut last_keys = HashSet::new();
    let mut last_endpoints = HashSet::<String>::new();
    let mut known_errors = HashSet::new();
    let mut last_sample = Instant::now();
    let mut sampling_tick = 0u32;
    let mut status = TrafficStatus::default();
    loop {
        let elapsed = start.elapsed().as_secs_f64();
        if elapsed >= duration || stop.load(Ordering::Relaxed) {
            break;
        }
        let sample_start = Instant::now();
        let interval = sample_start.duration_since(last_sample).as_secs_f64();
        if interval > 3.5 {
            event(&mut writer,&mut events,start,"gap","warning",format!("采样间隔延长至 {interval:.1} 秒，可能发生睡眠或调度停顿；此间隔不视作网络超时。"))?;
        }
        while markers.try_recv().is_ok() {
            event(
                &mut writer,
                &mut events,
                start,
                "marker",
                "warning",
                "玩家标记：刚刚卡了／掉线了".into(),
            )?;
        }
        let mut next_env = platform::environment(Some(server_ip));
        next_env.wifi = if sampling_tick.is_multiple_of(15) {
            platform::wifi()
        } else {
            env.wifi.clone()
        };
        if let Some((at, previous)) = &prev_env {
            let secs = sample_start.duration_since(*at).as_secs_f64();
            for a in &mut next_env.adapters {
                if let Some(old) = previous.adapters.iter().find(|p| p.index == a.index) {
                    if a.received >= old.received
                        && a.sent >= old.sent
                        && a.status == old.status
                        && secs < 3.5
                    {
                        a.receive_bps = Some((a.received - old.received) as f64 / secs);
                        a.send_bps = Some((a.sent - old.sent) as f64 / secs);
                    } else {
                        event(
                            &mut writer,
                            &mut events,
                            start,
                            "adapter",
                            "info",
                            format!("网卡 {} 状态变化或计数器重置，本窗口速率不可评估。", a.name),
                        )?;
                    }
                }
            }
            if previous.selected_interface != next_env.selected_interface {
                event(
                    &mut writer,
                    &mut events,
                    start,
                    "route",
                    "warning",
                    "到区服的出接口发生变化。".into(),
                )?;
            }
        }
        prev_env = Some((sample_start, next_env.clone()));
        env = next_env;
        if sampling_tick.is_multiple_of(2) {
            let (connections, errors) = platform::connections();
            latest_connections = connections;
            for e in errors {
                if known_errors.insert(e.clone()) {
                    event(&mut writer, &mut events, start, "capability", "warning", e)?;
                }
            }
        }
        if let Some(selected) = &game {
            let current = platform::process(selected.pid, "").0;
            if current.started != selected.started {
                event(
                    &mut writer,
                    &mut events,
                    start,
                    "game",
                    "info",
                    format!("所选游戏进程 {} 已退出或身份变化。", selected.pid),
                )?;
                game = None;
                game_pid = None;
            }
        }
        if let Some(path) = remembered_path
            .as_ref()
            .filter(|_| game.is_none() && sampling_tick.is_multiple_of(5))
        {
            let matches: Vec<_> = platform::processes()
                .into_iter()
                .filter(|p| {
                    p.path
                        .as_ref()
                        .is_some_and(|s| s.eq_ignore_ascii_case(path))
                        && p.started.is_some()
                })
                .collect();
            if matches.len() == 1 {
                game = Some(matches[0].clone());
                game_pid = Some(matches[0].pid);
                event(
                    &mut writer,
                    &mut events,
                    start,
                    "game",
                    "info",
                    format!("按 EXE 路径重新关联进程 {}。", matches[0].pid),
                )?;
            } else if matches.len() > 1 && known_errors.insert("multiple-game".into()) {
                event(
                    &mut writer,
                    &mut events,
                    start,
                    "game",
                    "warning",
                    "同一路径出现多个进程，已暂停自动关联，避免选错客户端。".into(),
                )?;
            }
        }
        let (raw, new_status) = monitor.sample();
        status = new_status;
        if sampling_tick == 0 {
            event(
                &mut writer,
                &mut events,
                start,
                "capability",
                "info",
                status.message.clone(),
            )?;
        }
        let pids: HashSet<_> = latest_connections
            .iter()
            .map(|c| c.pid)
            .chain(raw.keys().copied())
            .chain(game_pid)
            .collect();
        let mut processes = vec![];
        let mut traffic = vec![];
        for pid in pids {
            let p = if Some(pid) == game_pid {
                cpu.sample(pid)
            } else {
                platform::process(pid, "").0
            };
            if let Some(identity) = p.started.as_ref().and_then(|v| v.parse::<u64>().ok()) {
                let total = totals.entry((pid, identity)).or_default();
                let mut rx_bytes = 0;
                let mut tx_bytes = 0;
                if let Some(bytes) = raw.get(&pid) {
                    if bytes.earliest >= identity {
                        rx_bytes = bytes.received;
                        tx_bytes = bytes.sent;
                        total.0 += rx_bytes;
                        total.1 += tx_bytes;
                    } else if known_errors.insert(format!("stale-{pid}-{identity}")) {
                        event(
                            &mut writer,
                            &mut events,
                            start,
                            "attribution",
                            "info",
                            format!(
                                "PID {pid} 出现早于当前进程创建时间的事件，已排除该批流量归属。"
                            ),
                        )?;
                    }
                }
                if status.available {
                    let secs = interval.max(0.1);
                    let t = Traffic {
                        pid,
                        received: total.0,
                        sent: total.1,
                        receive_bps: rx_bytes as f64 / secs,
                        send_bps: tx_bytes as f64 / secs,
                    };
                    if interval < 3.5
                        && pid != std::process::id()
                        && Some(pid) != game_pid
                        && (t.send_bps > 524_288.0 || t.receive_bps > 2_097_152.0)
                    {
                        activities.push(Activity {
                            elapsed,
                            name: p.name.clone(),
                            pid,
                            send_bps: t.send_bps,
                            receive_bps: t.receive_bps,
                        });
                    }
                    traffic.push(t);
                }
            }
            processes.push(p);
        }
        processes.sort_by_key(|p| (Some(p.pid) != game_pid, p.name.clone()));
        traffic
            .sort_by(|a, b| (b.send_bps + b.receive_bps).total_cmp(&(a.send_bps + a.receive_bps)));
        let current_keys: HashSet<_> = latest_connections
            .iter()
            .map(|c| {
                format!(
                    "{}:{}:{}:{}",
                    c.pid,
                    c.protocol,
                    c.local,
                    c.remote.as_deref().unwrap_or("")
                )
            })
            .collect();
        if sampling_tick > 0 && sampling_tick.is_multiple_of(2) {
            let count = current_keys.difference(&last_keys).count();
            if count >= 50 {
                event(
                    &mut writer,
                    &mut events,
                    start,
                    "connections",
                    "info",
                    format!("本次连接快照发现 {count} 个新端点，仅表示活动变化，不代表恶意连接。"),
                )?;
            }
        }
        last_keys = current_keys;
        let game_endpoints: HashSet<_> = latest_connections
            .iter()
            .filter(|c| Some(c.pid) == game_pid && c.state == "已连接")
            .filter_map(|c| c.remote.clone())
            .collect();
        if game_endpoints != last_endpoints {
            event(
                &mut writer,
                &mut events,
                start,
                "game_connections",
                "info",
                format!(
                    "游戏已连接远端：{}。连接变化可能来自切图。",
                    if game_endpoints.is_empty() {
                        "暂无".into()
                    } else {
                        game_endpoints
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("、")
                    }
                ),
            )?;
            last_endpoints = game_endpoints;
        }
        let mut list = vec![];
        if let Some(a) = env
            .adapters
            .iter()
            .find(|a| Some(a.index) == env.selected_interface)
        {
            if let Some(g) = a
                .gateways
                .iter()
                .find(|ip| ip.parse::<std::net::Ipv4Addr>().is_ok())
            {
                list.push(Target {
                    ip: g.clone(),
                    label: "本地网关".into(),
                    role: "gateway".into(),
                    port: None,
                });
            }
        }
        for (ip, label) in [
            ("223.5.5.5", "公网参照 · 阿里"),
            ("119.29.29.29", "公网参照 · 腾讯"),
        ] {
            list.push(Target {
                ip: ip.into(),
                label: label.into(),
                role: "reference".into(),
                port: None,
            });
        }
        for e in config.server.endpoints.iter().take(4) {
            list.push(Target {
                ip: e.ip.clone(),
                label: config.server.name.clone(),
                role: "server".into(),
                port: Some(e.port),
            });
        }
        let mut candidates: Vec<_> = latest_connections
            .iter()
            .filter(|c| Some(c.pid) == game_pid && c.state == "已连接")
            .filter_map(|c| c.remote_ip.clone())
            .filter(|ip| ip.parse().is_ok_and(catalog::public_v4))
            .collect();
        candidates.sort();
        candidates.dedup();
        let mut count = 0;
        for ip in candidates {
            if count >= 3 {
                break;
            }
            if !list.iter().any(|t| t.ip == ip) {
                list.push(Target {
                    label: format!("游戏远端 {ip}"),
                    ip,
                    role: "game".into(),
                    port: None,
                });
                count += 1;
            }
        }
        *targets.lock().unwrap() = list;
        let tick = Tick {
            at: now(),
            elapsed,
            environment: env.clone(),
            processes,
            connections: latest_connections.clone(),
            traffic,
            traffic_status: status.clone(),
            game_pid,
        };
        record(&mut writer, "tick", &tick)?;
        {
            let mut v = view.lock().unwrap();
            while let Ok(measurement) = rx.try_recv() {
                match measurement {
                    Measurement::Probe(p) => {
                        record(&mut writer, "probe", &p)?;
                        probes.push(p.clone());
                        v.probes.push(p);
                    }
                    Measurement::Hop(h) => {
                        record(&mut writer, "hop", &h)?;
                        v.hops.push(h);
                    }
                }
            }
            if v.probes.len() > 1800 {
                let n = v.probes.len() - 1800;
                v.probes.drain(..n);
            }
            v.elapsed = elapsed;
            v.tick = Some(tick);
            v.events = events
                .iter()
                .rev()
                .take(200)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
        }
        writer.flush().map_err(|e| format!("日志刷新失败：{e}"))?;
        if sampling_tick.is_multiple_of(5) {
            writer
                .get_ref()
                .sync_data()
                .map_err(|e| format!("日志落盘失败：{e}"))?;
        }
        last_sample = sample_start;
        sampling_tick += 1;
        while sample_start.elapsed() < Duration::from_secs(1)
            && !stop.load(Ordering::Relaxed)
            && start.elapsed().as_secs_f64() < duration
        {
            thread::sleep(Duration::from_millis(50));
        }
    }
    let measured = start.elapsed().as_secs_f64().min(duration);
    let outcome = if stop.load(Ordering::Relaxed) {
        "stopped"
    } else {
        "completed"
    };
    {
        view.lock().unwrap().status = "stopping".into();
    }
    drop(workers);
    while let Ok(m) = rx.try_recv() {
        match m {
            Measurement::Probe(p) => {
                record(&mut writer, "probe", &p)?;
                if p.elapsed <= measured {
                    probes.push(p);
                }
            }
            Measurement::Hop(h) => {
                record(&mut writer, "hop", &h)?;
                view.lock().unwrap().hops.push(h);
            }
        }
    }
    while markers.try_recv().is_ok() {
        event(
            &mut writer,
            &mut events,
            start,
            "marker",
            "warning",
            "玩家标记：刚刚卡了／掉线了".into(),
        )?;
    }
    monitor.flush();
    let (_, final_status) = monitor.sample();
    if final_status.events >= status.events {
        status = final_status;
    }
    event(
        &mut writer,
        &mut events,
        start,
        "end",
        "info",
        if outcome == "completed" {
            "测试到时结束，正在生成报告。"
        } else {
            "测试已提前结束，正在生成部分时长报告。"
        }
        .into(),
    )?;
    record(&mut writer, "activities", &activities)?;
    writer.flush().map_err(|e| e.to_string())?;
    writer.get_ref().sync_data().map_err(|e| e.to_string())?;
    let mut limitations=vec!["仅覆盖测试时段与可测目标；ICMP 超时不是游戏丢包率，TCP 建连不是业务延迟。".into(),"中间路由节点不回应不能单独证明链路丢包；无法仅凭单端观测区分去程、回程与服务端内部问题。".into(),"每进程流量是 ETW 观测字节，存在交付延迟；代理、加速器和回环流量可能重复观察，不能简单相加。".into(),"无法读取创建时间的进程不归属流量；短连接、IPv6 主动探测、UDP 远端关联、帧时间与 TCP 重传尚未覆盖。".into(),"本机看不到家庭其他设备占用、光猫光功率及真实宽带带宽上限。".into()];
    limitations.extend(env.notes.clone());
    if status.lost_events > 0 || status.unparsed_events > 0 {
        limitations.push(format!(
            "事件流存在缺失：丢失计数 {}，未解析事件 {}；流量不完整。",
            status.lost_events, status.unparsed_events
        ));
    }
    let report = Report {
        id,
        started_at,
        ended_at: now(),
        duration_seconds: measured,
        requested_seconds: config.duration_seconds,
        server: config.server,
        status: outcome.into(),
        stats: analysis::statistics(&probes),
        findings: analysis::findings(&probes, &activities, &events, measured),
        events: events.clone(),
        traffic_status: status,
        limitations,
        log_dir: dir.to_string_lossy().into(),
    };
    fs::write(dir.join("report.html"), analysis::html(&report))
        .map_err(|e| format!("报告写入失败：{e}"))?;
    fs::write(
        dir.join("report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .map_err(|e| format!("报告写入失败：{e}"))?;
    let mut v = view.lock().unwrap();
    v.status = outcome.into();
    v.elapsed = measured;
    v.events = events;
    v.report = Some(report);
    v.probes = probes
        .into_iter()
        .rev()
        .take(1800)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Ok(())
}
