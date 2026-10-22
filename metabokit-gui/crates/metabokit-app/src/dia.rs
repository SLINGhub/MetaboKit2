use metabokit_core::progress::{Event, Reporter};
use metabokit_dia::{
    integration::{self, Baseline, Overview, TargetWindows, Window, WindowChange},
    params::Dataset,
    visualizer::{Detail, Session},
    Settings,
};
use std::path::Path;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, State};

struct Progress(AppHandle);
impl Reporter for Progress {
    fn emit(&self, event: Event) {
        let _ = self.0.emit("mk://dia-event", event);
    }
}

fn guarded<T>(f: impl FnOnce() -> Result<T, metabokit_dia::Failure>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|payload| {
        let detail=payload.downcast_ref::<String>().map(String::as_str).or_else(||payload.downcast_ref::<&str>().copied()).unwrap_or("unknown input error");
        format!("DIA could not process this input: {detail}. Partial results were retained for inspection.")
    })?.map_err(|e|e.to_string())
}
#[tauri::command]
pub async fn dia_scan(path: String) -> Result<Dataset, String> {
    tauri::async_runtime::spawn_blocking(move || {
        guarded(|| metabokit_dia::params::discover(Path::new(&path)))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn dia_validate(settings: Settings) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || settings.validate().map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn dia_load_preset(path: String) -> Result<Settings, String> {
    tauri::async_runtime::spawn_blocking(move || guarded(|| Settings::load(Path::new(&path))))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn dia_save_preset(path: String, settings: Settings) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(path, data).map_err(|e| e.to_string())
}
#[tauri::command]
pub fn dia_start(
    app: AppHandle,
    state: State<'_, crate::RunState>,
    settings: Settings,
) -> Result<(), String> {
    // Validation and filesystem I/O run on the worker, not the webview thread.
    if state.running.swap(true, Ordering::SeqCst) {
        return Err(
            "An analysis is already running. Wait for it to finish before starting another.".into(),
        );
    }
    state.cancel.reset();
    let cancel = state.cancel.clone();
    let running = state.running.clone();
    let result = std::thread::Builder::new()
        .name("metabokit-dia".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let outcome =
                guarded(|| metabokit_dia::run(&settings, &Progress(app.clone()), &cancel));
            running.store(false, Ordering::SeqCst);
            if cancel.is_cancelled() {
                let _ = app.emit("mk://dia-cancelled", ());
            } else {
                match outcome {
                    Ok(o) => {
                        let _ = app.emit("mk://dia-finished", o);
                    }
                    Err(e) => {
                        let _ = app.emit("mk://dia-failed", e);
                    }
                }
            }
        });
    if let Err(e) = result {
        state.running.store(false, Ordering::SeqCst);
        return Err(e.to_string());
    }
    Ok(())
}
#[tauri::command]
pub async fn dia_open(path: String, search: String, offset: usize) -> Result<Session, String> {
    tauri::async_runtime::spawn_blocking(move || {
        guarded(|| metabokit_dia::visualizer::open(Path::new(&path), &search, offset))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn dia_detail(
    path: String,
    target: usize,
    sample: usize,
    width: f32,
    window: Option<Window>,
    baseline: Option<Baseline>,
) -> Result<Detail, String> {
    tauri::async_runtime::spawn_blocking(move || {
        guarded(|| {
            metabokit_dia::visualizer::detail(
                Path::new(&path),
                target,
                sample,
                width,
                window,
                baseline,
            )
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn dia_integration_overview(path: String) -> Result<Overview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        guarded(|| integration::overview(Path::new(&path)))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn dia_integration_target(path: String, target: usize) -> Result<TargetWindows, String> {
    tauri::async_runtime::spawn_blocking(move || {
        guarded(|| integration::target(Path::new(&path), target))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn dia_integration_set_windows(
    state: State<'_, crate::RunState>,
    path: String,
    target: usize,
    changes: Vec<WindowChange>,
) -> Result<TargetWindows, String> {
    if state.running.load(Ordering::SeqCst) {
        return Err(
            "Wait for the current analysis or export to finish before editing windows.".into(),
        );
    }
    tauri::async_runtime::spawn_blocking(move || {
        guarded(|| integration::set_windows(Path::new(&path), target, &changes))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn dia_integration_set_baseline(
    state: State<'_, crate::RunState>,
    path: String,
    target: Option<usize>,
    baseline: Option<Baseline>,
) -> Result<(), String> {
    if state.running.load(Ordering::SeqCst) {
        return Err(
            "Wait for the current analysis or export to finish before changing baselines.".into(),
        );
    }
    tauri::async_runtime::spawn_blocking(move || {
        guarded(|| integration::set_baseline(Path::new(&path), target, baseline))
    })
    .await
    .map_err(|e| e.to_string())?
}
/// Re-integrates the whole result folder on a worker thread. Shares the
/// analysis slot and cancel token with DIA/DDA runs.
#[tauri::command]
pub fn dia_integration_export(
    app: AppHandle,
    state: State<'_, crate::RunState>,
    path: String,
) -> Result<(), String> {
    if state.running.swap(true, Ordering::SeqCst) {
        return Err(
            "An analysis is already running. Wait for it to finish before exporting.".into(),
        );
    }
    state.cancel.reset();
    let cancel = state.cancel.clone();
    let running = state.running.clone();
    let result = std::thread::Builder::new()
        .name("metabokit-dia-integration".into())
        .spawn(move || {
            let outcome =
                guarded(|| integration::export(Path::new(&path), &Progress(app.clone()), &cancel));
            running.store(false, Ordering::SeqCst);
            if cancel.is_cancelled() {
                let _ = app.emit("mk://dia-integration-cancelled", ());
            } else {
                match outcome {
                    Ok(o) => {
                        let _ = app.emit("mk://dia-integration-finished", o);
                    }
                    Err(e) => {
                        let _ = app.emit("mk://dia-integration-failed", e);
                    }
                }
            }
        });
    if let Err(e) = result {
        state.running.store(false, Ordering::SeqCst);
        return Err(e.to_string());
    }
    Ok(())
}
