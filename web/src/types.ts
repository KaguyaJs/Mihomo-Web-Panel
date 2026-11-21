export interface User {
  username: string;
  csrf_token: string;
}
export type Mode = "rule" | "global" | "direct";
export type Page = "overview" | "proxies" | "connections" | "logs" | "settings";
export type Theme = "light" | "dark" | "system";
export interface History {
  time: string;
  delay: number;
}
export interface ProxyNode {
  name: string;
  type: string;
  all: string[];
  now: string | null;
  history: History[];
  udp: boolean;
  alive: boolean | null;
}
export interface ProxyResponse {
  proxies: Record<string, ProxyNode>;
}
export interface BackendStatus {
  configured: boolean;
  connected: boolean;
  version: string | null;
  message?: string;
  error?: string;
}
export interface Traffic {
  up: number;
  down: number;
  upTotal?: number;
  downTotal?: number;
}
export interface TrafficPoint {
  up: number;
  down: number;
  time: number;
}
export interface Connection {
  id: string;
  network: string;
  source: string;
  destination: string;
  host: string;
  chains: string[];
  upload: number;
  download: number;
  start: string;
  rule: string;
  process: string;
}
export interface Connections {
  downloadTotal: number;
  uploadTotal: number;
  connections: Connection[];
}
export interface LogEntry {
  type: string;
  payload: string;
  timestamp: number;
}
export interface DelayResult {
  name: string;
  delay: number | null;
  status: "ok" | "timeout" | "unavailable" | "testing" | "busy";
}
export interface Session {
  id: string;
  current: boolean;
  created_at: number;
  last_seen: number;
  expires_at: number;
  ip: string;
  user_agent: string;
}
export interface WebSettings {
  listen: string;
  public_origin: string;
  session_idle_seconds: number;
  session_absolute_seconds: number;
  traffic_interval_ms: number;
}
export interface ServiceSettings {
  version: string;
  uptime_seconds: number;
  platform: string;
  architecture: string;
  active_listen: string;
  active_origin: string;
  secure_cookies: boolean;
  restart_required: boolean;
  settings: WebSettings;
}
export interface Autostart {
  supported: boolean;
  enabled: boolean;
  active: string;
  message: string;
}
export interface Toast {
  id: number;
  message: string;
  kind: "success" | "error" | "info";
}
