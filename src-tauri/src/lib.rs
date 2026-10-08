use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use morpho_engine::queue::{JobEngine, JobEvent, JobOptions, MatrixEntry};
use morpho_engine::Format;

#[derive(Default, Serialize, Deserialize, Clone)]
struct Settings {
    #[serde(default)]
    output_dir: Option<String>,
    #[serde(default)]
    quality: Option<u8>,
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    theme: Option<String>,
    #[serde(default)]
    lang: Option<String>,
}

struct AppState {
    engine: Arc<JobEngine>,
    settings: Mutex<Settings>,
    history: Mutex<Vec<HistoryEntry>>,
    /// Parameters per in-flight job, keyed by job id; consumed on Done so the
    /// history entry keeps everything needed to re-run the conversion.
    job_params: Mutex<HashMap<u64, JobOptions>>,
}

#[derive(Serialize, Deserialize, Clone)]
struct HistoryEntry {
    #[serde(default)]
    id: u64,
    source: String,
    output: String,
    target: String,
    when: String,
    /// Soft-deleted entries live in the archive view until restored or purged.
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    archived_at: Option<String>,
    /// Conversion parameters snapshot for "convert again"; None for entries
    /// recorded before this field existed.
    #[serde(default)]
    params: Option<JobOptions>,
}

/// The app data dir is not auto-created by anything else; every write below
/// silently no-ops without this (OpenOptions::create only makes the file).
fn ensure_data_dir(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir();
    if let Err(e) = &dir {
        eprintln!("[morpho] app_data_dir() failed: {e}");
    }
    let dir = dir.ok()?;
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("[morpho] create_dir_all({}) failed: {e}", dir.display());
        return None;
    }
    Some(dir)
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    ensure_data_dir(app).map(|d| d.join("settings.json"))
}

fn history_path(app: &AppHandle) -> Option<PathBuf> {
    ensure_data_dir(app).map(|d| d.join("history.jsonl"))
}

#[tauri::command]
fn get_matrix() -> Vec<MatrixEntry> {
    morpho_engine::queue::ui_matrix()
}

#[tauri::command]
fn get_formats() -> Vec<String> {
    morpho_engine::format::all_in_order().iter().map(|f| f.extension().into()).collect()
}

#[tauri::command]
fn engines_status(state: State<AppState>) -> Vec<(String, bool)> {
    state.engine.engines().status()
}

#[tauri::command]
async fn submit_jobs(
    state: State<'_, AppState>,
    files: Vec<String>,
    target: String,
    output_dir: Option<String>,
    quality: Option<u8>,
    preset: Option<String>,
) -> Result<Vec<u64>, String> {
    let target: Format = target.parse().map_err(|e: String| e)?;
    let out_dir = output_dir.map(PathBuf::from);
    let mut ids = Vec::new();
    for f in files {
        let opts = JobOptions {
            target,
            output_dir: out_dir.clone(),
            quality,
            preset: preset.clone(),
        };
        let job_id = state.engine.submit(PathBuf::from(f), opts.clone());
        state.job_params.lock().unwrap().insert(job_id, opts);
        ids.push(job_id);
    }
    Ok(ids)
}

#[tauri::command]
fn cancel_job(state: State<AppState>, id: u64) {
    state.engine.cancel(id);
}

#[tauri::command]
async fn thumbnail(path: String) -> Option<String> {
    // decode off the main thread — large images would freeze the UI otherwise
    let png = tokio::task::spawn_blocking(move || {
        morpho_engine::image::thumbnail_png(std::path::Path::new(&path), 160).ok()
    })
    .await
    .ok()
    .flatten()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    ))
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(app: AppHandle, state: State<AppState>, settings: Settings) {
    if let Some(path) = settings_path(&app) {
        if let Ok(json) = serde_json::to_string_pretty(&settings) {
            let _ = std::fs::create_dir_all(path.parent().unwrap());
            let _ = std::fs::write(path, json);
        }
    }
    *state.settings.lock().unwrap() = settings;
}

