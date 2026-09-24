//! Sanktuary bridge: lets the RapidRAW editor run in a browser, inside Sanktuary OS (sanktuary.studio).
//!
//! When the app is started with SANKTUARY_BRIDGE_PORT set, it keeps running (window hidden) on the home server
//! and answers a small set of editing commands over HTTP on 127.0.0.1 only:
//!
//!   POST /invoke/<command>   body: the same JSON arguments the desktop UI passes to invoke()
//!   GET  /events             server-sent events: the app events the editor listens for
//!
//! Every request must carry `x-bridge-token: <SANKTUARY_BRIDGE_TOKEN>`. Sanktuary's own server is the only
//! caller: it checks the user's sign-in and space rights, turns "sk://space/path" into real paths and back, and
//! allows one editing session at a time (RapidRAW edits one image at a time).
//!
//! Deliberately tiny: plain HTTP/1.1 over tokio, one request per connection, no new dependencies.

use crate::AppState;
use serde_json::{Value, json};
use std::sync::OnceLock;
use tauri::{AppHandle, Listener, Manager};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

/// App events the browser editor needs (they're emitted by the commands below).
const FORWARDED_EVENTS: &[&str] = &[
    "preview-update-uncropped",
    "image-metadata-loaded",
    "thumbnail-generated",
    "export-complete",
    "export-error",
    "export-cancelled",
    "batch-export-progress",
    "analytics-update",
    "histogram-update",
    "waveform-update",
];

static EVENTS: OnceLock<broadcast::Sender<String>> = OnceLock::new();

/// True when this copy runs as Sanktuary's engine: the desktop window then stays hidden.
pub fn active() -> bool {
    std::env::var("SANKTUARY_BRIDGE_PORT").is_ok()
}

/// Called from setup() when SANKTUARY_BRIDGE_PORT is set.
pub fn start(app: AppHandle) {
    let Ok(port) = std::env::var("SANKTUARY_BRIDGE_PORT").map(|p| p.parse::<u16>().unwrap_or(3091)) else {
        return;
    };
    let token = std::env::var("SANKTUARY_BRIDGE_TOKEN").unwrap_or_default();
    if token.len() < 16 {
        log::error!("Sanktuary bridge not started: SANKTUARY_BRIDGE_TOKEN must be at least 16 characters");
        return;
    }

    let (tx, _) = broadcast::channel::<String>(256);
    let _ = EVENTS.set(tx.clone());
    for name in FORWARDED_EVENTS {
        let tx = tx.clone();
        let name = name.to_string();
        app.listen_any(name.clone(), move |event| {
            let _ = tx.send(format!("event: {}\ndata: {}\n\n", name, event.payload()));
        });
    }

    tauri::async_runtime::spawn(async move {
        let listener = match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(l) => l,
            Err(e) => return log::error!("Sanktuary bridge could not listen on 127.0.0.1:{}: {}", port, e),
        };
        log::info!("Sanktuary bridge listening on 127.0.0.1:{}", port);
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            let app = app.clone();
            let token = token.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = handle(stream, app, &token).await {
                    log::warn!("Sanktuary bridge request failed: {}", e);
                }
            });
        }
    });
}

async fn handle(stream: TcpStream, app: AppHandle, token: &str) -> Result<(), String> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await.map_err(|e| e.to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

    let mut length = 0usize;
    let mut authorised = false;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).await.map_err(|e| e.to_string())? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = v.trim().parse().unwrap_or(0),
                "x-bridge-token" => authorised = constant_time_eq(v.trim().as_bytes(), token.as_bytes()),
                _ => {}
            }
        }
    }
    if !authorised || length > 64 * 1024 * 1024 {
        let mut stream = reader.into_inner();
        return respond(&mut stream, if authorised { 413 } else { 401 }, "text/plain", b"refused").await;
    }
    // The body is read through the same buffered reader (part of it may already be buffered with the headers)
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).await.map_err(|e| e.to_string())?;
    let mut stream = reader.into_inner();

    if method == "GET" && path == "/events" {
        let mut rx = EVENTS.get().ok_or("events not ready")?.subscribe();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\n\r\n")
            .await
            .map_err(|e| e.to_string())?;
        loop {
            let msg = match tokio::time::timeout(std::time::Duration::from_secs(25), rx.recv()).await {
                Ok(Ok(m)) => m,
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(_)) => return Ok(()),
                Err(_) => ": ping\n\n".to_string(), // keep the stream alive through proxies
            };
            if stream.write_all(msg.as_bytes()).await.is_err() {
                return Ok(());
            }
        }
    }

    let Some(command) = path.strip_prefix("/invoke/").filter(|_| method == "POST") else {
        return respond(&mut stream, 404, "text/plain", b"not found").await;
    };
    let args: Value = if body.is_empty() { json!({}) } else { serde_json::from_slice(&body).map_err(|e| e.to_string())? };

    match dispatch(command, args, &app).await {
        Ok(Reply::Json(v)) => respond(&mut stream, 200, "application/json", v.to_string().as_bytes()).await,
        Ok(Reply::Bytes(b)) => respond(&mut stream, 200, "application/octet-stream", &b).await,
        Err(e) => respond(&mut stream, 500, "text/plain", e.as_bytes()).await,
    }
}

