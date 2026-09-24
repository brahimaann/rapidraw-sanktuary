// Sanktuary OS build: stand-ins for the Tauri desktop APIs, so the RapidRAW UI runs in a browser inside
// sanktuary.studio. `vite.config.mjs` points every @tauri-apps/* import here when SANKTUARY=1.
//
// Editing commands go to Sanktuary's server (/api/raw/invoke/<command>), which checks the sign-in and space
// rights and forwards them to this app running on the home server (src-tauri/src/sanktuary_bridge.rs).
// Paths are Sanktuary paths: "sk://<space>/<folder>/<file>".

import './win98.css'; // look like the Windows 98 desktop it opens in

const params = new URLSearchParams(location.search);
const API = '/api/raw';

// The image Sanktuary asked to open (?file=sk://space/path)
const openWithFile = params.get('file');

// Library features that don't exist in the browser editor answer with "nothing" instead of failing,
// so the UI starts cleanly.
const EMPTY: Record<string, unknown> = {
  get_albums: [],
  get_pinned_folder_trees: [],
  get_folder_tree: null,
  get_folder_children: [],
  list_images_recursive: [],
  get_album_images: [],
  is_tethering_supported: false,
  tether_list_cameras: [],
  is_raw9_available: false,
  check_ai_connector_status: false,
  fetch_community_presets: [],
  start_background_indexing: null,
  update_thumbnail_queue: null,
  cancel_thumbnail_generation: null,
  update_wgpu_transform: null,
  clear_image_caches: null,
  clear_session_caches: null,
  save_settings: null,
  frontend_log: null,
  get_log_file_path: '',
  list_luts: [],
  get_lensfun_makers: [],
  is_image_cached: false,
};

export async function invoke<T = unknown>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (command === 'frontend_ready') return { openWithFile, editSession: null } as T;
  if (command in EMPTY) return EMPTY[command] as T;
  const res = await fetch(`${API}/invoke/${command}`, {
    method: 'POST',
    credentials: 'include',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(args ?? {}),
  });
  if (!res.ok) throw await res.text();
  return (res.headers.get('content-type')?.includes('json') ? res.json() : res.arrayBuffer()) as Promise<T>;
}

/** Files are served by Sanktuary with the user's rights (thumbnails, originals). */
export const convertFileSrc = (path: string) => `${API}/file?path=${encodeURIComponent(path)}`;

// One event stream for all listeners
type Handler = (e: { event: string; id: number; payload: unknown }) => void;
let source: EventSource | null = null;
const handlers = new Map<string, Set<Handler>>();
export async function listen(event: string, handler: Handler): Promise<() => void> {
  source ??= new EventSource(`${API}/events`);
  if (!handlers.has(event)) {
    handlers.set(event, new Set());
    source.addEventListener(event, (m) => {
      let payload: unknown = (m as MessageEvent).data;
      try {
        payload = JSON.parse(payload as string);
      } catch {}
      handlers.get(event)?.forEach((h) => h({ event, id: 0, payload }));
    });
  }
  handlers.get(event)!.add(handler);
  return () => handlers.get(event)?.delete(handler);
}
export const once = listen;
export const emit = async () => {};

// The window belongs to Sanktuary: every window call is a harmless no-op
const noop: any = new Proxy(() => {}, {
  get: (_t, key) => (key === 'then' ? undefined : key.toString().startsWith('on') ? async () => () => {} : async () => false),
});
export const getCurrentWindow = () => noop;
export const getCurrentWebviewWindow = () => noop;

// Files are chosen in Sanktuary, not with the desktop's own dialogs
export const open = async (arg?: unknown) => {
  if (typeof arg === 'string') window.open(arg, '_blank', 'noopener'); // plugin-shell open(url)
  return null;
};
export const save = async () => null;
export const message = async (text: string) => alert(text);
export const ask = async (text: string) => confirm(text);
export const confirmDialog = ask;

export const platform = () => 'linux'; // paths use "/" (sk://...)
export const type = () => 'linux';
export const exit = async () => window.parent?.postMessage({ sanktuary: 'close' }, location.origin);
export const relaunch = async () => location.reload();
export const getVersion = async () => 'sanktuary';
export const homeDir = async () => 'sk://';
