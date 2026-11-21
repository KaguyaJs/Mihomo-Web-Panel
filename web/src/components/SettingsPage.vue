<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  ShieldCheck,
  Link2,
  SlidersHorizontal,
  Power,
  Check,
  RefreshCw,
  Monitor,
  KeyRound,
  LogOut,
  Info,
  LockKeyhole,
} from "@lucide/vue";
import ConfirmDialog from "./ConfirmDialog.vue";
import { api, setCsrf } from "../lib/api";
import { duration } from "../lib/format";
import type {
  Autostart,
  BackendStatus,
  ServiceSettings,
  Session,
  User,
} from "../types";
const props = defineProps<{ user: User; status: BackendStatus }>();
const emit = defineEmits<{
  user: [user: User];
  backend: [];
  logout: [];
  notice: [message: string, kind: "success" | "error" | "info"];
}>();
const tab = ref("security"),
  loading = ref(true),
  busy = ref(""),
  error = ref("");
const accountName = ref(props.user.username),
  currentPassword = ref(""),
  newPassword = ref(""),
  confirmation = ref("");
const sessions = ref<Session[]>([]),
  controller = ref(""),
  secret = ref(""),
  tested = ref(false),
  testedVersion = ref("");
const service = ref<ServiceSettings | null>(null),
  listenHost = ref(""),
  listenPort = ref(8787),
  publicOrigin = ref(""),
  idleMinutes = ref(30),
  absoluteHours = ref(24),
  trafficInterval = ref(1000);
const autostart = ref<Autostart | null>(null),
  revokeScope = ref<"all" | "others" | null>(null);
const strong = computed(
  () => !newPassword.value || Array.from(newPassword.value).length >= 12,
);
const tabs = [
  { id: "security", label: "账户安全", icon: ShieldCheck },
  { id: "backend", label: "内核连接", icon: Link2 },
  { id: "service", label: "服务设置", icon: SlidersHorizontal },
  { id: "autostart", label: "开机自启", icon: Power },
];
function report(cause: unknown) {
  error.value = cause instanceof Error ? cause.message : "操作未完成";
  emit("notice", error.value, "error");
}
async function loadSessions() {
  sessions.value = (
    await api<{ sessions: Session[] }>("/api/settings/sessions")
  ).sessions;
}
onMounted(async () => {
  const results = await Promise.allSettled([
    loadSessions(),
    api<ServiceSettings>("/api/settings"),
    api<Autostart>("/api/settings/autostart"),
  ]);
  if (results[1]?.status === "fulfilled") {
    service.value = results[1].value;
    const settings = service.value.settings;
    const match =
      settings.listen.match(/^\[(.*)\]:(\d+)$/) ||
      settings.listen.match(/^(.*):(\d+)$/);
    listenHost.value = match?.[1] || "";
    listenPort.value = Number(match?.[2] || 8787);
    publicOrigin.value = settings.public_origin;
    idleMinutes.value = settings.session_idle_seconds / 60;
    absoluteHours.value = settings.session_absolute_seconds / 3600;
    trafficInterval.value = settings.traffic_interval_ms;
  } else if (results[1]?.status === "rejected") report(results[1].reason);
  if (results[2]?.status === "fulfilled") autostart.value = results[2].value;
  else if (results[2]?.status === "rejected") report(results[2].reason);
  if (results[0]?.status === "rejected") report(results[0].reason);
  loading.value = false;
});
async function saveAccount() {
  if (
    !strong.value ||
    (newPassword.value && newPassword.value !== confirmation.value)
  ) {
    error.value = !strong.value
      ? "新密码至少需要 12 个字符"
      : "两次输入的新密码不一致";
    return;
  }
  busy.value = "account";
  error.value = "";
  try {
    const user = await api<User>("/api/settings/security", {
      method: "PATCH",
      body: {
        username: accountName.value,
        current_password: currentPassword.value,
        new_password: newPassword.value || null,
        confirm_password: confirmation.value || null,
      },
    });
    setCsrf(user.csrf_token);
    emit("user", user);
    currentPassword.value = "";
    newPassword.value = "";
    confirmation.value = "";
    emit("notice", "账户已更新，其他会话已全部撤销", "success");
    await loadSessions();
  } catch (cause) {
    report(cause);
  } finally {
    busy.value = "";
  }
}
async function revoke() {
  if (!revokeScope.value) return;
  busy.value = "revoke";
  try {
    await api("/api/settings/sessions", {
      method: "DELETE",
      body: { scope: revokeScope.value },
    });
    if (revokeScope.value === "all") emit("logout");
    else {
      emit("notice", "其他会话已注销", "success");
      await loadSessions();
    }
    revokeScope.value = null;
  } catch (cause) {
    report(cause);
  } finally {
    busy.value = "";
  }
}
function backendChanged() {
  tested.value = false;
  testedVersion.value = "";
}
async function testBackend() {
  busy.value = "test";
  error.value = "";
  try {
    const result = await api<{ version: string }>(
      "/api/settings/backend/test",
      {
        method: "POST",
        body: { controller_url: controller.value.trim(), secret: secret.value },
      },
    );
    tested.value = true;
    testedVersion.value = result.version;
    emit("notice", "连接测试成功", "success");
  } catch (cause) {
    tested.value = false;
    report(cause);
  } finally {
    busy.value = "";
  }
}
async function saveBackend() {
  busy.value = "backend";
  error.value = "";
  try {
    await api("/api/settings/backend", {
      method: "PUT",
      body: { controller_url: controller.value.trim(), secret: secret.value },
    });
    controller.value = "";
    secret.value = "";
    tested.value = false;
    emit("backend");
    emit("notice", "内核连接配置已安全保存", "success");
  } catch (cause) {
    report(cause);
  } finally {
    busy.value = "";
  }
}
async function saveService() {
  busy.value = "service";
  error.value = "";
  try {
    const host = listenHost.value.trim();
    const listen = `${host.includes(":") ? `[${host}]` : host}:${listenPort.value}`;
    const settings = {
      listen,
      public_origin: publicOrigin.value.trim(),
      session_idle_seconds: Math.round(idleMinutes.value * 60),
      session_absolute_seconds: Math.round(absoluteHours.value * 3600),
      traffic_interval_ms: trafficInterval.value,
    };
    const result = await api<{ restart_required: boolean }>("/api/settings", {
      method: "PUT",
      body: settings,
    });
    if (service.value) {
      service.value.settings = settings;
      service.value.restart_required = result.restart_required;
    }
    emit(
      "notice",
      result.restart_required
        ? "设置已保存。监听与访问入口将在手动重启后生效"
        : "服务设置已保存并生效",
      "success",
    );
  } catch (cause) {
    report(cause);
  } finally {
    busy.value = "";
  }
}
async function toggleAutostart() {
  if (!autostart.value?.supported) return;
  busy.value = "autostart";
  error.value = "";
  try {
    autostart.value = await api<Autostart>("/api/settings/autostart", {
      method: "POST",
      body: { operation: autostart.value.enabled ? "disable" : "enable" },
    });
    emit(
      "notice",
      autostart.value.enabled ? "开机自启已开启" : "开机自启已关闭",
      "success",
    );
  } catch (cause) {
    report(cause);
  } finally {
    busy.value = "";
  }
}
</script>

