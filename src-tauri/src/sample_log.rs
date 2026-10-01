//! Store changing topology once, rates per sample, and gzip the raw stream.
use crate::{model::*, traffic::GameFlow};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
};

#[derive(Serialize)]
pub struct Entry {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub data: Value,
}
#[derive(Default)]
pub struct CompactSamples {
    environment: Option<Value>,
    processes: HashMap<u32, Value>,
    connections: HashSet<Connection>,
    flows: HashMap<String, u32>,
}
impl CompactSamples {
    pub fn tick(&mut self, t: &Tick) -> Vec<Entry> {
        let mut result = vec![];
        let environment = json!({"selectedInterface":t.environment.selected_interface,"wifi":t.environment.wifi,"notes":t.environment.notes,"adapters":t.environment.adapters.iter().map(|a|json!({"index":a.index,"name":a.name,"description":a.description,"kind":a.kind,"status":a.status,"addresses":a.addresses,"gateways":a.gateways,"dns":a.dns,"linkSpeed":a.link_speed})).collect::<Vec<_>>()});
        if self.environment.as_ref() != Some(&environment) {
            result.push(Entry {
                kind: "environment_change",
                data: json!({"at":t.at,"elapsed":t.elapsed,"environment":environment}),
            });
            self.environment = Some(environment);
        }
        let mut definitions = vec![];
        let pids = t.processes.iter().map(|p| p.pid).collect::<HashSet<_>>();
        self.processes.retain(|pid, _| pids.contains(pid));
        for p in &t.processes {
            let definition = json!({"pid":p.pid,"started":p.started,"name":p.name,"path":p.path});
            if self.processes.get(&p.pid) != Some(&definition) {
                self.processes.insert(p.pid, definition.clone());
                definitions.push(definition);
            }
        }
        if !definitions.is_empty() {
            result.push(Entry {
                kind: "process_definitions",
                data: json!({"elapsed":t.elapsed,"processes":definitions}),
            });
        }
        let current = t.connections.iter().cloned().collect::<HashSet<_>>();
        let added = current.difference(&self.connections).collect::<Vec<_>>();
        let removed = self.connections.difference(&current).collect::<Vec<_>>();
        if !added.is_empty() || !removed.is_empty() {
            result.push(Entry {
                kind: "connection_changes",
                data: json!({"elapsed":t.elapsed,"added":added,"removed":removed}),
            });
        }
        self.connections = current;
        let tracked = t
            .tracking
            .relays
            .iter()
            .map(|p| p.pid)
            .chain(t.game_pid)
            .collect::<HashSet<_>>();
        let traffic = t
            .traffic
            .iter()
            .filter(|p| p.receive_bps > 0.0 || p.send_bps > 0.0 || tracked.contains(&p.pid))
            .collect::<Vec<_>>();
        let game=t.processes.iter().find(|p|Some(p.pid)==t.game_pid).map(|p|json!({"pid":p.pid,"started":p.started,"cpuPercent":p.cpu_percent,"memoryBytes":p.memory_bytes}));
        let interfaces=t.environment.adapters.iter().filter(|a|a.status=="已连接"||Some(a.index)==t.environment.selected_interface).map(|a|json!({"index":a.index,"receiveBps":a.receive_bps,"sendBps":a.send_bps,"inErrors":a.in_errors,"outErrors":a.out_errors,"inDiscards":a.in_discards,"outDiscards":a.out_discards})).collect::<Vec<_>>();
        result.push(Entry{kind:"sample",data:json!({"at":t.at,"elapsed":t.elapsed,"game":game,"interfaces":interfaces,"processTraffic":traffic,"trafficAvailable":t.traffic_status.available,"events":t.traffic_status.events,"lostEvents":t.traffic_status.lost_events,"unparsedEvents":t.traffic_status.unparsed_events,"endpointErrors":t.traffic_status.endpoint_errors,"primaryEndpoint":t.tracking.primary_id})});
        result
    }
    pub fn flows(&mut self, elapsed: f64, flows: &[GameFlow]) -> Vec<Entry> {
        let mut definitions = vec![];
        let mut bytes = vec![];
        for flow in flows {
            let key = format!(
                "{}|{}|{}|{}|{}",
                flow.pid, flow.protocol, flow.local, flow.ip, flow.port
            );
            let next = self.flows.len() as u32;
            let id=*self.flows.entry(key).or_insert_with(||{definitions.push(json!({"id":next,"pid":flow.pid,"protocol":flow.protocol,"local":flow.local,"ip":flow.ip,"port":flow.port}));next});
            bytes.push(json!({"id":id,"sent":flow.sent,"received":flow.received}));
        }
        let mut result = vec![];
        if !definitions.is_empty() {
            result.push(Entry {
                kind: "flow_definitions",
                data: json!({"elapsed":elapsed,"flows":definitions}),
            });
        }
        if !bytes.is_empty() {
            result.push(Entry {
                kind: "flow_bytes",
                data: json!({"elapsed":elapsed,"flows":bytes}),
            });
        }
        result
    }
}
pub fn compact_existing(input: &Path, output: &Path) -> Result<(u64, u64), String> {
    let input_file = File::open(input).map_err(|e| e.to_string())?;
    let before = input_file.metadata().map_err(|e| e.to_string())?.len();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|e| e.to_string())?;
    let mut writer = BufWriter::new(flate2::write::GzEncoder::new(
        file,
        flate2::Compression::fast(),
    ));
    let mut compact = CompactSamples::default();
    for line in BufReader::new(input_file).lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let v: Value = serde_json::from_str(&line).map_err(|e| format!("原始日志不能解析：{e}"))?;
        let entries = match v["type"].as_str() {
            Some("tick") => compact.tick(
                &serde_json::from_value::<Tick>(v["data"].clone()).map_err(|e| e.to_string())?,
            ),
            Some("game_flows") => compact.flows(
                v["data"]["elapsed"].as_f64().unwrap_or(0.0),
                &serde_json::from_value::<Vec<GameFlow>>(v["data"]["flows"].clone())
                    .map_err(|e| e.to_string())?,
            ),
            _ => {
                serde_json::to_writer(&mut writer, &v).map_err(|e| e.to_string())?;
                writer.write_all(b"\n").map_err(|e| e.to_string())?;
                continue;
            }
        };
        for entry in entries {
            serde_json::to_writer(&mut writer, &entry).map_err(|e| e.to_string())?;
            writer.write_all(b"\n").map_err(|e| e.to_string())?;
        }
    }
    writer.flush().map_err(|e| e.to_string())?;
    let file = writer
        .into_inner()
        .map_err(|e| e.to_string())?
        .finish()
        .map_err(|e| e.to_string())?;
    file.sync_data().map_err(|e| e.to_string())?;
    Ok((before, file.metadata().map_err(|e| e.to_string())?.len()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unchanged_topology_is_not_repeated_and_removals_are_recorded() {
        let c = Connection {
            pid: 1,
            protocol: "TCP4".into(),
            local: "127.0.0.1:1".into(),
            remote: Some("127.0.0.1:2".into()),
            remote_ip: Some("127.0.0.1".into()),
            remote_port: Some(2),
            state: "已连接".into(),
        };
        let mut tick = Tick {
            at: "test".into(),
            elapsed: 0.0,
            environment: Environment::default(),
            processes: vec![],
            connections: vec![c],
            traffic: vec![],
            traffic_status: TrafficStatus::default(),
            game_pid: None,
            tracking: GameTracking::default(),
        };
        let mut compact = CompactSamples::default();
        assert!(compact
            .tick(&tick)
            .iter()
            .any(|e| e.kind == "connection_changes"));
        tick.elapsed = 1.0;
        assert_eq!(compact.tick(&tick).len(), 1);
        tick.connections.clear();
        let removed = compact.tick(&tick);
        assert_eq!(
            removed
                .iter()
                .find(|e| e.kind == "connection_changes")
                .unwrap()
                .data["removed"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
