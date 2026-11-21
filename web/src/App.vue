<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { usePreferredDark } from "@vueuse/core";
import {
  LayoutDashboard,
  Waypoints,
  Activity,
  Terminal,
  Settings2,
  LogOut,
  Moon,
  Sun,
  Monitor,
  RefreshCw,
  ShieldCheck,
  ChevronRight,
  ArrowUpRight,
  WifiOff,
  X,
  CheckCircle2,
  AlertCircle,
  Menu,
} from "@lucide/vue";
import Brand from "./components/Brand.vue";
import AuthScreen from "./components/AuthScreen.vue";
import OverviewPage from "./components/OverviewPage.vue";
import ProxiesPage from "./components/ProxiesPage.vue";
import ConnectionsPage from "./components/ConnectionsPage.vue";
import LogsPage from "./components/LogsPage.vue";
import SettingsPage from "./components/SettingsPage.vue";
import { api, setCsrf } from "./lib/api";
import { createStream } from "./lib/stream";
import type {
  BackendStatus,
  Connections,
  Mode,
  Page,
  ProxyNode,
  ProxyResponse,
  Theme,
  Toast,
  Traffic,
  TrafficPoint,
  User,
} from "./types";

const phase = ref<"loading" | "setup" | "login" | "app">("loading"),
  startupError = ref(""),
  initialUsername = ref("");
const user = ref<User | null>(null),
  page = ref<Page>("overview"),
  refreshing = ref(false),
  coreLoaded = ref(false),
  modeBusy = ref(false),
  mobileMenu = ref(false);
const backend = ref<BackendStatus>({
    configured: false,
    connected: false,
    version: null,
  }),
  mode = ref<Mode>("rule"),
  proxies = ref<Record<string, ProxyNode>>({});
const connections = ref<Connections>({
    connections: [],
    downloadTotal: 0,
    uploadTotal: 0,
  }),
  traffic = ref<Traffic>({ up: 0, down: 0 }),
  points = ref<TrafficPoint[]>([]),
  streamState = ref("connecting");
const activities = ref<{ message: string; time: string }[]>([]),
  toasts = ref<Toast[]>([]);
const nativeTotals = ref(false);
let toastId = 0,
  polling: ReturnType<typeof setInterval> | undefined,
  pollBusy = false,
  lastHeartbeat = 0;
const preferredDark = usePreferredDark();
function savedTheme(): Theme {
  try {
    const value = localStorage.getItem("mwp-theme");
    return value === "light" || value === "dark" ? value : "system";
  } catch {
    return "system";
  }
}
const theme = ref<Theme>(savedTheme());
const dark = computed(
  () =>
    theme.value === "dark" || (theme.value === "system" && preferredDark.value),
);
watch(
  [theme, dark],
  () => {
    document.documentElement.dataset.theme = dark.value ? "dark" : "light";
    try {
      localStorage.setItem("mwp-theme", theme.value);
    } catch {}
  },
  { immediate: true },
);
const navigation = [
  {
    id: "overview" as const,
    label: "网络概览",
    icon: LayoutDashboard,
    caption: "你的网络状态，一目了然。",
  },
  {
    id: "proxies" as const,
    label: "代理节点",
    icon: Waypoints,
    caption: "找到更好的连接，让访问一路顺畅。",
  },
  {
    id: "connections" as const,
    label: "连接管理",
    icon: Activity,
    caption: "每一个正在进行的连接，都清晰可见。",
  },
  {
    id: "logs" as const,
    label: "实时日志",
    icon: Terminal,
    caption: "观察内核运行，快速定位网络问题。",
  },
  {
    id: "settings" as const,
    label: "设置",
    icon: Settings2,
    caption: "按照你的习惯，调整管理空间。",
  },
];
const currentPage = computed(
  () => navigation.find((item) => item.id === page.value) || navigation[0]!,
);
function navigate(target: Page) {
  page.value = target;
  mobileMenu.value = false;
  history.replaceState(null, "", `#${target}`);
  window.scrollTo({ top: 0 });
  if (target === "connections") void refreshConnections();
  if (target === "proxies") void refreshProxies();
}
function hashPage() {
  const value = location.hash.slice(1);
  if (navigation.some((item) => item.id === value)) page.value = value as Page;
}
function notice(message: string, kind: Toast["kind"] = "success") {
  const id = toastId++;
  toasts.value = [...toasts.value.slice(-3), { id, message, kind }];
  setTimeout(
    () => {
      toasts.value = toasts.value.filter((item) => item.id !== id);
    },
    kind === "error" ? 7000 : 4500,
  );
  if (kind !== "error") {
    activities.value.unshift({
      message,
      time: new Date().toLocaleTimeString("zh-CN", {
        hour: "2-digit",
        minute: "2-digit",
      }),
    });
    activities.value = activities.value.slice(0, 12);
  }
}
const trafficStream = createStream(
  "/api/traffic",
  (message) => {
    if (message.type === "traffic" && message.data) {
      const sample = message.data as Traffic;
      if (sample.downTotal !== undefined || sample.upTotal !== undefined)
        nativeTotals.value = true;
      traffic.value = { ...traffic.value, ...sample };
      points.value = [
        ...points.value.slice(-59),
        { up: sample.up, down: sample.down, time: Date.now() },
      ];
    }
  },
  (state) => {
    streamState.value = state;
    if (state === "offline")
      traffic.value = { ...traffic.value, up: 0, down: 0 };
  },
);

