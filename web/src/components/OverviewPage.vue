<script setup lang="ts">
import { computed } from "vue";
import {
  ArrowDownLeft,
  ArrowUpRight,
  ArrowRight,
  Radio,
  Activity,
  ShieldCheck,
  Globe2,
  Clock3,
  CheckCircle2,
} from "@lucide/vue";
import TrafficChart from "./TrafficChart.vue";
import {
  bytes,
  formatBytes,
  lastDelay,
  delayClass,
  nodeRegion,
} from "../lib/format";
import type {
  BackendStatus,
  Mode,
  ProxyNode,
  Traffic,
  TrafficPoint,
} from "../types";
const props = defineProps<{
  status: BackendStatus;
  mode: Mode;
  modeBusy: boolean;
  proxies: Record<string, ProxyNode>;
  traffic: Traffic;
  points: TrafficPoint[];
  connectionCount: number;
  streamState: string;
  activities: { message: string; time: string }[];
}>();
const emit = defineEmits<{
  navigate: [page: "proxies" | "connections"];
  mode: [mode: Mode];
}>();
const groups = computed(() =>
  Object.values(props.proxies)
    .filter((proxy) => proxy.all?.length)
    .sort(
      (a, b) =>
        (a.type === "Selector" ? -1 : 0) - (b.type === "Selector" ? -1 : 0),
    ),
);
const primary = computed(
  () =>
    groups.value.find((group) => group.type === "Selector") || groups.value[0],
);
const down = computed(() => bytes(props.traffic.down, 2)),
  up = computed(() => bytes(props.traffic.up, 2));
const modeNames: Record<Mode, string> = {
  rule: "规则模式",
  global: "全局模式",
  direct: "直连模式",
};
</script>

