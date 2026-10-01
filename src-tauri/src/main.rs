#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod analysis;
mod catalog;
mod model;
mod platform;
mod probe;
mod session;
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
fn get_report(state: State<'_, Manager>, id: String) -> Result<Report, String> {
    state.report(&id)
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
        let selected = catalog
            .servers
            .iter()
            .find(|s| s.name == "绝代天骄")
            .unwrap_or(&catalog.servers[0])
            .clone();
        let p = platform::process(std::process::id(), "").0;
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
            start_test,
            stop_test,
            mark_event,
            get_session,
            get_history,
            get_report,
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
