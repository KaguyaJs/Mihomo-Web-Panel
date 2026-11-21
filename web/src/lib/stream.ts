import type { LogEntry, Traffic } from "../types";

export function createStream(
  path: "/api/traffic" | "/api/logs",
  receive: (message: {
    type: string;
    state?: string;
    data?: Traffic | LogEntry;
  }) => void,
  stateChanged: (state: string) => void,
) {
  let socket: WebSocket | null = null;
  let retry: ReturnType<typeof setTimeout> | undefined;
  let stopped = true;
  let backoff = 1000;
  let generation = 0;
  function connect() {
    if (stopped || socket || document.hidden) return;
    const current = ++generation;
    const url = new URL(path, window.location.origin);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    stateChanged("connecting");
    socket = new WebSocket(url);
    socket.onopen = () => {
      if (current === generation) {
        backoff = 1000;
        stateChanged("online");
      }
    };
    socket.onmessage = (event) => {
      if (current !== generation) return;
      try {
        const message = JSON.parse(String(event.data));
        if (message.type === "status") stateChanged(message.state);
        receive(message);
      } catch {
        stateChanged("invalid_data");
      }
    };
    socket.onerror = () => {
      if (current === generation) stateChanged("offline");
    };
    socket.onclose = () => {
      if (current !== generation) return;
      socket = null;
      stateChanged("offline");
      if (!stopped && !document.hidden) {
        retry = setTimeout(connect, backoff);
        backoff = Math.min(backoff * 2, 30000);
      }
    };
  }
  function stop() {
    stopped = true;
    generation++;
    if (retry) clearTimeout(retry);
    if (socket) {
      socket.onclose = null;
      socket.close();
      socket = null;
    }
    stateChanged("paused");
  }
  function start() {
    if (!stopped) return;
    stopped = false;
    connect();
  }
  return { start, stop };
}