<template>
  <div class="overview-page">
    <div class="summary-strip">
      <div class="summary-icon"><Radio :size="22" /></div>
      <div class="summary-copy">
        <span class="overline">当前网络路径</span>
        <h2>
          {{
            primary?.now ||
            (status.connected ? "暂无选中的节点" : "等待内核连接")
          }}
        </h2>
        <span
          >{{ primary?.name || "代理组" }}
          <span class="tiny-separator">/</span> {{ modeNames[mode] }}</span
        >
      </div>
      <button
        class="button button-secondary"
        @click="emit('navigate', 'proxies')"
      >
        管理节点<ArrowRight :size="16" />
      </button>
    </div>
    <div class="metrics-grid">
      <section class="metric">
        <div class="metric-label">
          <span class="metric-icon metric-down"
            ><ArrowDownLeft :size="18" /></span
          >下载速度
        </div>
        <div class="metric-number">
          {{ down.value }}<span>{{ down.unit }}/s</span>
        </div>
        <div class="metric-caption">
          <span class="mini-dot green" />{{
            streamState === "online" ? "实时更新中" : "等待实时数据"
          }}
        </div>
      </section>
      <section class="metric">
        <div class="metric-label">
          <span class="metric-icon metric-up"><ArrowUpRight :size="18" /></span
          >上传速度
        </div>
        <div class="metric-number">
          {{ up.value }}<span>{{ up.unit }}/s</span>
        </div>
        <div class="metric-caption">轻松掌握每一次传输</div>
      </section>
      <section class="metric">
        <div class="metric-label">
          <span class="metric-icon metric-connections"
            ><Activity :size="18" /></span
          >活跃连接
        </div>
        <div class="metric-number">{{ connectionCount }}<span>个</span></div>
        <button class="metric-link" @click="emit('navigate', 'connections')">
          查看连接明细<ArrowRight :size="13" />
        </button>
      </section>
      <section class="metric">
        <div class="metric-label">
          <span class="metric-icon metric-status"
            ><ShieldCheck :size="18" /></span
          >内核状态
        </div>
        <div
          class="metric-number metric-state"
          :class="{ 'text-muted': !status.connected }"
        >
          {{ status.connected ? "运行正常" : "暂时离线" }}
        </div>
        <div class="metric-caption">
          {{ status.version || "检查内核连接配置" }}
        </div>
      </section>
    </div>
    <section class="surface traffic-surface">
      <div class="section-heading">
        <div>
          <h2>
            实时流量<span class="live-badge"
              ><span class="mini-dot green" />LIVE</span
            >
          </h2>
          <p>让每一次传输，都清晰可见。</p>
        </div>
        <div class="chart-legend">
          <span><i class="legend-down" />下载</span
          ><span><i class="legend-up" />上传</span
          ><span class="chart-range">最近 60 次采样</span>
        </div>
      </div>
      <TrafficChart :points="points" />
      <div class="traffic-footer">
        <span
          >累计下载<strong>{{
            formatBytes(traffic.downTotal || 0)
          }}</strong></span
        ><span
          >累计上传<strong>{{
            formatBytes(traffic.upTotal || 0)
          }}</strong></span
        ><span class="traffic-footer-note"
          ><Clock3 :size="13" />内核本次运行</span
        >
      </div>
    </section>
    <div class="overview-bottom">
      <section class="surface group-summary">
        <div class="section-heading">
          <div>
            <h2>常用代理组</h2>
            <p>你的连接，各就其位。</p>
          </div>
          <button class="text-button" @click="emit('navigate', 'proxies')">
            查看全部<ArrowRight :size="14" />
          </button>
        </div>
        <div v-if="!groups.length" class="small-empty">
          <Globe2 :size="28" /><span>{{
            status.connected
              ? "内核没有可用的代理组"
              : "内核连接后将在这里显示代理组"
          }}</span>
        </div>
        <button
          v-for="group in groups.slice(0, 3)"
          :key="group.name"
          class="group-summary-row"
          @click="emit('navigate', 'proxies')"
        >
          <span
            class="region-badge"
            :class="`region-${nodeRegion(group.now || '').code.toLowerCase()}`"
            >{{ nodeRegion(group.now || "").code }}</span
          ><span class="group-summary-copy"
            ><strong>{{ group.name }}</strong
            ><span>{{
              group.now || `${group.all.length} 个节点 · ${group.type}`
            }}</span></span
          ><span
            class="latency"
            :class="delayClass(lastDelay(proxies[group.now || '']?.history))"
            >{{ lastDelay(proxies[group.now || ""]?.history) || "—"
            }}<small v-if="lastDelay(proxies[group.now || '']?.history)"
              >ms</small
            ></span
          ><ArrowRight :size="16" class="text-muted" />
        </button>
      </section>
      <section class="surface mode-surface">
        <div class="section-heading">
          <div>
            <h2>代理模式</h2>
            <p>为当前网络选择合适的方式。</p>
          </div>
          <SlidersIcon />
        </div>
        <div class="mode-options">
          <button
            v-for="choice in ['rule', 'global', 'direct'] as const"
            :key="choice"
            :class="{ selected: mode === choice }"
            :disabled="modeBusy || !status.connected"
            @click="emit('mode', choice)"
          >
            <CheckCircle2 v-if="mode === choice" :size="16" /><span>{{
              modeNames[choice]
            }}</span
            ><small>{{
              choice === "rule"
                ? "按规则智能分流"
                : choice === "global"
                  ? "全部通过代理"
                  : "全部直接连接"
            }}</small>
          </button>
        </div>
      </section>
    </div>
    <section class="recent-activity">
      <span class="overline">最近动态</span>
      <div v-if="!activities.length">
        <span class="mini-dot green" />{{
          status.connected
            ? "管理服务已就绪，网络由你掌握。"
            : "管理服务已就绪，等待内核恢复连接。"
        }}
      </div>
      <div
        v-for="activity in activities.slice(0, 3)"
        :key="activity.time + activity.message"
      >
        <span class="mini-dot green" /><span>{{ activity.message }}</span
        ><time>{{ activity.time }}</time>
      </div>
    </section>
  </div>
</template>

<script lang="ts">
import { defineComponent, h } from "vue";
import { SlidersHorizontal } from "@lucide/vue";
const SlidersIcon = defineComponent({
  render: () => h(SlidersHorizontal, { size: 18, class: "text-muted" }),
});
</script>