async function load() {
  phase.value = "loading";
  startupError.value = "";
  try {
    const setup = await api<{ required: boolean }>("/api/setup/status");
    if (setup.required) {
      phase.value = "setup";
      return;
    }
    try {
      const current = await api<User>("/api/auth/me");
      enter(current);
    } catch (cause) {
      if (cause instanceof Error && "status" in cause && cause.status === 401)
        phase.value = "login";
      else throw cause;
    }
  } catch (cause) {
    startupError.value =
      cause instanceof Error ? cause.message : "管理服务无法访问";
  }
}
function enter(current: User) {
  user.value = current;
  setCsrf(current.csrf_token);
  phase.value = "app";
  hashPage();
  coreLoaded.value = false;
  void refreshCore();
  if (!document.hidden) trafficStream.start();
  if (polling) clearInterval(polling);
  let count = 0;
  polling = setInterval(async () => {
    if (document.hidden || phase.value !== "app" || pollBusy) return;
    pollBusy = true;
    count++;
    try {
      const operations: Promise<unknown>[] = [];
      if (count % 3 === 0) operations.push(refreshStatus());
      if (page.value === "overview" || page.value === "connections")
        operations.push(refreshConnections(false));
      if (page.value === "proxies" && count % 5 === 0)
        operations.push(refreshProxies(false));
      await Promise.allSettled(operations);
    } finally {
      pollBusy = false;
    }
  }, 3000);
}
function signedOut() {
  trafficStream.stop();
  if (polling) clearInterval(polling);
  polling = undefined;
  setCsrf("");
  user.value = null;
  proxies.value = {};
  connections.value = { connections: [], uploadTotal: 0, downloadTotal: 0 };
  points.value = [];
  traffic.value = { up: 0, down: 0 };
  nativeTotals.value = false;
  phase.value = "login";
}
function expired() {
  if (phase.value === "app") {
    signedOut();
    notice("登录已过期，请重新登录", "info");
  }
}
async function logout() {
  try {
    await api("/api/auth/logout", { method: "POST" });
    signedOut();
  } catch (cause) {
    notice(cause instanceof Error ? cause.message : "退出登录失败", "error");
  }
}
async function refreshStatus() {
  backend.value = await api<BackendStatus>("/api/status");
}
async function refreshConnections(report = true) {
  try {
    const result = await api<Connections>("/api/connections");
    connections.value = result;
    if (!nativeTotals.value)
      traffic.value = {
        ...traffic.value,
        downTotal: result.downloadTotal,
        upTotal: result.uploadTotal,
      };
  } catch (cause) {
    if (report)
      notice(
        cause instanceof Error ? cause.message : "连接列表刷新失败",
        "error",
      );
  }
}
async function refreshProxies(report = true) {
  try {
    proxies.value = (await api<ProxyResponse>("/api/proxies")).proxies;
  } catch (cause) {
    if (report)
      notice(cause instanceof Error ? cause.message : "节点刷新失败", "error");
  }
}
async function refreshCore() {
  if (refreshing.value) return;
  refreshing.value = true;
  const results = await Promise.allSettled([
    refreshStatus(),
    refreshProxies(false),
    refreshConnections(false),
    api<{ mode: Mode }>("/api/config"),
  ]);
  if (results[3]?.status === "fulfilled") mode.value = results[3].value.mode;
  if (results[0]?.status === "rejected")
    backend.value = {
      ...backend.value,
      connected: false,
      message:
        results[0].reason instanceof Error
          ? results[0].reason.message
          : "连接未完成",
    };
  refreshing.value = false;
  coreLoaded.value = true;
}
async function changeMode(target: Mode) {
  if (modeBusy.value || target === mode.value) return;
  modeBusy.value = true;
  try {
    await api("/api/config", { method: "PATCH", body: { mode: target } });
    mode.value = (await api<{ mode: Mode }>("/api/config")).mode;
    notice(
      `代理模式已切换为${mode.value === "rule" ? "规则" : mode.value === "global" ? "全局" : "直连"}模式`,
    );
  } catch (cause) {
    notice(cause instanceof Error ? cause.message : "模式切换失败", "error");
  } finally {
    modeBusy.value = false;
  }
}
function updateGroup(name: string, proxy: ProxyNode) {
  proxies.value = { ...proxies.value, [name]: proxy };
}
function visibility() {
  if (document.hidden) trafficStream.stop();
  else if (phase.value === "app") {
    trafficStream.start();
    void refreshStatus().catch(() => {});
  }
}
function interaction() {
  if (
    phase.value !== "app" ||
    Date.now() - lastHeartbeat < 60000 ||
    document.hidden
  )
    return;
  lastHeartbeat = Date.now();
  void api<User>("/api/auth/me").catch(() => {});
}
function keyboard(event: KeyboardEvent) {
  if (
    event.key === "/" &&
    page.value === "proxies" &&
    !(event.target instanceof HTMLInputElement) &&
    !(event.target instanceof HTMLTextAreaElement)
  ) {
    event.preventDefault();
    document.querySelector<HTMLInputElement>(".search-input input")?.focus();
  }
}
onMounted(() => {
  void load();
  window.addEventListener("session-expired", expired);
  document.addEventListener("visibilitychange", visibility);
  window.addEventListener("pointerdown", interaction);
  window.addEventListener("keydown", interaction);
  window.addEventListener("keydown", keyboard);
  window.addEventListener("hashchange", hashPage);
});
onBeforeUnmount(() => {
  trafficStream.stop();
  if (polling) clearInterval(polling);
  window.removeEventListener("session-expired", expired);
  document.removeEventListener("visibilitychange", visibility);
  window.removeEventListener("pointerdown", interaction);
  window.removeEventListener("keydown", interaction);
  window.removeEventListener("keydown", keyboard);
  window.removeEventListener("hashchange", hashPage);
});
</script>

