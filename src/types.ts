export interface Endpoint { ip: string; port: number }
export interface Server { id: string; name: string; area: string; aliases: string[]; endpoints: Endpoint[] }
export interface Catalog { servers: Server[]; source: string; fetchedAt: string; warning: string | null }
export interface ProcessInfo { pid: number; name: string; path: string | null; started: string | null; cpuPercent: number | null; memoryBytes: number | null; isGame: boolean }
export interface Connection { pid: number; protocol: string; local: string; remote: string | null; remoteIp: string | null; remotePort: number | null; state: string }
export interface Adapter { index: number; name: string; description: string; kind: string; status: string; addresses: string[]; gateways: string[]; dns: string[]; received: number; sent: number; receiveBps: number | null; sendBps: number | null; inErrors: number; outErrors: number; inDiscards: number; outDiscards: number; linkSpeed: number }
export interface Environment { adapters: Adapter[]; selectedInterface: number | null; wifi: string[]; notes: string[] }
export interface Traffic { pid: number; received: number; sent: number; receiveBps: number; sendBps: number }
export interface TrafficStatus { available: boolean; message: string; events: number; lostEvents: number; unparsedEvents: number; endpointEvents?: number; endpointErrors?: number }
export interface Probe { at: string; elapsed: number; target: string; label: string; role: string; method: string; status: string; ms: number | null; detail: string | null }
export interface Hop { target: string; ttl: number; address: string | null; ms: number | null; status: string }
export interface Event { at: string; elapsed: number; level: string; kind: string; message: string }
export interface GameEndpoint { id: string; ip: string; port: number; protocol: string; firstSeen: number; lastSeen: number; lastActive: number | null; sent: number; received: number; state: string; probing: boolean; catalogNames: string[]; source: string; processName: string; pid: number }
export interface GameTracking { primaryId: string | null; primaryIp: string | null; endpoints: GameEndpoint[]; probeIps: string[]; message: string; relays: ProcessInfo[] }
export interface EndpointChange { elapsed: number; kind: string; endpoint: GameEndpoint }
export interface TargetTransition { elapsed: number; from: string | null; to: string | null; reason: string }
export interface GameContext { matches: Server[]; relays: ProcessInfo[]; errors: string[] }
export interface Tick { at: string; elapsed: number; environment: Environment; processes: ProcessInfo[]; connections: Connection[]; traffic: Traffic[]; trafficStatus: TrafficStatus; gamePid: number | null; tracking: GameTracking }
export interface TargetStats { target: string; label: string; role: string; method: string; sent: number; success: number; timeouts: number; errors: number; p50: number | null; p95: number | null; max: number | null; jitter: number | null; longestTimeoutRun: number; assessable: boolean; thresholdMs: number | null }
export interface Finding { title: string; level: string; confidence: string; start: number; end: number; evidence: string[]; suggestion: string }
export interface Report { id: string; startedAt: string; endedAt: string; durationSeconds: number; requestedSeconds: number; server: Server; status: string; stats: TargetStats[]; findings: Finding[]; events: Event[]; trafficStatus: TrafficStatus; limitations: string[]; logDir: string; gameEndpoints: GameEndpoint[]; transitions: TargetTransition[]; connectionChanges: EndpointChange[]; relayProcesses: ProcessInfo[]; summary: { headline: string; facts: string[]; nextStep: string }; rawLogBytes: number; logFormat: string }
export interface SessionView { id: string | null; status: string; elapsed: number; durationSeconds: number; logDir: string | null; tick: Tick | null; probes: Probe[]; events: Event[]; hops: Hop[]; report: Report | null; error: string | null }
export interface HistoryItem { id: string; serverName: string; startedAt: string; status: string; durationSeconds: number }
