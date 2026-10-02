use crate::{model::StartConfig, session::Manager};
use serde::Serialize;
use std::{sync::Mutex, time::Duration};
use tauri::{ipc::Channel, AppHandle, State};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
struct Gate {
    checking: bool,
    installing: bool,
    pending: Option<Update>,
}

#[derive(Default)]
pub struct UpdateState(Mutex<Gate>);

// Starting a test and starting an installation share a lock, including the
// transition into the running state. A stale frontend poll cannot bypass it.
impl UpdateState {
    pub fn start_test(&self, manager: &Manager, config: StartConfig) -> Result<String, String> {
        let gate = self.0.lock().map_err(|e| e.to_string())?;
        if gate.installing {
            return Err("正在安装更新，请等待工具重新启动".into());
        }
        manager.start(config, false)
    }
}

struct ResetBusy<'a>(&'a UpdateState, bool);
impl Drop for ResetBusy<'_> {
    fn drop(&mut self) {
        let mut gate = self.0 .0.lock().unwrap_or_else(|e| e.into_inner());
        if self.1 {
            gate.installing = false;
        } else {
            gate.checking = false;
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    notes: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    phase: &'static str,
    downloaded: u64,
    total: Option<u64>,
}

#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    state: State<'_, UpdateState>,
) -> Result<Option<UpdateInfo>, String> {
    {
        let mut gate = state.0.lock().map_err(|e| e.to_string())?;
        if gate.checking || gate.installing {
            return Err("更新操作正在进行，请稍后再试".into());
        }
        gate.checking = true;
        gate.pending = None;
    }
    let _reset = ResetBusy(&state, false);
    let mut pending = app
        .updater_builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| format!("无法连接 GitHub 更新服务，请稍后重试。{e}"))?;
    if let Some(update) = pending.as_mut() {
        update.timeout = Some(Duration::from_secs(600));
    }
    let info = pending.as_ref().map(|update| UpdateInfo {
        version: update.version.clone(),
        notes: update.body.clone().unwrap_or_default(),
    });
    state.0.lock().map_err(|e| e.to_string())?.pending = pending;
    Ok(info)
}

fn ensure_can_install(status: &str, busy: bool) -> Result<(), String> {
    if busy {
        return Err("更新操作正在进行，请稍后再试".into());
    }
    if matches!(status, "running" | "stopping") {
        return Err("请先结束测试并等待报告保存，再安装更新".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    state: State<'_, UpdateState>,
    manager: State<'_, Manager>,
    on_progress: Channel<Progress>,
) -> Result<(), String> {
    let update = {
        let mut gate = state.0.lock().map_err(|e| e.to_string())?;
        let view = manager.view.lock().map_err(|e| e.to_string())?;
        ensure_can_install(&view.status, gate.checking || gate.installing)?;
        let update = gate.pending.clone().ok_or("请先检查更新")?;
        gate.installing = true;
        update
    };
    let _reset = ResetBusy(&state, true);
    let mut downloaded = 0;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = on_progress.send(Progress {
                    phase: "downloading",
                    downloaded,
                    total,
                });
            },
            || {},
        )
        .await
        .map_err(|e| format!("下载或签名校验失败，尚未安装更新，可重试。{e}"))?;
    // download() has verified the signature before we hand anything to Windows.
    manager.wait();
    let _ = on_progress.send(Progress {
        phase: "installing",
        downloaded,
        total: Some(downloaded),
    });
    tauri::async_runtime::spawn_blocking(move || {
        let _keep_alive = app;
        update
            .install(bytes)
            .map_err(|e| format!("启动安装程序失败，可重试。{e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installation_does_not_interrupt_collection_or_saving() {
        for status in ["running", "stopping"] {
            assert!(ensure_can_install(status, false).is_err());
        }
        for status in ["idle", "completed", "stopped", "error"] {
            assert!(ensure_can_install(status, false).is_ok());
            assert!(ensure_can_install(status, true).is_err());
        }
    }

    #[test]
    fn failure_releases_busy_state_for_retry() {
        let state = UpdateState::default();
        state.0.lock().unwrap().installing = true;
        drop(ResetBusy(&state, true));
        assert!(!state.0.lock().unwrap().installing);
        state.0.lock().unwrap().checking = true;
        drop(ResetBusy(&state, false));
        assert!(!state.0.lock().unwrap().checking);
    }
}