#[tauri::command]
fn get_history(state: State<AppState>) -> Vec<HistoryEntry> {
    state.history.lock().unwrap().clone()
}

#[tauri::command]
fn clear_history(app: AppHandle, state: State<AppState>) {
    state.history.lock().unwrap().clear();
    if let Some(path) = history_path(&app) {
        let _ = std::fs::remove_file(path);
    }
}

/// Rewrite history.jsonl from memory (the file is oldest-first, the in-memory
/// list newest-first). Used by every mutating command below.
fn save_history_file(app: &AppHandle, state: &AppState) {
    let Some(path) = history_path(app) else { return };
    let history = state.history.lock().unwrap();
    let mut out = String::new();
    for entry in history.iter().rev() {
        if let Ok(line) = serde_json::to_string(entry) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    let _ = std::fs::write(path, out);
}

fn with_entry(state: &AppState, id: u64, f: impl FnOnce(&mut HistoryEntry)) -> bool {
    let mut history = state.history.lock().unwrap();
    match history.iter_mut().find(|e| e.id == id) {
        Some(entry) => {
            f(entry);
            true
        }
        None => false,
    }
}

#[tauri::command]
fn archive_history(app: AppHandle, state: State<AppState>, id: u64) -> Result<(), String> {
    if !with_entry(&state, id, |e| {
        e.archived = true;
        e.archived_at = Some(chrono_like_now());
    }) {
        return Err(format!("no history entry #{id}"));
    }
    save_history_file(&app, &state);
    Ok(())
}

#[tauri::command]
fn restore_history(app: AppHandle, state: State<AppState>, id: u64) -> Result<(), String> {
    if !with_entry(&state, id, |e| {
        e.archived = false;
        e.archived_at = None;
    }) {
        return Err(format!("no history entry #{id}"));
    }
    save_history_file(&app, &state);
    Ok(())
}

/// Second-stage delete: the record is gone for good. The converted file on
/// disk is intentionally left alone — only the history entry is removed.
#[tauri::command]
fn purge_history(app: AppHandle, state: State<AppState>, id: u64) -> Result<(), String> {
    state
        .history
        .lock()
        .unwrap()
        .retain(|e| e.id != id);
    save_history_file(&app, &state);
    Ok(())
}

/// Re-run a history entry: same source file, same conversion parameters.
#[tauri::command]
fn reconvert(state: State<'_, AppState>, id: u64) -> Result<u64, String> {
    let entry = state
        .history
        .lock()
        .unwrap()
        .iter()
        .find(|e| e.id == id)
        .cloned()
        .ok_or_else(|| format!("no history entry #{id}"))?;
    if entry.source.is_empty() {
        return Err("history entry has no source path (recorded before v0.1.2)".into());
    }
    let source = PathBuf::from(&entry.source);
    if !source.exists() {
        return Err(format!("source file no longer exists: {}", entry.source));
    }
    let opts = match entry.params.clone() {
        Some(p) => p,
        None => JobOptions {
            target: entry.target.parse().map_err(|e: String| e)?,
            output_dir: None,
            quality: None,
            preset: None,
        },
    };
    Ok(state.engine.submit(source, opts))
}

/// Reveal a file in Explorer / Finder.
#[tauri::command]
fn reveal(path: String) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(&path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    {
        // ask Finder to reveal AND select the file itself; plain `open` only
        // opens the parent folder with nothing highlighted. Fall back to it
        // if the AppleScript path fails.
        let script = format!(
            "tell application \"Finder\" to reveal POSIX file \"{}\"",
            path.replace('\\', "\\\\").replace('"', "\\\"")
        );
        let revealed = std::process::Command::new("osascript")
            .args(["-e", &script, "-e", "tell application \"Finder\" to activate"])
            .spawn()
            .and_then(|mut c| c.wait())
            .map(|s| s.success())
            .unwrap_or(false);
        if !revealed {
            std::process::Command::new("open")
                .arg(
                    std::path::Path::new(&path)
                        .parent()
                        .unwrap_or(std::path::Path::new(".")),
                )
                .spawn()
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn load_settings(app: &AppHandle) -> Settings {
    settings_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn load_history(app: &AppHandle) -> Vec<HistoryEntry> {
    let Some(path) = history_path(app) else { return vec![] };
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string(path) {
        for line in text.lines() {
            if let Ok(entry) = serde_json::from_str::<HistoryEntry>(line) {
                out.push(entry);
            }
        }
    }
    out.reverse();
    out
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let engine = JobEngine::new();
            let settings = load_settings(&handle);
            let history = load_history(&handle);

            // forward engine events to the webview
            let mut rx = engine.subscribe();
            let emitter = handle.clone();
            tauri::async_runtime::spawn(async move {
                // Queued events carry the source path; keep it per job so the
                // history entry records what went IN, not just what came out
                let mut sources: std::collections::HashMap<u64, String> =
                    std::collections::HashMap::new();
                loop {
                    if let Ok(ev) = rx.recv().await {
                        match &ev {
                            JobEvent::Queued { id, name } => {
                                sources.insert(*id, name.clone());
                            }
                            JobEvent::Done { id, output } => {
                                let source = sources.remove(id).unwrap_or_default();
                                let params = emitter.try_state::<AppState>().and_then(|s| {
                                    let mut m = s.job_params.lock().ok()?;
                                    m.remove(id)
                                });
                                append_history(&emitter, output, &source, params);
                            }
                            JobEvent::Failed { id, .. } | JobEvent::Cancelled { id } => {
                                sources.remove(id);
                                if let Some(s) = emitter.try_state::<AppState>() {
                                    if let Ok(mut m) = s.job_params.lock() {
                                        m.remove(id);
                                    }
                                }
                            }
                            _ => {}
                        }
                        let _ = emitter.emit("job-event", &ev);
                    }
                }
            });

            app.manage(AppState {
                engine,
                settings: Mutex::new(settings),
                history: Mutex::new(history),
                job_params: Mutex::new(HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_matrix,
            get_formats,
            engines_status,
            submit_jobs,
            cancel_job,
            thumbnail,
            get_settings,
            save_settings,
            get_history,
            clear_history,
            archive_history,
            restore_history,
            purge_history,
            reconvert,
            reveal
        ])
        .run(tauri::generate_context!())
        .expect("error while running Morpho");
}

fn append_history(
    app: &AppHandle,
    output: &std::path::Path,
    source: &str,
    params: Option<JobOptions>,
) {
    let entry = HistoryEntry {
        id: 0, // assigned below from the in-memory list
        source: source.to_string(),
        output: output.display().to_string(),
        target: output
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string(),
        when: chrono_like_now(),
        archived: false,
        archived_at: None,
        params,
    };
    if let Some(state) = app.try_state::<AppState>() {
        let mut history = state.history.lock().unwrap();
        let id = history.iter().map(|e| e.id).max().unwrap_or(0) + 1;
        let entry = HistoryEntry { id, ..entry };
        history.insert(0, entry.clone());
        drop(history);
        if let Some(path) = history_path(app) {
            use std::io::Write;
            match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                Ok(mut f) => {
                    if let Err(e) = writeln!(f, "{}", serde_json::to_string(&entry).unwrap_or_default()) {
                        eprintln!("[morpho] history write failed: {e}");
                    }
                }
                Err(e) => eprintln!("[morpho] history open failed ({}): {e}", path.display()),
            }
        }
    }
}

/// Local-time timestamp (`chrono` was already in the tree via tauri; the old
/// hand-rolled version formatted UTC seconds, 8h off for UTC+8 users).
fn chrono_like_now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