<template>
  <div class="settings-page">
    <nav class="settings-tabs" aria-label="设置分类">
      <button
        v-for="item in tabs"
        :key="item.id"
        :class="{ active: tab === item.id }"
        @click="
          tab = item.id;
          error = '';
        "
      >
        <component :is="item.icon" :size="17" />{{ item.label }}
      </button>
    </nav>
    <div v-if="loading" class="settings-loading">
      <div class="skeleton" />
      <div class="skeleton" />
    </div>
    <div v-else class="settings-content">
      <div v-if="error" class="form-error" role="alert">{{ error }}</div>
      <template v-if="tab === 'security'"
        ><section class="surface settings-section">
          <div class="section-heading">
            <div>
              <h2>管理员账户</h2>
              <p>更新账户信息，保护你的管理空间。</p>
            </div>
            <KeyRound :size="20" class="text-muted" />
          </div>
          <form @submit.prevent="saveAccount">
            <div class="form-grid">
              <label class="field"
                >管理员用户名<input
                  v-model="accountName"
                  autocomplete="username"
                  required
                  maxlength="32" /></label
              ><label class="field"
                >原密码<input
                  v-model="currentPassword"
                  type="password"
                  autocomplete="current-password"
                  placeholder="修改前需要验证原密码"
                  required /></label
              ><label class="field"
                >新密码<input
                  v-model="newPassword"
                  aria-label="新密码"
                  aria-describedby="new-password-help"
                  type="password"
                  autocomplete="new-password"
                  placeholder="至少 12 个字符，不修改可留空"
                /><small id="new-password-help" :class="{ 'text-error': !strong }">{{
                  strong
                    ? "建议使用多个不相关的词语组成长密码"
                    : "新密码至少需要 12 个字符"
                }}</small></label
              ><label class="field"
                >确认新密码<input
                  v-model="confirmation"
                  type="password"
                  autocomplete="new-password"
                  placeholder="再次输入新密码"
              /></label>
            </div>
            <div class="settings-form-footer">
              <span
                ><LockKeyhole
                  :size="14"
                />更新后，其他登录会话会自动注销。</span
              ><button class="button button-primary" :disabled="!!busy">
                <span v-if="busy === 'account'" class="spinner" />更新账户
              </button>
            </div>
          </form>
        </section>
        <section class="surface settings-section">
          <div class="section-heading">
            <div>
              <h2>
                登录会话<span class="count-badge">{{ sessions.length }}</span>
              </h2>
              <p>查看和管理正在访问此账户的设备。</p>
            </div>
            <button
              class="icon-button"
              aria-label="刷新登录会话"
              @click="loadSessions().catch(report)"
            >
              <RefreshCw :size="16" />
            </button>
          </div>
          <div
            v-for="session in sessions"
            :key="session.id"
            class="session-row"
          >
            <span class="session-icon"><Monitor :size="20" /></span>
            <div>
              <strong
                >{{ session.current ? "当前设备" : "其他设备"
                }}<span v-if="session.current" class="current-badge"
                  >当前会话</span
                ></strong
              ><span
                >{{ session.ip }} ·
                {{
                  new Date(session.last_seen * 1000).toLocaleString("zh-CN")
                }}</span
              ><small>{{ session.user_agent || "设备信息未提供" }}</small>
            </div>
          </div>
          <div class="session-actions">
            <button
              class="button button-secondary"
              :disabled="!!busy || sessions.length < 2"
              @click="revokeScope = 'others'"
            >
              <LogOut :size="16" />注销其他会话</button
            ><button
              class="button button-danger-soft"
              :disabled="!!busy"
              @click="revokeScope = 'all'"
            >
              注销全部会话
            </button>
          </div>
        </section>
      </template>
      <template v-if="tab === 'backend'"
        ><section class="surface settings-section">
          <div class="section-heading">
            <div>
              <h2>Mihomo 内核连接</h2>
              <p>连接独立运行的 Mihomo 实例。</p>
            </div>
            <span
              class="connection-status"
              :class="{ offline: !status.connected }"
              ><span class="status-dot" />{{
                status.connected ? "已连接" : "暂时离线"
              }}</span
            >
          </div>
          <div class="backend-saved-note">
            <ShieldCheck :size="19" />
            <div>
              <strong>{{
                status.configured
                  ? "当前连接配置已安全保存"
                  : "尚未保存连接配置"
              }}</strong>
              <p>修改时请重新填写地址和 Secret。已保存的凭据不会回显。</p>
            </div>
            <span v-if="status.version" class="version-tag">{{
              status.version
            }}</span>
          </div>
          <form @submit.prevent="saveBackend">
            <label class="field"
              >Controller 地址<input
                v-model="controller"
                aria-label="Controller 地址"
                aria-describedby="backend-controller-help"
                autocomplete="off"
                spellcheck="false"
                placeholder="重新输入本机或已授权的内网地址"
                required
                @input="backendChanged"
              /><small id="backend-controller-help"
                >默认仅允许回环 IP。可信私网地址需由服务器配置授权。</small
              ></label
            ><label class="field"
              >API Secret<input
                v-model="secret"
                aria-label="API Secret"
                aria-describedby="backend-secret-help"
                type="password"
                autocomplete="new-password"
                placeholder="重新设置 API Secret"
                @input="backendChanged"
              /><small id="backend-secret-help">内核未启用 Secret 时可留空。</small></label
            >
            <div v-if="tested" class="test-success">
              <Check :size="16" />连接测试成功 · {{ testedVersion }}
            </div>
            <div class="settings-form-footer">
              <button
                type="button"
                class="button button-secondary"
                :disabled="!!busy || !controller"
                @click="testBackend"
              >
                <span v-if="busy === 'test'" class="spinner" /><Link2
                  v-else
                  :size="16"
                />测试连接</button
              ><button
                class="button button-primary"
                :disabled="!!busy || !tested"
              >
                <span v-if="busy === 'backend'" class="spinner" />保存连接配置
              </button>
            </div>
          </form>
        </section></template
      >
      <template v-if="tab === 'service'"
        ><section v-if="service" class="surface settings-section">
          <div class="section-heading">
            <div>
              <h2>管理服务</h2>
              <p>让服务按照你的习惯运行。</p>
            </div>
            <span class="version-tag">v{{ service.version }}</span>
          </div>
          <div class="system-facts">
            <span
              >运行平台<strong
                >{{ service.platform }} / {{ service.architecture }}</strong
              ></span
            ><span
              >运行时间<strong>{{
                duration(service.uptime_seconds)
              }}</strong></span
            ><span
              >安全 Cookie<strong>{{
                service.secure_cookies ? "已启用" : "本机开发环境"
              }}</strong></span
            >
          </div>
          <form @submit.prevent="saveService">
            <div class="form-grid">
              <label class="field"
                >监听地址<input
                  v-model="listenHost"
                  autocomplete="off"
                  required /></label
              ><label class="field"
                >监听端口<input
                  v-model.number="listenPort"
                  type="number"
                  min="1024"
                  max="65535"
                  required
              /></label>
            </div>
            <label class="field"
              >Web 访问入口<input
                v-model="publicOrigin"
                autocomplete="off"
                required
              /><small>填写完整的同源入口。公网访问须使用 HTTPS。</small></label
            >
            <div class="settings-restart-note">
              <Info :size="17" /><span
                >监听地址、端口和访问入口仅在手动重启服务后生效。请先准备好反向代理和服务器访问方式。</span
              >
            </div>
            <div class="form-grid">
              <label class="field"
                >会话空闲超时（分钟）<input
                  v-model.number="idleMinutes"
                  type="number"
                  min="5"
                  max="1440"
                  required /></label
              ><label class="field"
                >会话最长时间（小时）<input
                  v-model.number="absoluteHours"
                  type="number"
                  min="1"
                  max="168"
                  required /></label
              ><label class="field"
                >流量刷新频率<select v-model.number="trafficInterval" aria-label="流量刷新频率" aria-describedby="traffic-interval-help">
                  <option :value="500">0.5 秒</option>
                  <option :value="1000">1 秒</option>
                  <option :value="2000">2 秒</option>
                  <option :value="5000">5 秒</option></select
                ><small id="traffic-interval-help">实际数据频率受内核采样频率限制。</small></label
              >
            </div>
            <div class="settings-form-footer">
              <span v-if="service.restart_required" class="restart-required"
                >有待重启生效的监听设置</span
              ><span v-else>会话与刷新设置保存后立即生效。</span
              ><button class="button button-primary" :disabled="!!busy">
                <span v-if="busy === 'service'" class="spinner" />保存服务设置
              </button>
            </div>
          </form>
        </section>
        <div v-else class="empty-state">
          服务设置加载失败，请刷新后重试。
        </div></template
      >
      <template v-if="tab === 'autostart'"
        ><section class="surface settings-section">
          <div class="section-heading">
            <div>
              <h2>开机自启</h2>
              <p>在服务器重新启动后，自动恢复管理服务。</p>
            </div>
            <Power :size="20" class="text-muted" />
          </div>
          <div v-if="autostart?.supported" class="autostart-control">
            <div class="autostart-icon"><Power :size="24" /></div>
            <div>
              <strong>{{
                autostart.enabled ? "开机自启已开启" : "开机自启已关闭"
              }}</strong>
              <p>systemd 服务状态：{{ autostart.active }}</p>
            </div>
            <button
              class="switch"
              role="switch"
              :aria-checked="autostart.enabled"
              aria-label="管理服务开机自启"
              :class="{ enabled: autostart.enabled }"
              :disabled="!!busy"
              @click="toggleAutostart"
            >
              <span />
            </button>
          </div>
          <div v-else class="autostart-unavailable">
            <Monitor :size="32" />
            <h3>当前环境暂不支持此操作</h3>
            <p>
              {{
                autostart?.message ||
                "权限辅助程序未能返回状态。请检查安装和服务日志。"
              }}
            </p>
            <p>
              请查看项目中的 Linux 安装指南，安装 systemd
              服务及专用权限辅助程序。
            </p>
          </div>
          <div class="settings-restart-note">
            <ShieldCheck :size="17" /><span
              >主服务以普通用户运行。开机自启仅通过受限辅助程序控制本项目服务。</span
            >
          </div>
        </section></template
      >
    </div>
    <ConfirmDialog
      :open="!!revokeScope"
      :title="
        revokeScope === 'all' ? '注销全部登录会话？' : '注销其他登录会话？'
      "
      :description="
        revokeScope === 'all'
          ? '包括当前设备在内的所有设备都需要重新登录。'
          : '其他设备将立即失去访问权限，当前设备可以继续使用。'
      "
      label="确认注销"
      :busy="busy === 'revoke'"
      @close="revokeScope = null"
      @confirm="revoke"
    />
  </div>
</template>
