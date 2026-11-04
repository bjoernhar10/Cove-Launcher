/**
 * Plugin worker entry (bundled inline as a blob worker, so it inherits the
 * app's Content Security Policy: modules only from `cove-plugin:`, no
 * network). Before the plugin loads, every way to reach the network, other
 * storage or new workers is removed from the global scope.
 */
import { runPlugin, type RuntimePort } from "./runtime.ts";

const BLOCKED = [
  "fetch",
  "XMLHttpRequest",
  "WebSocket",
  "WebTransport",
  "EventSource",
  "importScripts",
  "indexedDB",
  "caches",
  "Worker",
  "SharedWorker",
  "BroadcastChannel",
] as const;

for (const name of BLOCKED) {
  try {
    Object.defineProperty(globalThis, name, { value: undefined, writable: false, configurable: false });
  } catch {
    // Already non-configurable in this engine: it is not reachable either way.
  }
}

runPlugin(globalThis as unknown as RuntimePort, (url) => import(/* @vite-ignore */ url));
