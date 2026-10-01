use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoint {
    pub ip: String,
    pub port: u16,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub id: String,
    pub area: String,
    pub name: String,
    pub aliases: Vec<String>,
    pub endpoints: Vec<Endpoint>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub servers: Vec<Server>,
    pub source: String,
    pub fetched_at: String,
    pub warning: Option<String>,
}
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub path: Option<String>,
    pub started: Option<String>,
    pub cpu_percent: Option<f64>,
    pub memory_bytes: Option<u64>,
    pub is_game: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub pid: u32,
    pub protocol: String,
    pub local: String,
    pub remote: Option<String>,
    pub remote_ip: Option<String>,
    pub remote_port: Option<u16>,
    pub state: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Adapter {
    pub index: u32,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub status: String,
    pub addresses: Vec<String>,
    pub gateways: Vec<String>,
    pub dns: Vec<String>,
    pub received: u64,
    pub sent: u64,
    pub receive_bps: Option<f64>,
    pub send_bps: Option<f64>,
    pub in_errors: u64,
    pub out_errors: u64,
    pub in_discards: u64,
    pub out_discards: u64,
    pub link_speed: u64,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub adapters: Vec<Adapter>,
    pub selected_interface: Option<u32>,
    pub wifi: Vec<String>,
    pub notes: Vec<String>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Traffic {
    pub pid: u32,
    pub received: u64,
    pub sent: u64,
    pub receive_bps: f64,
    pub send_bps: f64,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficStatus {
    pub available: bool,
    pub message: String,
    pub events: u64,
    pub lost_events: u32,
    pub unparsed_events: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub at: String,
    pub elapsed: f64,
    pub target: String,
    pub label: String,
    pub role: String,
    pub method: String,
    pub status: String,
    pub ms: Option<f64>,
    pub detail: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hop {
    pub ttl: u8,
    pub address: Option<String>,
    pub ms: Option<f64>,
    pub status: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub at: String,
    pub elapsed: f64,
    pub level: String,
    pub kind: String,
    pub message: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tick {
    pub at: String,
    pub elapsed: f64,
    pub environment: Environment,
    pub processes: Vec<ProcessInfo>,
    pub connections: Vec<Connection>,
    pub traffic: Vec<Traffic>,
    pub traffic_status: TrafficStatus,
    pub game_pid: Option<u32>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartConfig {
    pub server: Server,
    pub duration_seconds: u32,
    pub game_pid: Option<u32>,
    pub game_started: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetStats {
    pub target: String,
    pub label: String,
    pub role: String,
    pub method: String,
    pub sent: usize,
    pub success: usize,
    pub timeouts: usize,
    pub errors: usize,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub max: Option<f64>,
    pub jitter: Option<f64>,
    pub longest_timeout_run: usize,
    pub assessable: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub title: String,
    pub level: String,
    pub confidence: String,
    pub start: f64,
    pub end: f64,
    pub evidence: Vec<String>,
    pub suggestion: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub id: String,
    pub started_at: String,
    pub ended_at: String,
    pub duration_seconds: f64,
    pub requested_seconds: u32,
    pub server: Server,
    pub status: String,
    pub stats: Vec<TargetStats>,
    pub findings: Vec<Finding>,
    pub events: Vec<Event>,
    pub traffic_status: TrafficStatus,
    pub limitations: Vec<String>,
    pub log_dir: String,
}
#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub id: Option<String>,
    pub status: String,
    pub elapsed: f64,
    pub duration_seconds: u32,
    pub log_dir: Option<String>,
    pub tick: Option<Tick>,
    pub probes: Vec<Probe>,
    pub events: Vec<Event>,
    pub hops: Vec<Hop>,
    pub report: Option<Report>,
    pub error: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: String,
    pub server_name: String,
    pub started_at: String,
    pub status: String,
    pub duration_seconds: f64,
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
