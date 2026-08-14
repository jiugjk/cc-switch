import crossFetch, {
  Headers as CrossFetchHeaders,
  Request as CrossFetchRequest,
  Response as CrossFetchResponse,
} from "cross-fetch";
import { vi } from "vitest";
import { server } from "./server";

const TAURI_ENDPOINT = "http://tauri.local";

globalThis.fetch = crossFetch as typeof fetch;
globalThis.Headers = CrossFetchHeaders as typeof Headers;
globalThis.Request = CrossFetchRequest as typeof Request;
globalThis.Response = CrossFetchResponse as typeof Response;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string, payload: Record<string, unknown> = {}) => {
    const response = await fetch(`${TAURI_ENDPOINT}/${command}`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload ?? {}),
    });

    if (!response.ok) {
      const text = await response.text();
      throw new Error(text || `Invoke failed for ${command}`);
    }

    const text = await response.text();
    if (!text) return undefined;
    try {
      return JSON.parse(text);
    } catch {
      return text;
    }
  },
}));

const listeners = new Map<string, Set<(event: { payload: unknown }) => void>>();

const ensureListenerSet = (event: string) => {
  if (!listeners.has(event)) {
    listeners.set(event, new Set());
  }
  return listeners.get(event)!;
};

export const emitTauriEvent = (event: string, payload: unknown) => {
  const handlers = listeners.get(event);
  handlers?.forEach((handler) => handler({ payload }));
};

vi.mock("@tauri-apps/api/event", () => ({
  listen: async (
    event: string,
    handler: (event: { payload: unknown }) => void,
  ) => {
    const set = ensureListenerSet(event);
    set.add(handler);
    return () => {
      set.delete(handler);
    };
  },
}));

// Ensure the MSW server is referenced so tree shaking doesn't remove imports
void server;

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: async () => "/home/mock",
  join: async (...segments: string[]) => segments.join("/"),
}));

// App / useInstalledApps call getCurrentWindow() for decorations, maximize
// state, and focus. Without a stub, @tauri-apps/api throws on every mount
// (metadata of undefined) and pollutes logs / slows integration tests.
const createMockWindow = () => ({
  label: "main",
  isMaximized: async () => false,
  setDecorations: async () => {},
  minimize: async () => {},
  maximize: async () => {},
  unmaximize: async () => {},
  close: async () => {},
  startDragging: async () => {},
  onFocusChanged: async () => () => {},
  onResized: async () => () => {},
  listen: async () => () => {},
});

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => createMockWindow(),
  getCurrent: () => createMockWindow(),
  Window: {
    getCurrent: () => createMockWindow(),
  },
}));
