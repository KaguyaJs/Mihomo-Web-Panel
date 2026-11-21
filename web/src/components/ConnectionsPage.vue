<script setup lang="ts">
import { computed, ref } from "vue";
import {
  Search,
  RefreshCw,
  X,
  Unplug,
  ArrowDownLeft,
  ArrowUpRight,
  Activity,
} from "@lucide/vue";
import ConfirmDialog from "./ConfirmDialog.vue";
import { api } from "../lib/api";
import { duration, formatBytes } from "../lib/format";
import type { Connections } from "../types";
const props = defineProps<{
  data: Connections;
  loading: boolean;
  connected: boolean;
}>();
const emit = defineEmits<{
  refresh: [];
  notice: [message: string, kind: "success" | "error"];
}>();
const search = ref(""),
  sort = ref("download"),
  pending = ref(new Set<string>()),
  confirming = ref(false),
  closingAll = ref(false);
const visible = computed(() =>
  props.data.connections
    .filter((connection) =>
      [
        connection.destination,
        connection.source,
        connection.chains.join(" "),
        connection.id,
        connection.process,
      ]
        .join(" ")
        .toLowerCase()
        .includes(search.value.toLowerCase()),
    )
    .sort((a, b) =>
      sort.value === "download"
        ? b.download - a.download
        : sort.value === "upload"
          ? b.upload - a.upload
          : Date.parse(b.start) - Date.parse(a.start),
    ),
);
async function close(id: string) {
  if (pending.value.has(id)) return;
  pending.value = new Set([...pending.value, id]);
  try {
    await api(`/api/connections/${encodeURIComponent(id)}`, {
      method: "DELETE",
    });
    emit("notice", "已关闭指定连接", "success");
    emit("refresh");
  } catch (cause) {
    emit(
      "notice",
      cause instanceof Error ? cause.message : "关闭连接失败",
      "error",
    );
  } finally {
    const next = new Set(pending.value);
    next.delete(id);
    pending.value = next;
  }
}
async function closeAll() {
  closingAll.value = true;
  try {
    await api("/api/connections", {
      method: "DELETE",
      body: { confirm: true },
    });
    confirming.value = false;
    emit("notice", "已关闭全部连接", "success");
    emit("refresh");
  } catch (cause) {
    emit(
      "notice",
      cause instanceof Error ? cause.message : "关闭连接失败",
      "error",
    );
  } finally {
    closingAll.value = false;
  }
}
</script>

<template>
  <div class="connections-page">
    <div class="page-toolbar">
      <label class="search-input"
        ><Search :size="18" /><input
          v-model="search"
          type="search"
          aria-label="搜索连接"
          placeholder="搜索目标、来源或代理链"
      /></label>
      <div class="toolbar-actions">
        <select v-model="sort" class="plain-select" aria-label="连接排序">
          <option value="download">下载流量排序</option>
          <option value="upload">上传流量排序</option>
          <option value="recent">最近连接</option></select
        ><button
          class="button button-secondary"
          :disabled="loading"
          @click="emit('refresh')"
        >
          <RefreshCw :size="16" :class="{ spinning: loading }" /></button
        ><button
          class="button button-danger-soft"
          :disabled="!connected || !data.connections.length"
          @click="confirming = true"
        >
          <Unplug :size="16" />关闭全部
        </button>
      </div>
    </div>
    <div class="list-summary">
      <span
        ><span class="mini-dot green" /><strong>{{
          data.connections.length
        }}</strong>
        个活跃连接</span
      ><span>每 3 秒自动刷新</span>
    </div>
    <section class="surface connections-surface">
      <div v-if="!visible.length" class="empty-state">
        <Activity :size="32" />
        <h3>{{ search ? "没有匹配的连接" : "当前没有活跃连接" }}</h3>
        <p>新的网络连接将在这里显示。</p>
      </div>
      <table v-else class="connections-table">
        <thead>
          <tr>
            <th>目标 / 来源</th>
            <th>代理链</th>
            <th>流量</th>
            <th>持续时间</th>
            <th><span class="sr-only">操作</span></th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="connection in visible.slice(0, 200)" :key="connection.id">
            <td>
              <div class="connection-target">
                <span class="network-badge">{{
                  connection.network.toUpperCase()
                }}</span
                ><strong>{{ connection.destination }}</strong>
              </div>
              <span class="connection-source"
                >{{ connection.source
                }}<span v-if="connection.process">
                  · {{ connection.process }}</span
                ></span
              >
              <details class="connection-details">
                <summary>连接明细</summary>
                <code>{{ connection.id }}</code
                ><span>规则：{{ connection.rule || "未记录" }}</span>
              </details>
            </td>
            <td>
              <span
                class="chain-pill"
                v-for="chain in connection.chains"
                :key="chain"
                >{{ chain }}</span
              >
            </td>
            <td>
              <div class="connection-flow">
                <ArrowDownLeft :size="13" />{{
                  formatBytes(connection.download)
                }}
              </div>
              <div class="connection-flow text-muted">
                <ArrowUpRight :size="13" />{{ formatBytes(connection.upload) }}
              </div>
            </td>
            <td class="connection-duration">
              {{ duration(connection.start) }}
            </td>
            <td>
              <button
                class="icon-button close-connection"
                :aria-label="`关闭连接 ${connection.id}`"
                :disabled="pending.has(connection.id)"
                @click="close(connection.id)"
              >
                <span v-if="pending.has(connection.id)" class="spinner" /><X
                  v-else
                  :size="17"
                />
              </button>
            </td>
          </tr>
        </tbody>
      </table>
      <div v-if="visible.length > 200" class="table-limit-note">
        显示前 200 个匹配连接。请使用搜索定位更多连接。
      </div>
    </section>
    <ConfirmDialog
      :open="confirming"
      title="关闭全部连接？"
      description="所有正在进行的连接会立即断开。下载、视频和其他传输可能受到影响，新的请求仍可重新建立连接。"
      label="关闭全部连接"
      :busy="closingAll"
      @close="confirming = false"
      @confirm="closeAll"
    />
  </div>
</template>