enum Reply {
    Json(Value),
    Bytes(Vec<u8>),
}

fn arg<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value(args.get(key).cloned().unwrap_or(Value::Null)).map_err(|e| format!("argument {}: {}", key, e))
}
fn to_json<T: serde::Serialize>(v: T) -> Result<Reply, String> {
    serde_json::to_value(v).map(Reply::Json).map_err(|e| e.to_string())
}

/// The editing commands, called exactly as the desktop app's invoke() would (same argument names).
/// Anything not listed here is refused: no folder browsing, moving, deleting, settings or AI downloads.
async fn dispatch(command: &str, args: Value, app: &AppHandle) -> Result<Reply, String> {
    let state = app.state::<AppState>();
    match command {
        "load_image" => to_json(crate::image_loader::load_image(arg(&args, "path")?, state, app.clone()).await?),
        "apply_adjustments" => {
            // Same job the desktop command queues, but the rendered bytes come straight back here
            let (tx, rx) = tokio::sync::oneshot::channel();
            {
                let guard = state.preview_worker_tx.lock().unwrap();
                let worker = guard.as_ref().ok_or("Preview worker not running")?;
                worker
                    .send(crate::PreviewJob {
                        adjustments: arg(&args, "jsAdjustments")?,
                        is_interactive: arg(&args, "isInteractive").unwrap_or(false),
                        target_resolution: arg(&args, "targetResolution").unwrap_or(None),
                        roi: arg(&args, "roi").unwrap_or(None),
                        request_analytics: arg(&args, "requestAnalytics").unwrap_or(false),
                        compute_waveform: arg(&args, "computeWaveform").unwrap_or(false),
                        active_waveform_channel: arg(&args, "activeWaveformChannel").unwrap_or(None),
                        responder: tx,
                    })
                    .map_err(|e| e.to_string())?;
            }
            rx.await.map(Reply::Bytes).map_err(|_| "Superseded".to_string())
        }
        "generate_uncropped_preview" => to_json(crate::generate_uncropped_preview(arg(&args, "jsAdjustments")?, state, app.clone()).await?),
        "calculate_auto_adjustments" => to_json(crate::image_processing::calculate_auto_adjustments(state)?),
        "load_metadata" => to_json(crate::file_management::load_metadata(arg(&args, "path")?, app.clone())?),
        "save_metadata_and_update_thumbnail" => to_json(crate::file_management::save_metadata_and_update_thumbnail(
            arg(&args, "path")?,
            arg(&args, "adjustments")?,
            app.clone(),
            state,
        )?),
        "get_image_dimensions" => to_json(crate::get_image_dimensions(arg(&args, "path")?)?),
        "list_images_in_dir" => to_json(crate::file_management::list_images_in_dir(arg(&args, "path")?, app.clone())?),
        "get_supported_file_types" => to_json(crate::file_management::get_supported_file_types()?),
        "load_settings" => to_json(crate::app_settings::load_settings(app.clone())?),
        "load_presets" => to_json(crate::file_management::load_presets(app.clone())?),
        "export_images" => to_json(
            crate::export_processing::export_images(
                arg(&args, "paths")?,
                arg(&args, "outputFolderOrFile")?,
                arg(&args, "isExplicitFilePath").unwrap_or(false),
                arg(&args, "baseOriginFolders").unwrap_or_default(),
                arg(&args, "exportSettings")?,
                arg(&args, "outputFormat")?,
                arg(&args, "currentEditPath").unwrap_or(None),
                arg(&args, "currentEditAdjustments").unwrap_or(None),
                state,
                app.clone(),
            )
            .await?,
        ),
        _ => Err(format!("{} is not available in the Sanktuary editor", command)),
    }
}

async fn respond(stream: &mut TcpStream, status: u16, kind: &str, body: &[u8]) -> Result<(), String> {
    let head = format!(
        "HTTP/1.1 {} {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        status,
        if status == 200 { "OK" } else { "Error" },
        kind,
        body.len()
    );
    stream.write_all(head.as_bytes()).await.map_err(|e| e.to_string())?;
    stream.write_all(body).await.map_err(|e| e.to_string())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
