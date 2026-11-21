<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  ArrowRight,
  ArrowLeft,
  Check,
  ShieldCheck,
  Activity,
  SlidersHorizontal,
  Eye,
  EyeOff,
  KeyRound,
  Link2,
} from "@lucide/vue";
import Brand from "./Brand.vue";
import { api, setCsrf } from "../lib/api";
import type { User } from "../types";
const props = defineProps<{ setup: boolean; initialUsername?: string }>();
const emit = defineEmits<{
  login: [user: User];
  initialized: [username: string];
}>();
const username = ref(props.initialUsername || ""),
  password = ref(""),
  confirmPassword = ref("");
const token = ref(""),
  controller = ref(""),
  secret = ref("");
const step = ref(1),
  busy = ref(false),
  testing = ref(false),
  error = ref(""),
  showPassword = ref(false),
  tested = ref(false),
  version = ref("");
const strong = computed(() => Array.from(password.value).length >= 12);
watch([controller, secret], () => {
  tested.value = false;
  version.value = "";
});
function next() {
  error.value = "";
  if (!/^[a-zA-Z0-9._-]{3,32}$/.test(username.value)) {
    error.value = "用户名应为 3–32 位字母、数字、点、下划线或连字符";
    return;
  }
  if (!strong.value) {
    error.value = "请设置至少 12 个字符的密码";
    return;
  }
  if (password.value !== confirmPassword.value) {
    error.value = "两次输入的密码不一致";
    return;
  }
  if (!token.value.trim()) {
    error.value = "请输入一次性初始化令牌";
    return;
  }
  step.value = 2;
}
async function test() {
  if (!controller.value) {
    error.value = "请填写 Controller 地址";
    return;
  }
  testing.value = true;
  error.value = "";
  try {
    const result = await api<{ version: string }>("/api/setup/test", {
      method: "POST",
      body: {
        token: token.value.trim(),
        controller_url: controller.value.trim(),
        secret: secret.value,
      },
    });
    tested.value = true;
    version.value = result.version;
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : "连接测试失败";
  } finally {
    testing.value = false;
  }
}
async function submit() {
  if (busy.value) return;
  if (props.setup && step.value === 1) {
    next();
    return;
  }
  busy.value = true;
  error.value = "";
  try {
    if (props.setup) {
      if (!tested.value) {
        error.value = "请先测试 Mihomo 连接";
        return;
      }
      await api("/api/setup/complete", {
        method: "POST",
        body: {
          username: username.value,
          password: password.value,
          confirm_password: confirmPassword.value,
          token: token.value.trim(),
          controller_url: controller.value.trim(),
          secret: secret.value,
        },
      });
      password.value = "";
      confirmPassword.value = "";
      token.value = "";
      secret.value = "";
      controller.value = "";
      emit("initialized", username.value);
    } else {
      const user = await api<User>("/api/auth/login", {
        method: "POST",
        body: { username: username.value, password: password.value },
      });
      password.value = "";
      setCsrf(user.csrf_token);
      emit("login", user);
    }
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : "请求未完成，请重试";
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="auth-screen">
    <div class="auth-brand"><Brand /></div>
    <section class="auth-intro">
      <div class="eyebrow">
        <span class="status-dot" /> YOUR NETWORK. YOUR SPACE.
      </div>
      <h1>你的网络。<br /><span>尽在掌握。</span></h1>
      <p>
        一个安静、有序的网络管理空间。<br />连接、切换、观察，让每一次访问更从容。
      </p>
      <div class="auth-features">
        <span><SlidersHorizontal :size="17" /> 自由选择代理节点</span
        ><span><Activity :size="17" /> 实时洞察网络流量</span
        ><span><ShieldCheck :size="17" /> 独立、安全的登录保护</span>
      </div>
      <div class="auth-network-art" aria-hidden="true">
        <svg viewBox="0 0 400 140" fill="none">
          <path
            d="M30 70H130C170 70 170 26 210 26H340M130 70H340M130 70C170 70 170 114 210 114H340"
            stroke="currentColor"
            stroke-width="1.5"
          />
          <circle cx="30" cy="70" r="8" />
          <circle cx="130" cy="70" r="14" />
          <circle cx="340" cy="26" r="6" />
          <circle cx="340" cy="70" r="6" />
          <circle cx="340" cy="114" r="6" />
        </svg>
      </div>
    </section>
    <main class="auth-panel">
      <div v-if="setup" class="setup-steps">
        <span :class="{ active: step === 1, complete: step > 1 }"
          ><Check v-if="step > 1" :size="14" /><b v-else>01</b> 账户安全</span
        ><span class="step-line" /><span :class="{ active: step === 2 }"
          ><b>02</b> 连接内核</span
        >
      </div>
      <div class="auth-heading">
        <div class="auth-heading-icon">
          <KeyRound v-if="!setup || step === 1" :size="22" /><Link2
            v-else
            :size="22"
          />
        </div>
        <h2>
          {{
            setup
              ? step === 1
                ? "从一个安全的账户开始"
                : "连接你的 Mihomo"
              : "欢迎回来"
          }}
        </h2>
        <p>
          {{
            setup
              ? step === 1
                ? "完成首次配置，开启你的管理空间。"
                : "连接已运行的内核，所有请求由管理服务安全处理。"
              : "登录你的管理空间，继续掌握网络状态。"
          }}
        </p>
      </div>
      <form @submit.prevent="submit">
        <template v-if="!setup || step === 1">
          <label v-if="setup" class="field"
            >初始化令牌<input
              v-model="token"
              aria-label="初始化令牌"
              aria-describedby="bootstrap-help"
              name="bootstrap-token"
              autocomplete="off"
              spellcheck="false"
              type="password"
              placeholder="输入服务器上的一次性令牌"
              required
            /><small id="bootstrap-help">通过服务器上的 bootstrap-token 命令读取。</small></label
          >
          <label class="field"
            >用户名<input
              v-model="username"
              name="username"
              autocomplete="username"
              placeholder="你的管理员用户名"
              required
              maxlength="32"
          /></label>
          <label class="field"
            >{{ setup ? "设置密码" : "密码"
            }}<span class="password-input"
              ><input
                v-model="password"
                :aria-label="setup ? '设置密码' : '密码'"
                name="password"
                :type="showPassword ? 'text' : 'password'"
                :autocomplete="setup ? 'new-password' : 'current-password'"
                :placeholder="setup ? '至少 12 个字符' : '输入你的密码'"
                required
                maxlength="512" /><button
                type="button"
                :aria-label="showPassword ? '隐藏密码' : '显示密码'"
                @click="showPassword = !showPassword"
              >
                <EyeOff v-if="showPassword" :size="18" /><Eye
                  v-else
                  :size="18"
                /></button></span
          ></label>
          <template v-if="setup"
            ><div class="password-strength" :class="{ valid: strong }">
              <span /><span /><span /><small>{{
                strong ? "密码长度符合要求" : "建议使用多个不相关的词语"
              }}</small>
            </div>
            <label class="field"
              >确认密码<input
                v-model="confirmPassword"
                name="confirm-password"
                type="password"
                autocomplete="new-password"
                placeholder="再输入一次密码"
                required /></label
          ></template>
        </template>
        <template v-else>
          <label class="field"
            >Controller 地址<input
              v-model="controller"
              aria-label="Controller 地址"
              aria-describedby="setup-controller-help"
              name="controller"
              autocomplete="off"
              spellcheck="false"
              placeholder="填写本机或已授权的内网地址"
              required
            /><small id="setup-controller-help"
              >默认仅允许回环 IP。其他私网需在服务器配置中授权。</small
            ></label
          >
          <label class="field"
            >API Secret<input
              v-model="secret"
              aria-label="API Secret"
              aria-describedby="setup-secret-help"
              name="secret"
              type="password"
              autocomplete="new-password"
              placeholder="输入现有内核的 Secret"
            /><small id="setup-secret-help">仅保存在服务器。未设置 Secret 的内核可留空。</small></label
          >
          <button
            class="button button-secondary full-width"
            type="button"
            :disabled="testing || busy"
            @click="test"
          >
            <span v-if="testing" class="spinner" /><Check
              v-else-if="tested"
              :size="17"
            /><Link2 v-else :size="17" />{{
              testing
                ? "正在测试连接…"
                : tested
                  ? `已连接 · ${version}`
                  : "测试连接"
            }}
          </button>
        </template>
        <div v-if="error" class="form-error" role="alert">{{ error }}</div>
        <button
          class="button button-primary full-width auth-submit"
          type="submit"
          :disabled="busy || testing || (setup && step === 2 && !tested)"
        >
          <span v-if="busy" class="spinner" />{{
            busy
              ? setup
                ? "正在完成配置…"
                : "正在登录…"
              : setup
                ? step === 1
                  ? "下一步"
                  : "完成配置"
                : "进入管理空间"
          }}<ArrowRight v-if="!busy" :size="18" />
        </button>
        <button
          v-if="setup && step === 2"
          class="button button-quiet full-width"
          type="button"
          :disabled="busy"
          @click="
            step = 1;
            error = '';
          "
        >
          <ArrowLeft :size="16" />返回账户设置
        </button>
      </form>
      <div class="auth-assurance">
        <ShieldCheck :size="14" /><span
          >安全的服务端会话 · 凭据不会存入浏览器</span
        >
      </div>
    </main>
    <footer class="auth-footer">
      <span>MIHOMO WEB PANEL</span><span>独立管理 · 简单连接</span>
    </footer>
  </div>
</template>