<template>
  <div v-if="phase === 'loading'" class="startup-screen">
    <Brand />
    <div v-if="!startupError" class="startup-loader">
      <span class="spinner" />正在打开管理空间
    </div>
    <div v-else class="startup-error">
      <AlertCircle :size="26" />
      <p>{{ startupError }}</p>
      <button class="button button-primary" @click="load">重新连接</button>
    </div>
  </div>
  <Transition
    v-else-if="phase === 'setup' || phase === 'login'"
    name="page"
    mode="out-in"
    ><AuthScreen
      :key="phase"
      :setup="phase === 'setup'"
      :initial-username="initialUsername"
      @login="enter"
      @initialized="
        (name) => {
          initialUsername = name;
          phase = 'login';
          notice('首次配置已完成，请登录你的账户');
        }
      "
  /></Transition>
  <div v-else-if="user" class="app-shell">
    <aside class="sidebar" :class="{ 'mobile-open': mobileMenu }">
      <Brand />
      <div class="sidebar-section-label">工作空间</div>
      <nav class="sidebar-nav" aria-label="主导航">
        <button
          v-for="item in navigation"
          :key="item.id"
          :class="{ active: page === item.id }"
          @click="navigate(item.id)"
        >
          <component :is="item.icon" :size="19" /><span>{{ item.label }}</span
          ><ChevronRight
            v-if="page === item.id"
            :size="14"
            class="nav-chevron"
          />
        </button>
      </nav>
      <div class="sidebar-bottom">
        <div class="sidebar-security">
          <ShieldCheck :size="17" /><span
            >安全的管理空间<small>服务端连接 · 凭据保护</small></span
          ><span class="mini-dot green" />
        </div>
        <div class="profile">
          <span class="avatar">{{
            user.username.slice(0, 1).toUpperCase()
          }}</span
          ><span
            ><strong>{{ user.username }}</strong
            ><small>管理员</small></span
          ><button class="icon-button" aria-label="退出登录" @click="logout">
            <LogOut :size="17" />
          </button>
        </div>
      </div>
    </aside>
    <div class="workspace">
      <header class="topbar">
        <div class="breadcrumb">
          <span>工作空间</span><ChevronRight :size="13" /><strong>{{
            currentPage.label
          }}</strong>
        </div>
        <button
          class="mobile-brand"
          aria-label="打开导航"
          @click="mobileMenu = !mobileMenu"
        >
          <Brand compact /><Menu :size="17" />
        </button>
        <div class="topbar-actions">
          <span
            class="connection-status"
            :class="{ offline: !backend.connected }"
            ><span class="status-dot" />{{
              backend.connected ? "内核已连接" : "内核离线"
            }}</span
          ><label class="theme-select"
            ><component
              :is="theme === 'system' ? Monitor : dark ? Moon : Sun"
              :size="17"
            /><select v-model="theme" aria-label="切换主题">
              <option value="system">跟随系统</option>
              <option value="light">浅色主题</option>
              <option value="dark">深色主题</option>
            </select></label
          ><span class="topbar-divider" /><span class="topbar-avatar">{{
            user.username.slice(0, 1).toUpperCase()
          }}</span>
        </div>
      </header>
      <main class="main-content">
        <div class="page-heading">
          <div>
            <div class="eyebrow">
              {{
                page === "overview"
                  ? "NETWORK OVERVIEW"
                  : page === "proxies"
                    ? "PROXY NODES"
                    : page === "connections"
                      ? "ACTIVE CONNECTIONS"
                      : page === "logs"
                        ? "LIVE LOGS"
                        : "YOUR PREFERENCES"
              }}
            </div>
            <h1>{{ currentPage.label }}</h1>
            <p>{{ currentPage.caption }}</p>
          </div>
          <button
            v-if="page === 'overview'"
            class="button button-secondary heading-refresh"
            :disabled="refreshing"
            @click="refreshCore"
          >
            <RefreshCw :size="16" :class="{ spinning: refreshing }" />刷新状态
          </button>
        </div>
        <div
          v-if="coreLoaded && !backend.connected"
          class="offline-banner"
          role="status"
        >
          <WifiOff :size="19" />
          <div>
            <strong>Mihomo 暂时离线</strong
            ><span>{{
              backend.message || "管理服务仍在运行，内核恢复后会自动重连。"
            }}</span>
          </div>
          <button class="text-button" @click="navigate('settings')">
            检查设置<ArrowUpRight :size="15" />
          </button>
        </div>
        <div v-if="!coreLoaded" class="dashboard-skeleton">
          <div class="skeleton" />
          <div class="skeleton" />
          <div class="skeleton" />
        </div>
        <Transition v-else name="page" mode="out-in"
          ><OverviewPage
            v-if="page === 'overview'"
            :status="backend"
            :mode="mode"
            :mode-busy="modeBusy"
            :proxies="proxies"
            :traffic="traffic"
            :points="points"
            :connection-count="connections.connections.length"
            :stream-state="streamState"
            :activities="activities"
            @navigate="navigate"
            @mode="changeMode" /><ProxiesPage
            v-else-if="page === 'proxies'"
            :proxies="proxies"
            :loading="refreshing"
            :connected="backend.connected"
            @refresh="refreshProxies()"
            @update="updateGroup"
            @notice="notice" /><ConnectionsPage
            v-else-if="page === 'connections'"
            :data="connections"
            :loading="refreshing"
            :connected="backend.connected"
            @refresh="refreshConnections()"
            @notice="notice" /><LogsPage
            v-else-if="page === 'logs'" /><SettingsPage
            v-else
            :user="user"
            :status="backend"
            @user="
              (current) => {
                user = current;
              }
            "
            @backend="refreshCore"
            @logout="signedOut"
            @notice="notice"
        /></Transition>
        <footer class="workspace-footer">
          <span
            >mihomo web panel<span class="tiny-separator">/</span
            >专注每一次连接</span
          ><span><ShieldCheck :size="12" />安全的同源连接</span>
        </footer>
      </main>
    </div>
    <nav class="mobile-nav" aria-label="移动端导航">
      <button
        v-for="item in navigation"
        :key="item.id"
        :class="{ active: page === item.id }"
        @click="navigate(item.id)"
      >
        <component :is="item.icon" :size="20" /><span>{{
          item.id === "overview"
            ? "概览"
            : item.id === "proxies"
              ? "节点"
              : item.id === "connections"
                ? "连接"
                : item.id === "logs"
                  ? "日志"
                  : "设置"
        }}</span>
      </button>
    </nav>
    <div v-if="mobileMenu" class="sidebar-scrim" @click="mobileMenu = false" />
  </div>
  <Teleport to="body"
    ><div class="toast-stack" aria-live="polite">
      <TransitionGroup name="toast"
        ><div
          v-for="toast in toasts"
          :key="toast.id"
          class="toast-message"
          :class="`toast-${toast.kind}`"
        >
          <CheckCircle2
            v-if="toast.kind === 'success'"
            :size="19"
          /><AlertCircle v-else :size="19" /><span>{{ toast.message }}</span
          ><button
            class="icon-button"
            aria-label="关闭通知"
            @click="toasts = toasts.filter((item) => item.id !== toast.id)"
          >
            <X :size="14" />
          </button></div
      ></TransitionGroup></div
  ></Teleport>
</template>
