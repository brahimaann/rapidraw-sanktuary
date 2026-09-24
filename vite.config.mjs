import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import { fileURLToPath } from 'node:url';

const host = process.env.TAURI_DEV_HOST;

// Sanktuary OS build (SANKTUARY=1 npm run build): the browser editor served at sanktuary.studio/apps/rapidraw/,
// with every desktop (Tauri) API swapped for the stand-ins in src/sanktuary/tauri.ts.
const sanktuary = !!process.env.SANKTUARY;
const shim = fileURLToPath(new URL('./src/sanktuary/tauri.ts', import.meta.url));
const tauriModules = [
  '@tauri-apps/api/core',
  '@tauri-apps/api/event',
  '@tauri-apps/api/window',
  '@tauri-apps/api/webviewWindow',
  '@tauri-apps/api/app',
  '@tauri-apps/api/path',
  '@tauri-apps/plugin-dialog',
  '@tauri-apps/plugin-os',
  '@tauri-apps/plugin-process',
  '@tauri-apps/plugin-shell',
];

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [tailwindcss(), react()],
  ...(sanktuary && {
    base: '/apps/rapidraw/',
    resolve: { alias: tauriModules.map((find) => ({ find, replacement: shim })) },
  }),

  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },

  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    minify: !process.env.TAURI_ENV_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
}));
