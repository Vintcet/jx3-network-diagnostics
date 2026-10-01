//! Follow observed game endpoints; endpoint changes do not prove a physical-server change.
use crate::{catalog::public_v4, model::*, traffic::GameFlow};
use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    net::Ipv4Addr,
};

const ACTIVE_SECONDS: f64 = 6.0;
const RETAIN_SECONDS: f64 = 20.0;
const MAX_ENDPOINTS: usize = 512;
const MAX_PROBE_IPS: usize = 6;
pub fn relay_pids(game_pid: u32, connections: &[Connection]) -> Vec<u32> {
    let mut pids = HashSet::new();
    for c in connections.iter().filter(|c| {
        c.pid == game_pid
            && c.state == "已连接"
            && c.remote_ip
                .as_ref()
                .and_then(|ip| ip.parse::<std::net::IpAddr>().ok())
                .is_some_and(|ip| ip.is_loopback())
    }) {
        for peer in connections.iter().filter(|p| {
            p.pid != game_pid
                && p.protocol == c.protocol
                && Some(&p.local) == c.remote.as_ref()
                && p.remote.as_ref() == Some(&c.local)
        }) {
            pids.insert(peer.pid);
        }
    }
    let mut result = pids.into_iter().collect::<Vec<_>>();
    result.sort_unstable();
    result.truncate(8);
    result
}
pub fn match_servers(game_pid: u32, connections: &[Connection], catalog: &[Server]) -> Vec<Server> {
    catalog
        .iter()
        .filter(|s| {
            s.endpoints.iter().any(|e| {
                connections.iter().any(|c| {
                    c.pid == game_pid
                        && c.state == "已连接"
                        && c.remote_ip.as_ref() == Some(&e.ip)
                        && c.remote_port == Some(e.port)
                })
            })
        })
        .cloned()
        .collect()
}
struct Observed {
    endpoint: GameEndpoint,
    identity: String,
    connected: bool,
    activity: VecDeque<(f64, u64, u64)>,
}
pub struct GameTracker {
    endpoints: BTreeMap<String, Observed>,
    catalog: Vec<Server>,
    primary: Option<String>,
    pending: Option<(String, u8)>,
    pub transitions: Vec<TargetTransition>,
    pub snapshot: GameTracking,
    relays: Vec<ProcessInfo>,
    pub connection_changes: Vec<EndpointChange>,
    pub changes_in_tick: Vec<EndpointChange>,
    capped: bool,
}
impl GameTracker {
    pub fn new(catalog: Vec<Server>) -> Self {
        Self {
            endpoints: BTreeMap::new(),
            catalog,
            primary: None,
            pending: None,
            transitions: vec![],
            snapshot: GameTracking::default(),
            relays: vec![],
            connection_changes: vec![],
            changes_in_tick: vec![],
            capped: false,
        }
    }
    pub fn set_relays(&mut self, relays: Vec<ProcessInfo>) {
        self.relays = relays;
    }
    fn observe(
        &mut self,
        identity: &str,
        protocol: &str,
        ip: &str,
        port: u16,
        elapsed: f64,
    ) -> Option<&mut Observed> {
        if port == 0 || ip.parse::<std::net::IpAddr>().is_err() {
            return None;
        }
        let id = format!("{identity}/{protocol}/{ip}/{port}");
        if !self.endpoints.contains_key(&id) && self.endpoints.len() >= MAX_ENDPOINTS {
            self.capped = true;
            // Keep active entries. Historical entries are already preserved by the JSONL log.
            let expired = self
                .endpoints
                .iter()
                .filter(|(_, v)| v.endpoint.state == "inactive")
                .min_by(|a, b| a.1.endpoint.last_seen.total_cmp(&b.1.endpoint.last_seen))
                .map(|(id, _)| id.clone());
            if let Some(id) = expired {
                self.endpoints.remove(&id);
            } else {
                return None;
            }
        }
        let names = self
            .catalog
            .iter()
            .filter(|s| s.endpoints.iter().any(|e| e.ip == ip && e.port == port))
            .map(|s| format!("{} / {}", s.area, s.name))
            .collect();
        let item = self
            .endpoints
            .entry(id.clone())
            .or_insert_with(|| Observed {
                endpoint: GameEndpoint {
                    id,
                    ip: ip.into(),
                    port,
                    protocol: protocol.into(),
                    first_seen: elapsed,
                    last_seen: elapsed,
                    catalog_names: names,
                    ..Default::default()
                },
                identity: identity.into(),
                connected: false,
                activity: VecDeque::new(),
            });
        item.endpoint.last_seen = elapsed;
        Some(item)
    }
    pub fn update(
        &mut self,
        elapsed: f64,
        game: Option<&ProcessInfo>,
        connections: &[Connection],
        flows: &[GameFlow],
        traffic_available: bool,
    ) -> Option<TargetTransition> {
        let identity = game.and_then(|p| p.started.as_ref().map(|s| format!("{}:{s}", p.pid)));
        let previous = self
            .endpoints
            .iter()
            .map(|(id, item)| (id.clone(), (item.endpoint.state.clone(), item.connected)))
            .collect::<BTreeMap<_, _>>();
        self.changes_in_tick.clear();
        for item in self.endpoints.values_mut() {
            item.connected = false;
            item.endpoint.probing = false;
        }
        let subjects = game
            .into_iter()
            .cloned()
            .chain(self.relays.iter().cloned())
            .collect::<Vec<_>>();
        let identities = subjects
            .iter()
            .filter_map(|p| p.started.as_ref().map(|s| format!("{}:{s}", p.pid)))
            .collect::<HashSet<_>>();
        for subject in subjects {
            let Some(identity) = subject
                .started
                .as_ref()
                .map(|s| format!("{}:{s}", subject.pid))
            else {
                continue;
            };
            let source = if game.is_some_and(|p| p.pid == subject.pid) {
                "game"
            } else {
                "relay"
            };
            for c in connections
                .iter()
                .filter(|c| c.pid == subject.pid && c.state == "已连接")
            {
                if let (Some(ip), Some(port)) = (&c.remote_ip, c.remote_port) {
                    if let Some(item) = self.observe(&identity, &c.protocol, ip, port, elapsed) {
                        item.connected = true;
                        item.endpoint.source = source.into();
                        item.endpoint.process_name = subject.name.clone();
                        item.endpoint.pid = subject.pid;
                    }
                }
            }
            for flow in flows
                .iter()
                .filter(|f| f.pid == subject.pid && f.sent + f.received > 0)
            {
                if let Some(item) =
                    self.observe(&identity, &flow.protocol, &flow.ip, flow.port, elapsed)
                {
                    item.endpoint.source = source.into();
                    item.endpoint.process_name = subject.name.clone();
                    item.endpoint.pid = subject.pid;
                    item.endpoint.sent += flow.sent;
                    item.endpoint.received += flow.received;
                    item.endpoint.last_active = Some(elapsed);
                    if let Some(last) = item
                        .activity
                        .back_mut()
                        .filter(|last| (last.0 - elapsed).abs() < 0.001)
                    {
                        last.1 += flow.sent;
                        last.2 += flow.received;
                    } else {
                        item.activity.push_back((elapsed, flow.sent, flow.received));
                    }
                }
            }
        }
        let mut ranked = vec![];
        for (id, item) in &mut self.endpoints {
            while item
                .activity
                .front()
                .is_some_and(|a| elapsed - a.0 > ACTIVE_SECONDS)
            {
                item.activity.pop_front();
            }
            let current = identities.contains(&item.identity);
            let (sent, received) = item
                .activity
                .iter()
                .fold((0u64, 0u64), |(a, b), (_, s, r)| (a + s, b + r));
            let active = current && sent + received > 0;
            item.endpoint.state = if active {
                "active"
            } else if current && item.connected {
                "connected"
            } else if elapsed - item.endpoint.last_seen <= RETAIN_SECONDS {
                "recent"
            } else {
                "inactive"
            }
            .into();
            let mut score = if !current {
                0.0
            } else if active {
                100.0
                    + if sent > 0 && received > 0 { 30.0 } else { 0.0 }
                    + item.activity.len() as f64 * 3.0
                    + ((sent + received) as f64).log2().min(20.0)
            } else if item.connected {
                20.0
            } else {
                0.0
            };
            // Internal loopback traffic must not hide visible game/relay internet endpoints.
            if score > 0.0 && item.endpoint.ip.parse::<Ipv4Addr>().is_ok_and(public_v4) {
                score += 200.0;
            }
            let kind = match previous.get(id) {
                None => Some("first_seen"),
                Some((_, false)) if item.connected => Some("resumed"),
                Some((old, _)) if old == "recent" && item.endpoint.state == "active" => {
                    Some("resumed")
                }
                Some((old, _)) if old == "inactive" && item.endpoint.state != "inactive" => {
                    Some("resumed")
                }
                Some((_, true)) if !item.connected && item.endpoint.protocol.starts_with("TCP") => {
                    Some("left_table")
                }
                Some((old, _))
                    if old == "active"
                        && item.endpoint.state == "recent"
                        && item.endpoint.protocol.starts_with("UDP") =>
                {
                    Some("quiet")
                }
                _ => None,
            };
            if let Some(kind) = kind {
                self.changes_in_tick.push(EndpointChange {
                    elapsed,
                    kind: kind.into(),
                    endpoint: item.endpoint.clone(),
                });
            }
            ranked.push((
                id.clone(),
                score,
                item.endpoint
                    .last_active
                    .unwrap_or(item.endpoint.first_seen),
            ));
        }
        ranked.sort_by(|a, b| {
            b.1.total_cmp(&a.1)
                .then(b.2.total_cmp(&a.2))
                .then(a.0.cmp(&b.0))
        });
        self.connection_changes.extend(self.changes_in_tick.clone());
        let candidate = ranked.first().filter(|e| e.1 > 0.0).map(|e| e.0.clone());
        let mut next = self.primary.clone();
        let old_score = ranked
            .iter()
            .find(|e| Some(&e.0) == self.primary.as_ref())
            .map(|e| e.1)
            .unwrap_or(0.0);
        match candidate {
            None => {
                next = None;
                self.pending = None;
            }
            Some(candidate) if Some(&candidate) == self.primary.as_ref() => {
                self.pending = None;
            }
            Some(candidate) => {
                let new_score = ranked.iter().find(|e| e.0 == candidate).unwrap().1;
                if old_score == 0.0 || self.primary.is_none() {
                    next = Some(candidate);
                    self.pending = None;
                } else if new_score > old_score + 4.0 || !traffic_available {
                    let count = self
                        .pending
                        .as_ref()
                        .filter(|p| p.0 == candidate)
                        .map(|p| p.1 + 1)
                        .unwrap_or(1);
                    if count >= 2 {
                        next = Some(candidate);
                        self.pending = None;
                    } else {
                        self.pending = Some((candidate, count));
                    }
                } else {
                    self.pending = None;
                }
            }
        }
        let changed = if next != self.primary {
            let describe = |id: &String| {
                self.endpoints
                    .get(id)
                    .map(|e| {
                        format!(
                            "{} {}:{}",
                            e.endpoint.protocol, e.endpoint.ip, e.endpoint.port
                        )
                    })
                    .unwrap_or_else(|| id.clone())
            };
            let transition=TargetTransition {elapsed,from:self.primary.as_ref().map(describe),to:next.as_ref().map(describe),reason:if traffic_available {"按近期收发活动调整重点连接；可能来自切图、跨服或其他连接活动，不能据此确认物理服务器。"}else{"按连接表调整重点连接；缺少连接级流量证据，业务角色未知。"}.into()};
            self.transitions.push(transition.clone());
            self.primary = next;
            Some(transition)
        } else {
            None
        };
        let primary_ip = self
            .primary
            .as_ref()
            .and_then(|id| self.endpoints.get(id))
            .map(|e| e.endpoint.ip.clone());
        let mut eligible = vec![];
        let mut seen = HashSet::new();
        if let Some(ip) = primary_ip
            .as_ref()
            .filter(|ip| ip.parse::<Ipv4Addr>().is_ok_and(public_v4))
        {
            eligible.push(ip.clone());
            seen.insert(ip.clone());
        }
        for (id, _, _) in &ranked {
            let e = &self.endpoints[id].endpoint;
            if e.state != "inactive"
                && e.ip.parse::<Ipv4Addr>().is_ok_and(public_v4)
                && seen.insert(e.ip.clone())
            {
                eligible.push(e.ip.clone());
            }
        }
        let mut probe_ips = eligible.iter().take(4).cloned().collect::<Vec<_>>();
        if eligible.len() > 4 {
            let tail = &eligible[4..];
            let offset = ((elapsed / 2.0) as usize) % tail.len();
            for i in 0..(MAX_PROBE_IPS - probe_ips.len()).min(tail.len()) {
                probe_ips.push(tail[(offset + i) % tail.len()].clone());
            }
        }
        for item in self.endpoints.values_mut() {
            item.endpoint.probing =
                item.endpoint.state != "inactive" && probe_ips.contains(&item.endpoint.ip);
        }
        let message = if identity.is_none() {
            "尚未关联游戏进程，当前使用所选区服作为网络参照。"
        } else if self.primary.is_none() {
            "等待游戏建立连接；所选区服持续作为参照。"
        } else if probe_ips.is_empty() {
            "已发现游戏连接，但当前远端为本地代理、私网或 IPv6；无法直接探测背后的跨服地址。"
        } else if !self.relays.is_empty() {
            "已沿本地连接发现中转进程，正在观察其上游候选。候选可能包含控制连接；加速节点之后的真实游戏服务器仍可能不可见。"
        } else if !traffic_available {
            "正在跟随游戏 TCP 连接；连接级流量不可用，UDP 远端暂无法发现，业务角色未知。"
        } else {
            "正在按游戏收发活动跟随目标；目录外地址也会加入观测，重点连接不代表已确认的地图服务器。"
        };
        self.snapshot = GameTracking {
            primary_id: self.primary.clone(),
            primary_ip,
            probe_ips,
            endpoints: ranked
                .iter()
                .map(|(id, _, _)| self.endpoints[id].endpoint.clone())
                .collect(),
            message: format!(
                "{message}{}",
                if self.capped {
                    " 连接历史达到上限，部分早期端点仅保存在原始日志。"
                } else {
                    ""
                }
            ),
            relays: self.relays.clone(),
        };
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn game() -> ProcessInfo {
        ProcessInfo {
            pid: 10,
            started: Some("100".into()),
            ..Default::default()
        }
    }
    fn connection(ip: &str, port: u16) -> Connection {
        Connection {
            pid: 10,
            protocol: "TCP4".into(),
            local: "192.168.0.2:5000".into(),
            remote: Some(format!("{ip}:{port}")),
            remote_ip: Some(ip.into()),
            remote_port: Some(port),
            state: "已连接".into(),
        }
    }
    fn flow(ip: &str, port: u16) -> GameFlow {
        GameFlow {
            pid: 10,
            protocol: "TCP4".into(),
            ip: ip.into(),
            port,
            sent: 1024,
            received: 4096,
            ..Default::default()
        }
    }
    #[test]
    fn discovers_generic_relay_by_reverse_socket_not_brand_name() {
        let mut local = connection("127.0.0.1", 1111);
        local.local = "127.0.0.1:5555".into();
        let mut peer = local.clone();
        peer.pid = 20;
        peer.local = "127.0.0.1:1111".into();
        peer.remote = Some("127.0.0.1:5555".into());
        peer.remote_port = Some(5555);
        let mut upstream = connection("8.8.8.8", 443);
        upstream.pid = 20;
        let cs = vec![local, peer, upstream];
        assert_eq!(relay_pids(10, &cs), vec![20]);
        let mut tracker = GameTracker::new(vec![]);
        tracker.set_relays(vec![ProcessInfo {
            pid: 20,
            started: Some("120".into()),
            name: "generic-proxy.exe".into(),
            ..Default::default()
        }]);
        tracker.update(0.0, Some(&game()), &cs, &[], false);
        let focused = tracker
            .snapshot
            .endpoints
            .iter()
            .find(|e| Some(&e.id) == tracker.snapshot.primary_id.as_ref())
            .unwrap();
        assert_eq!(focused.source, "relay");
        assert_eq!(focused.ip, "8.8.8.8");
        assert!(tracker.snapshot.message.contains("候选"));
    }
    #[test]
    fn server_auto_match_requires_game_owned_endpoint_and_port() {
        let catalog = vec![Server {
            id: "x".into(),
            area: "area".into(),
            name: "server".into(),
            aliases: vec![],
            endpoints: vec![Endpoint {
                ip: "8.8.8.8".into(),
                port: 3724,
            }],
        }];
        assert_eq!(
            match_servers(10, &[connection("8.8.8.8", 3724)], &catalog).len(),
            1
        );
        assert!(match_servers(10, &[connection("8.8.8.8", 443)], &catalog).is_empty());
        let mut proxy = connection("8.8.8.8", 3724);
        proxy.pid = 20;
        assert!(match_servers(10, &[proxy], &catalog).is_empty());
    }
    #[test]
    fn records_all_new_addresses_not_only_primary_changes() {
        let mut t = GameTracker::new(vec![]);
        t.update(0.0, Some(&game()), &[connection("8.8.8.8", 1)], &[], false);
        t.update(
            10.0,
            Some(&game()),
            &[
                connection("8.8.8.8", 1),
                connection("1.1.1.1", 2),
                connection("1.1.1.1", 3),
            ],
            &[],
            false,
        );
        assert_eq!(
            t.changes_in_tick
                .iter()
                .filter(|c| c.kind == "first_seen")
                .count(),
            2
        );
        assert!(t.changes_in_tick.iter().all(|c| c.elapsed == 10.0));
    }
    #[test]
    fn records_return_to_an_address_during_retention() {
        let mut t = GameTracker::new(vec![]);
        t.update(0.0, Some(&game()), &[connection("8.8.8.8", 1)], &[], false);
        t.update(2.0, Some(&game()), &[connection("1.1.1.1", 2)], &[], false);
        t.update(4.0, Some(&game()), &[connection("8.8.8.8", 1)], &[], false);
        assert!(t
            .changes_in_tick
            .iter()
            .any(|c| c.kind == "resumed" && c.endpoint.ip == "8.8.8.8"));
    }
    #[test]
    fn follows_unknown_endpoint_without_catalog_membership() {
        let mut t = GameTracker::new(vec![]);
        let a = connection("8.8.8.8", 3724);
        t.update(0.0, Some(&game()), &[a], &[flow("8.8.8.8", 3724)], true);
        assert_eq!(t.snapshot.primary_ip.as_deref(), Some("8.8.8.8"));
        t.update(
            8.0,
            Some(&game()),
            &[connection("1.1.1.1", 9999)],
            &[flow("1.1.1.1", 9999)],
            true,
        );
        assert_eq!(t.snapshot.primary_ip.as_deref(), Some("1.1.1.1"));
        assert!(t.snapshot.probe_ips.contains(&"8.8.8.8".into()));
        t.update(
            30.0,
            Some(&game()),
            &[connection("1.1.1.1", 9999)],
            &[],
            false,
        );
        assert!(!t.snapshot.probe_ips.contains(&"8.8.8.8".into()));
        assert!(t.snapshot.endpoints.iter().any(|e| e.ip == "8.8.8.8"));
    }
    #[test]
    fn same_ip_different_port_is_separate_connection() {
        let mut t = GameTracker::new(vec![]);
        t.update(
            0.0,
            Some(&game()),
            &[connection("8.8.8.8", 3724)],
            &[],
            false,
        );
        t.update(
            2.0,
            Some(&game()),
            &[connection("8.8.8.8", 8888)],
            &[],
            false,
        );
        assert_eq!(t.snapshot.endpoints.len(), 2);
        assert_eq!(t.snapshot.probe_ips.len(), 1);
        assert!(t
            .transitions
            .last()
            .unwrap()
            .to
            .as_ref()
            .unwrap()
            .contains("8888"));
    }
    #[test]
    fn udp_flow_is_discovered_without_a_tcp_row() {
        let mut t = GameTracker::new(vec![]);
        let mut f = flow("1.1.1.1", 9988);
        f.protocol = "UDP4".into();
        t.update(0.0, Some(&game()), &[], &[f], true);
        assert_eq!(t.snapshot.endpoints[0].protocol, "UDP4");
        assert_eq!(t.snapshot.probe_ips, vec!["1.1.1.1"]);
    }
    #[test]
    fn local_proxy_is_observed_but_not_probed() {
        let mut t = GameTracker::new(vec![]);
        t.update(
            0.0,
            Some(&game()),
            &[connection("127.0.0.1", 5000)],
            &[],
            false,
        );
        assert!(t.snapshot.probe_ips.is_empty());
        assert!(t.snapshot.message.contains("代理"));
    }
    #[test]
    fn new_high_activity_target_is_not_excluded_by_sort_order() {
        let mut t = GameTracker::new(vec![]);
        let mut cs = (1..10)
            .map(|i| connection(&format!("8.8.8.{i}"), 3724))
            .collect::<Vec<_>>();
        cs.push(connection("209.85.1.1", 3724));
        t.update(0.0, Some(&game()), &cs, &[flow("209.85.1.1", 3724)], true);
        assert_eq!(t.snapshot.primary_ip.as_deref(), Some("209.85.1.1"));
        assert!(t.snapshot.probe_ips.contains(&"209.85.1.1".into()));
        assert!(t.snapshot.probe_ips.len() <= MAX_PROBE_IPS);
    }
    #[test]
    fn pid_reuse_does_not_retain_previous_primary() {
        let mut t = GameTracker::new(vec![]);
        t.update(0.0, Some(&game()), &[connection("8.8.8.8", 1)], &[], false);
        let mut replacement = game();
        replacement.started = Some("200".into());
        t.update(
            1.0,
            Some(&replacement),
            &[connection("1.1.1.1", 1)],
            &[],
            false,
        );
        assert_eq!(t.snapshot.primary_ip.as_deref(), Some("1.1.1.1"));
    }
}
