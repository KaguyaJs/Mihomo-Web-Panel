<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import {
  Search,
  Pause,
  Play,
  Trash2,
  Terminal,
  ArrowDownToLine,
} from "@lucide/vue";
import { createStream } from "../lib/stream";
import type { LogEntry } from "../types";
const entries = ref<(LogEntry & { id: number })[]>([]),
  query = ref(""),
  level = ref("all"),
  paused = ref(false),
  autoscroll = ref(true),
  state = ref("connecting"),
  viewport = ref<HTMLElement>();
let sequence = 0;
const visible = computed(() =>
  entries.value.filter(
    (entry) =>
      (level.value === "all" ||
        entry.type === level.value ||
        (level.value === "warning" && entry.type === "warn")) &&
      entry.payload.toLowerCase().includes(query.value.toLowerCase()),
  ),
);
const stream = createStream(
  "/api/logs",
  async (message) => {
    if (message.type !== "log" || !message.data) return;
    entries.value.push({ ...(message.data as LogEntry), id: sequence++ });
    if (entries.value.length > 500)
      entries.value.splice(0, entries.value.length - 500);
    if (autoscroll.value && !document.hidden) {
      await nextTick();
      if (viewport.value)
        viewport.value.scrollTop = viewport.value.scrollHeight;
    }
  },
  (value) => {
    state.value = value;
  },
);
function pause() {
  paused.value = !paused.value;
  if (paused.value) stream.stop();
  else stream.start();
}
function visibility() {
  if (document.hidden) stream.stop();
  else if (!paused.value) stream.start();
}
onMounted(() => {
  stream.start();
  document.addEventListener("visibilitychange", visibility);
});
onBeforeUnmount(() => {
  stream.stop();
  document.removeEventListener("visibilitychange", visibility);
});
</script>

<template>
  <div class="logs-page">
    <div class="page-toolbar">
      <label class="search-input"
        ><Search :size="18" /><input
          v-model="query"
          type="search"
          aria-label="搜索日志"
          placeholder="搜索日志关键词"
      /></label>
      <div class="toolbar-actions">
        <select v-model="level" class="plain-select" aria-label="日志等级">
          <option value="all">所有等级</option>
          <option value="debug">Debug</option>
          <option value="info">Info</option>
          <option value="warning">Warning</option>
          <option value="error">Error</option></select
        ><button class="button button-secondary" @click="pause">
          <Play v-if="paused" :size="16" /><Pause v-else :size="16" />{{
            paused ? "继续" : "暂停"
          }}</button
        ><button
          class="icon-button"
          aria-label="清空日志显示"
          @click="entries = []"
        >
          <Trash2 :size="17" />
        </button>
      </div>
    </div>
    <section class="surface log-surface">
      <header class="log-header">
        <span
          ><Terminal :size="17" />Mihomo 实时日志<span class="live-badge"
            ><span
              class="mini-dot"
              :class="state === 'online' ? 'green' : 'amber'"
            />{{
              paused
                ? "已暂停"
                : state === "online"
                  ? "LIVE"
                  : state === "connecting"
                    ? "连接中"
                    : "等待连接"
            }}</span
          ></span
        ><label class="autoscroll-toggle"
          ><input v-model="autoscroll" type="checkbox" /><ArrowDownToLine
            :size="14"
          />自动滚动</label
        >
      </header>
      <div
        ref="viewport"
        class="log-viewport"
        role="log"
        aria-label="实时内核日志"
        :aria-live="paused ? 'off' : 'polite'"
      >
        <div v-if="!visible.length" class="empty-state">
          <Terminal :size="30" />
          <h3>
            {{
              query
                ? "没有匹配的日志"
                : paused
                  ? "日志接收已暂停"
                  : "等待新的日志"
            }}
          </h3>
          <p>
            {{
              query
                ? "尝试调整关键词或日志等级。"
                : "内核日志将在这里实时显示。"
            }}
          </p>
        </div>
        <div v-for="entry in visible" :key="entry.id" class="log-entry">
          <time>{{
            new Date(entry.timestamp * 1000).toLocaleTimeString("zh-CN", {
              hour12: false,
            })
          }}</time
          ><span class="log-level" :class="`log-${entry.type}`">{{
            entry.type.toUpperCase()
          }}</span
          ><span class="log-payload">{{ entry.payload }}</span>
        </div>
      </div>
      <footer class="log-footer">
        <span>保留最近 500 条 · 当前 {{ entries.length }} 条</span
        ><span>{{ visible.length }} 条匹配</span>
      </footer>
    </section>
  </div>
</template>
