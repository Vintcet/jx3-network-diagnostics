#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod analysis;
mod catalog;
mod model;
mod platform;
mod probe;
mod report_text;
mod sample_log;
mod session;
mod tracking;
mod traffic;

use model::*;
use session::Manager;
use tauri::{Manager as _, State};

#[tauri::command]
async fn get_catalog(state: State<'_, Manager>, refresh: bool) -> Result<Catalog, String> {
    let root = state.root.clone();
    tauri::async_runtime::spawn_blocking(move || catalog::load(&root, refresh))
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn get_processes() -> Result<Vec<ProcessInfo>, String> {
    tauri::async_runtime::spawn_blocking(platform::processes)
        .await
        .map_err(|e| e.to_string())
}
#[tauri::command]
async fn get_environment() -> Result<Environment, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut e = platform::environment(Some("223.5.5.5"));
        e.wifi = platform::wifi();
        e
    })
    .await
    .map_err(|e| e.to_string())
}
#[tauri::command]
async fn inspect_game(
    state: State<'_, Manager>,
    pid: u32,
    started: String,
) -> Result<serde_json::Value, String> {
    let root = state.root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let p = platform::process(pid, "").0;
        if p.started.as_deref() != Some(started.as_str()) {
            return Err("进程身份已变化，请刷新进程列表".into());
        }
        let (connections, errors) = platform::connections();
        let catalog = catalog::load(&root, false);
        let matches = tracking::match_servers(pid, &connections, &catalog.servers);
        let relays = tracking::relay_pids(pid, &connections)
            .into_iter()
            .map(|pid| platform::process(pid, "").0)
            .collect::<Vec<_>>();
        Ok(serde_json::json!({"matches":matches,"relays":relays,"errors":errors}))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn start_test(state: State<'_, Manager>, config: StartConfig) -> Result<String, String> {
    state.start(config, false)
}
#[tauri::command]
fn stop_test(state: State<'_, Manager>) {
    state.stop();
}
#[tauri::command]
fn mark_event(state: State<'_, Manager>) -> Result<(), String> {
    state.mark()
}
#[tauri::command]
fn get_session(state: State<'_, Manager>) -> SessionView {
    state.view.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
#[tauri::command]
fn get_history(state: State<'_, Manager>) -> Vec<HistoryItem> {
    state.history()
}
#[tauri::command]
async fn get_report(state: State<'_, Manager>, id: String) -> Result<Report, String> {
    let root = state.root.clone();
    tauri::async_runtime::spawn_blocking(move || Manager::new(root).report(&id))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn export_report(state: State<'_, Manager>, id: String) -> Result<String, String> {
    let root = state.root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        Manager::new(root)
            .export_report(&id)
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn open_logs(state: State<'_, Manager>, id: String) -> Result<(), String> {
    let dir = state.directory(&id)?;
    std::process::Command::new("explorer.exe")
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn main() {
    let utility_args = std::env::args().collect::<Vec<_>>();
    if let Some(input) = utility_args
        .windows(2)
        .find(|a| a[0] == "--compact-log")
        .map(|a| &a[1])
    {
        let Some(output) = utility_args
            .windows(2)
            .find(|a| a[0] == "--output")
            .map(|a| &a[1])
        else {
            eprintln!("需要 --output 路径");
            std::process::exit(1);
        };
        match sample_log::compact_existing(
            std::path::Path::new(input),
            std::path::Path::new(output),
        ) {
            Ok((before, after)) => println!(
                "{}",
                serde_json::json!({"beforeBytes":before,"afterBytes":after})
            ),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if let Some(id) = utility_args
        .windows(2)
        .find(|a| a[0] == "--export-report")
        .map(|a| &a[1])
    {
        let root = std::path::PathBuf::from(std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA"))
            .join("com.jx3.network-diagnostics");
        match Manager::new(root).export_report(id) {
            Ok(path) => println!("{}", path.display()),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|a| a == "--self-check") {
        let root = std::env::current_dir()
            .unwrap()
            .join(".tmp")
            .join("native-check");
        let manager = Manager::new(root.clone());
        let args: Vec<_> = std::env::args().collect();
        let seconds = args
            .windows(2)
            .find(|a| a[0] == "--seconds")
            .and_then(|a| a[1].parse::<u32>().ok())
            .unwrap_or(8)
            .clamp(2, 60);
        let catalog = catalog::load(&root, false);
        let requested_server = args
            .windows(2)
            .find(|a| a[0] == "--server")
            .map(|a| a[1].as_str())
            .unwrap_or("绝代天骄");
        let selected = catalog
            .servers
            .iter()
            .find(|s| s.name == requested_server)
            .unwrap_or(&catalog.servers[0])
            .clone();
        let selected_pid = args
            .windows(2)
            .find(|a| a[0] == "--game-pid")
            .and_then(|a| a[1].parse().ok())
            .unwrap_or(std::process::id());
        let p = platform::process(selected_pid, "").0;
        let config = StartConfig {
            server: selected,
            duration_seconds: seconds,
            game_pid: Some(p.pid),
            game_started: p.started,
        };
        match manager.start(config, true) {
            Ok(id) => {
                std::thread::sleep(std::time::Duration::from_millis(1200));
                let _ = manager.mark();
                if args.iter().any(|a| a == "--stop-early") {
                    manager.stop();
                }
                manager.wait();
                let v = manager.view.lock().unwrap();
                println!("{}",serde_json::to_string_pretty(&serde_json::json!({"id":id,"status":v.status,"error":v.error,"logDir":v.log_dir,"report":v.report,"lastTick":v.tick})).unwrap());
                if v.error.is_some() {
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    tauri::Builder::default()
        .setup(|app| {
            let root = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&root)?;
            app.manage(Manager::new(root));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_catalog,
            get_processes,
            get_environment,
            inspect_game,
            start_test,
            stop_test,
            mark_event,
            get_session,
            get_history,
            get_report,
            export_report,
            open_logs
        ])
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                window.state::<Manager>().stop();
                window.state::<Manager>().wait();
            }
        })
        .run(tauri::generate_context!())
        .expect("无法启动桌面应用");
}
