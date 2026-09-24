# RapidRAW for Sanktuary OS

This fork of [RapidRAW](https://github.com/CyberTimon/RapidRAW) by CyberTimon (AGPL-3.0) lets the RapidRAW
editor run in a browser inside [Sanktuary OS](https://sanktuary.studio). Under the AGPL, the source of this
modified version is published here; RapidRAW's own licence and copyright are unchanged.

## What changed

| File | Change |
|---|---|
| `src-tauri/src/sanktuary_bridge.rs` | New. When `SANKTUARY_BRIDGE_PORT` is set, a small HTTP server on 127.0.0.1 answers a fixed list of editing commands (load, adjust/preview, auto, metadata, save sidecar, export) and streams the app's events. Token-protected (`SANKTUARY_BRIDGE_TOKEN`). |
| `src-tauri/src/lib.rs` | Starts the bridge from `setup()` when enabled, and keeps the desktop window hidden in that mode. Nothing changes for normal desktop use. |
| `src/sanktuary/tauri.ts` | New. Browser stand-ins for the Tauri APIs the UI imports (`invoke` goes to Sanktuary's server, events via server-sent events, window/dialog calls are no-ops). |
| `vite.config.mjs` | With `SANKTUARY=1`, builds for `/apps/rapidraw/` and points `@tauri-apps/*` imports at the stand-ins. |

## How it runs

```
browser (Sanktuary, signed in) ──► sanktuary.studio/api/raw/* ──► 127.0.0.1:3091 (this app, hidden, GPU)
                                   checks sign-in + space rights,
                                   maps sk://space/path <-> real paths,
                                   one editing session at a time
```

## Build (on the Sanktuary home server)

Run `ops/rapidraw/setup.ps1` from the Sanktuary repo (as the user who runs Sanktuary). It installs the Rust
toolchain this repo asks for, builds the engine (`cargo build --release`) and the browser UI
(`SANKTUARY=1 npm run build`), copies the UI to `C:\homeserver\rapidraw\ui`, and registers the hidden
"Sanktuary RapidRAW" scheduled task.
