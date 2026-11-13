import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const bridgePort = process.env.HURLUI_BRIDGE_PORT ?? '1421';

// https://vite.dev/config/
export default defineConfig({
  // The frontend lives in `ui/`: `src/` is the Rust code of the Tauri shell,
  // as in the other packages of Hurl.
  root: 'ui',
  plugins: [react()],
  // Tauri expects a fixed port and its own output on the terminal.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // Browser mode (`npm run dev` + `npm run bridge`): the core API is served
    // by the development bridge instead of the Tauri shell. The bridge only
    // answers to requests carrying its token, which the page holds and sends
    // itself (see ui/src/api.ts): this proxy forwards it and adds nothing, so
    // that reaching this port does not give access to the bridge.
    proxy: {
      '/__bridge': {
        target: `http://127.0.0.1:${bridgePort}`,
        rewrite: (path) => path.replace(/^\/__bridge/, ''),
      },
    },
  },
  build: {
    // Embedded in the application: `frontendDist` of tauri.conf.json.
    outDir: '../dist',
    emptyOutDir: true,
    target: ['es2022', 'chrome105', 'safari15'],
    chunkSizeWarningLimit: 1500,
  },
});
