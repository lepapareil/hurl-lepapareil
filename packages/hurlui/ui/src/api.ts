// Transport to the Rust core: Tauri IPC in the desktop application, HTTP to the
// development bridge when the frontend runs in a regular browser.

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';

export const isTauri = '__TAURI_INTERNALS__' in window;

// --- Development bridge (browser mode) ------------------------------------------
//
// The bridge only answers to requests carrying its access token. It prints a
// URL of the form `http://localhost:1420/#bridge-token=<token>`: the page
// opened from that URL holds the token and sends it with every request. The
// Vite proxy only forwards it, so that reaching the Vite port is not enough to
// drive the bridge.

const TOKEN_HEADER = 'X-Hurlui-Bridge-Token';
const TOKEN_STORAGE_KEY = 'hurlui-bridge-token';
const TOKEN_PARAM = 'bridge-token';
const UNAUTHENTICATED =
  'No access to the development bridge. Open the URL printed by `npm run bridge` (it carries the access token).';

/** Token found in the fragment of the address, which is then removed from the address bar. */
function takeTokenFromUrl(): string | null {
  const params = new URLSearchParams(window.location.hash.replace(/^#/, ''));
  const token = params.get(TOKEN_PARAM);
  if (token === null) return null;
  // Not left in the URL: it would end up in the history and the bookmarks.
  params.delete(TOKEN_PARAM);
  const rest = params.toString();
  const { pathname, search } = window.location;
  window.history.replaceState(window.history.state, '', `${pathname}${search}${rest ? `#${rest}` : ''}`);
  // The token is made of hexadecimal digits: anything exotic is not a token,
  // and is not put in a header.
  return /^[\w-]+$/.test(token) ? token : null;
}

/**
 * Token of this page: taken from the URL the first time, then kept for the
 * session of the tab (reloads included).
 */
function readBridgeToken(): string {
  const fromUrl = takeTokenFromUrl();
  try {
    if (fromUrl) {
      window.sessionStorage.setItem(TOKEN_STORAGE_KEY, fromUrl);
      return fromUrl;
    }
    return window.sessionStorage.getItem(TOKEN_STORAGE_KEY) ?? '';
  } catch {
    // Storage disabled: the token lives as long as the page.
    return fromUrl ?? '';
  }
}

const bridgeToken = isTauri ? '' : readBridgeToken();
/** The bridge answered 403 to the token of this page (the bridge was restarted, most likely). */
let bridgeRefused = false;

if (!isTauri) {
  // Pasting the URL of the bridge in a tab which already shows the application
  // only changes the fragment: start over with the new token.
  window.addEventListener('hashchange', () => {
    const token = takeTokenFromUrl();
    if (!token) return;
    try {
      window.sessionStorage.setItem(TOKEN_STORAGE_KEY, token);
      window.location.reload();
    } catch {
      // Storage disabled: a reload would lose the token.
    }
  });
}

async function bridge<T>(path: string, body?: unknown): Promise<T> {
  if (!bridgeToken) throw new Error(UNAUTHENTICATED);
  const res = await fetch(`/__bridge${path}`, {
    method: body === undefined ? 'GET' : 'POST',
    headers: { 'Content-Type': 'application/json', [TOKEN_HEADER]: bridgeToken },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  bridgeRefused = res.status === 403;
  if (bridgeRefused) throw new Error(UNAUTHENTICATED);
  const data = (await res.json()) as { ok?: T; error?: string };
  if (data.error !== undefined) throw new Error(data.error);
  return data.ok as T;
}

/** Calls a command of the core (see `packages/hurlui_core/src/api.rs`). */
export async function call<T = null>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!isTauri) return bridge<T>(`/api/${cmd}`, args);
  try {
    return await invoke<T>('api', { cmd, args });
  } catch (e) {
    throw new Error(typeof e === 'string' ? e : String((e as Error)?.message ?? e));
  }
}

type Handler = (payload: unknown) => void;
const bridgeHandlers = new Map<string, Set<Handler>>();
let polling = false;

function pollBridge() {
  if (polling) return;
  polling = true;
  setInterval(async () => {
    // Unauthenticated: asking every second would only fill the console with
    // refused requests. The commands report the problem; polling resumes as
    // soon as one of them is accepted.
    if (!bridgeToken || bridgeRefused) return;
    try {
      const events = await bridge<{ event: string; payload: unknown }[]>('/events');
      for (const e of events) bridgeHandlers.get(e.event)?.forEach((h) => h(e.payload));
    } catch {
      // Bridge not reachable: nothing to deliver.
    }
  }, 1000);
}

/** Subscribes to an event pushed by the core. */
export function onEvent<T>(name: string, handler: (payload: T) => void): void {
  if (isTauri) {
    void listen<T>(name, (e) => handler(e.payload));
    return;
  }
  if (!bridgeHandlers.has(name)) bridgeHandlers.set(name, new Set());
  bridgeHandlers.get(name)!.add(handler as Handler);
  pollBridge();
}

/** Native directory picker (a plain prompt in browser mode). */
export async function pickDirectory(title: string): Promise<string | null> {
  if (!isTauri) return window.prompt(`${title} (absolute path)`)?.trim() || null;
  const selected = await open({ directory: true, multiple: false, title });
  return typeof selected === 'string' ? selected : null;
}

export async function pickFile(title: string): Promise<string | null> {
  if (!isTauri) return window.prompt(`${title} (absolute path)`)?.trim() || null;
  const selected = await open({ directory: false, multiple: false, title });
  return typeof selected === 'string' ? selected : null;
}
