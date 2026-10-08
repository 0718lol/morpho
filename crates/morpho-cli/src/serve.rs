//! morpho serve: a local HTTP API over the conversion engine.
//!
//! Endpoints (JSON):
//!   GET  /formats           full conversion matrix
//!   GET  /engines           engine availability
//!   POST /convert           submit jobs: {files, target, output_dir?, quality?, preset?}
//!   GET  /jobs              all jobs with live status
//!   GET  /jobs/{id}         one job
//!   POST /jobs/{id}/cancel  cancel a queued or running job
//!
//! Binds 127.0.0.1 only — this API is for local automation, never exposed.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::json;

use morpho_engine::queue::{JobEngine, JobEvent, JobOptions};
use morpho_engine::Format;

#[derive(Clone)]
struct AppState {
    engine: Arc<JobEngine>,
    jobs: Arc<Mutex<HashMap<u64, JobView>>>,
}

#[derive(Serialize, Clone)]
struct JobView {
    id: u64,
    source: String,
    target: String,
    status: String,
    progress: f32,
    note: String,
    output: Option<String>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct ConvertRequest {
    files: Vec<String>,
    target: String,
    #[serde(default)]
    output_dir: Option<String>,
    #[serde(default)]
    quality: Option<u8>,
    #[serde(default)]
    preset: Option<String>,
}

async fn formats(State(_state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({ "formats": morpho_engine::queue::ui_matrix() }))
}

async fn engines(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({ "engines": state.engine.engines().status() }))
}

async fn convert(
    State(state): State<AppState>,
    Json(req): Json<ConvertRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let target: Format = req
        .target
        .parse()
        .map_err(|e: String| (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))))?;
    if req.files.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "no files given" })),
        ));
    }
    let mut ids = Vec::new();
    for f in &req.files {
        let opts = JobOptions {
            target,
            output_dir: req.output_dir.as_ref().map(PathBuf::from),
            quality: req.quality,
            preset: req.preset.clone(),
        };
        let id = state.engine.submit(PathBuf::from(f), opts);
        state.jobs.lock().unwrap().insert(
            id,
            JobView {
                id,
                source: f.clone(),
                target: req.target.clone(),
                status: "queued".into(),
                progress: 0.0,
                note: String::new(),
                output: None,
                error: None,
            },
        );
        ids.push(id);
    }
    Ok(Json(json!({ "jobs": ids })))
}

async fn list_jobs(State(state): State<AppState>) -> Json<serde_json::Value> {
    let jobs = state.jobs.lock().unwrap();
    let mut list: Vec<&JobView> = jobs.values().collect();
    list.sort_by_key(|j| j.id);
    Json(json!({ "jobs": list }))
}

async fn get_job(
    State(state): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let jobs = state.jobs.lock().unwrap();
    jobs.get(&id)
        .map(|j| Json(json!({ "job": j })))
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": format!("no job #{id}") })),
            )
        })
}

async fn cancel_job(State(state): State<AppState>, Path(id): Path<u64>) -> Json<serde_json::Value> {
    state.engine.cancel(id);
    Json(json!({ "cancelled": id }))
}

/// Feed engine broadcast events into the per-job status map.
async fn forward_events(state: AppState) {
    let mut rx = state.engine.subscribe();
    while let Ok(ev) = rx.recv().await {
        let mut jobs = state.jobs.lock().unwrap();
        match ev {
            JobEvent::Queued { .. } => {}
            JobEvent::Started { id } => {
                if let Some(j) = jobs.get_mut(&id) {
                    j.status = "running".into();
                }
            }
            JobEvent::Progress { id, ratio, note } => {
                if let Some(j) = jobs.get_mut(&id) {
                    j.status = "running".into();
                    j.progress = ratio;
                    j.note = note;
                }
            }
            JobEvent::Done { id, output } => {
                if let Some(j) = jobs.get_mut(&id) {
                    j.status = "done".into();
                    j.progress = 1.0;
                    j.output = Some(output.display().to_string());
                }
            }
            JobEvent::Failed { id, error, .. } => {
                if let Some(j) = jobs.get_mut(&id) {
                    j.status = "failed".into();
                    j.error = Some(error);
                }
            }
            JobEvent::Cancelled { id } => {
                if let Some(j) = jobs.get_mut(&id) {
                    j.status = "cancelled".into();
                }
            }
        }
    }
}

pub async fn run(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let engine = JobEngine::new();
    let state = AppState {
        engine: engine.clone(),
        jobs: Arc::new(Mutex::new(HashMap::new())),
    };
    let fwd = state.clone();
    tokio::spawn(async move { forward_events(fwd).await });
    let app = Router::new()
        .route("/formats", get(formats))
        .route("/engines", get(engines))
        .route("/convert", post(convert))
        .route("/jobs", get(list_jobs))
        .route("/jobs/{id}", get(get_job))
        .route("/jobs/{id}/cancel", post(cancel_job))
        .with_state(state);
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("morpho serve listening on http://{addr} (local only)");
    axum::serve(listener, app).await?;
    Ok(())
}
