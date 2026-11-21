<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";
import {
  Search,
  RefreshCw,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Check,
  Zap,
  Radio,
  Info,
  X,
} from "@lucide/vue";
import { api } from "../lib/api";
import { lastDelay, delayClass, nodeRegion } from "../lib/format";
import type { DelayResult, ProxyNode } from "../types";
const props = defineProps<{
  proxies: Record<string, ProxyNode>;
  loading: boolean;
  connected: boolean;
}>();
const emit = defineEmits<{
  refresh: [];
  update: [name: string, node: ProxyNode];
  notice: [message: string, kind: "success" | "error"];
}>();
const search = ref(""),
  collapsed = ref(new Set<string>()),
  pages = ref<Record<string, number>>({}),
  pending = ref<string | null>(null);
const delays = ref<Record<string, DelayResult>>({}),
  batchBusy = ref(false),
  batchDone = ref(0),
  batchTotal = ref(0),
  timeoutMs = ref(5000),
  historyNode = ref<ProxyNode | null>(null);
let active = true;
onBeforeUnmount(() => {
  active = false;
});
const descriptions: Record<string, string> = {
  Selector: "手动选择",
  URLTest: "自动选择低延迟节点",
  Fallback: "自动故障转移",
  LoadBalance: "自动负载均衡",
  Relay: "按顺序中继",
  Direct: "直接连接",
  Reject: "拒绝连接",
};
const groups = computed(() =>
  Object.values(props.proxies)
    .filter((proxy) => proxy.all?.length)
    .sort(
      (a, b) => Number(b.type === "Selector") - Number(a.type === "Selector"),
    ),
);
const visibleGroups = computed(() =>
  groups.value.filter(
    (group) =>
      !search.value ||
      group.name.toLowerCase().includes(search.value.toLowerCase()) ||
      group.all.some((name) =>
        name.toLowerCase().includes(search.value.toLowerCase()),
      ),
  ),
);
const allNodeNames = computed(() =>
  [...new Set(groups.value.flatMap((group) => group.all))].filter(
    (name) => props.proxies[name]?.type !== "Reject",
  ),
);
const standalone = computed(() =>
  Object.values(props.proxies).filter(
    (proxy) =>
      !proxy.all?.length &&
      !groups.value.some((group) => group.all.includes(proxy.name)) &&
      (!search.value ||
        proxy.name.toLowerCase().includes(search.value.toLowerCase())),
  ),
);
function members(group: ProxyNode) {
  return group.all.filter(
    (name) =>
      !search.value ||
      group.name.toLowerCase().includes(search.value.toLowerCase()) ||
      name.toLowerCase().includes(search.value.toLowerCase()),
  );
}
function pageMembers(group: ProxyNode) {
  const page = Math.min(
    pages.value[group.name] || 0,
    Math.max(0, Math.ceil(members(group).length / 60) - 1),
  );
  return members(group).slice(page * 60, (page + 1) * 60);
}
function toggle(name: string) {
  const next = new Set(collapsed.value);
  if (next.has(name)) next.delete(name);
  else next.add(name);
  collapsed.value = next;
}
function latency(name: string) {
  return delays.value[name]?.delay ?? lastDelay(props.proxies[name]?.history);
}
function latencyLabel(name: string) {
  const result = delays.value[name];
  if (result?.status === "testing") return "测试中";
  if (result?.status === "timeout") return "超时";
  if (result?.status === "unavailable") return "不可用";
  const value = latency(name);
  return value === 0 ? "不可用" : value !== null ? `${value} ms` : "未测试";
}
async function select(group: ProxyNode, name: string) {
  if (group.type !== "Selector" || group.now === name || pending.value) return;
  pending.value = `${group.name}/${name}`;
  try {
    const updated = await api<ProxyNode>(
      `/api/proxies/${encodeURIComponent(group.name)}`,
      { method: "PUT", body: { name } },
    );
    emit("update", group.name, updated);
    emit("notice", `已切换到 ${updated.now || name}`, "success");
  } catch (cause) {
    emit(
      "notice",
      cause instanceof Error ? cause.message : "节点切换失败",
      "error",
    );
  } finally {
    pending.value = null;
  }
}
async function testNode(name: string) {
  if (delays.value[name]?.status === "testing" || batchBusy.value) return;
  delays.value[name] = { name, delay: null, status: "testing" };
  try {
    delays.value[name] = await api<DelayResult>(
      `/api/proxies/${encodeURIComponent(name)}/delay`,
      { method: "POST", body: { timeout_ms: timeoutMs.value } },
    );
  } catch (cause) {
    delays.value[name] = {
      name,
      delay: null,
      status:
        cause instanceof Error && cause.message.includes("超时")
          ? "timeout"
          : "unavailable",
    };
    emit(
      "notice",
      cause instanceof Error ? cause.message : "测速失败",
      "error",
    );
  }
}
async function batch(names: string[]) {
  if (batchBusy.value || !names.length) return;
  const unique = [...new Set(names)].filter(
    (name) => props.proxies[name]?.type !== "Reject",
  );
  batchBusy.value = true;
  batchDone.value = 0;
  batchTotal.value = unique.length;
  // Four requests per chunk share the server's global four-slot semaphore. Each chunk updates progress.
  for (let index = 0; index < unique.length; index += 4) {
    if (!active || document.hidden) {
      batchBusy.value = false;
      return;
    }
    const chunk = unique.slice(index, index + 4);
    for (const name of chunk)
      delays.value[name] = { name, delay: null, status: "testing" };
    try {
      const result = await api<{ results: DelayResult[] }>("/api/delay/batch", {
        method: "POST",
        body: { names: chunk, timeout_ms: timeoutMs.value },
      });
      for (const entry of result.results) delays.value[entry.name] = entry;
    } catch (cause) {
      for (const name of chunk)
        delays.value[name] = { name, delay: null, status: "unavailable" };
      emit(
        "notice",
        cause instanceof Error ? cause.message : "批量测速失败",
        "error",
      );
      batchBusy.value = false;
      return;
    }
    batchDone.value += chunk.length;
  }
  batchBusy.value = false;
  emit("notice", `测速完成 · ${batchDone.value} 个节点`, "success");
}
</script>

<template>
  <div class="proxies-page">
    <div class="page-toolbar">
      <label class="search-input"
        ><Search :size="18" /><input
          v-model="search"
          type="search"
          placeholder="搜索节点或代理组"
          aria-label="搜索节点或代理组"
          @input="pages = {}"
        /><kbd>/</kbd></label
      >
      <div class="toolbar-actions">
        <label class="compact-select"
          ><span>超时</span
          ><select v-model="timeoutMs" aria-label="测速超时">
            <option :value="3000">3 秒</option>
            <option :value="5000">5 秒</option>
            <option :value="10000">10 秒</option>
            <option :value="15000">15 秒</option>
          </select></label
        ><button
          class="button button-secondary"
          :disabled="loading"
          @click="emit('refresh')"
        >
          <RefreshCw :size="16" :class="{ spinning: loading }" /><span
            class="desktop-label"
            >刷新</span
          ></button
        ><button
          class="button button-primary"
          :disabled="batchBusy || !connected || !allNodeNames.length"
          @click="batch(allNodeNames)"
        >
          <span v-if="batchBusy" class="spinner" /><Zap v-else :size="16" />{{
            batchBusy ? `${batchDone} / ${batchTotal}` : "全部测速"
          }}
        </button>
      </div>
    </div>
    <div class="list-summary">
      <span
        ><strong>{{ groups.length }}</strong> 个代理组<span
          class="tiny-separator"
          >·</span
        ><strong>{{ allNodeNames.length }}</strong> 个节点</span
      ><span
        ><span class="mini-dot green" />低延迟<span
          class="mini-dot amber"
        />较高延迟</span
      >
    </div>
    <div v-if="loading && !groups.length" class="skeleton-list">
      <div v-for="item in 3" :key="item" class="skeleton" />
    </div>
    <div
      v-else-if="!visibleGroups.length && !standalone.length"
      class="empty-state"
    >
      <Search :size="32" />
      <h3>{{ search ? "没有找到匹配的节点" : "暂时没有代理节点" }}</h3>
      <p>
        {{
          search
            ? "试试其他关键词，或清空搜索。"
            : "请检查内核连接和 Mihomo 代理配置。"
        }}
      </p>
    </div>
    <section
      v-for="group in visibleGroups"
      :key="group.name"
      class="proxy-group surface"
      :class="{ collapsed: collapsed.has(group.name) }"
    >
      <header class="proxy-group-header">
        <button
          class="proxy-group-toggle"
          :aria-expanded="!collapsed.has(group.name)"
          @click="toggle(group.name)"
        >
          <span class="group-icon"><Radio :size="19" /></span
          ><span
            ><strong
              >{{ group.name
              }}<span class="type-badge">{{ group.type }}</span></strong
            ><small
              >{{ descriptions[group.type] || "自动管理"
              }}<span class="tiny-separator">·</span
              >{{ group.all.length }} 个节点<span
                v-if="group.now"
                class="current-node-hint"
                >当前：{{ group.now }}</span
              ></small
            ></span
          ></button
        ><button
          class="icon-button"
          :aria-label="`测试 ${group.name} 代理组`"
          title="代理组测速"
          :disabled="batchBusy || !connected"
          @click="batch(group.all)"
        >
          <Zap :size="17" /></button
        ><button
          class="icon-button group-collapse"
          :aria-label="
            collapsed.has(group.name)
              ? `展开 ${group.name}`
              : `收起 ${group.name}`
          "
          @click="toggle(group.name)"
        >
          <ChevronDown :size="18" />
        </button>
      </header>
      <div v-if="!collapsed.has(group.name)" class="proxy-group-body">
        <div v-if="group.type !== 'Selector'" class="group-readonly-note">
          <Info :size="14" />{{
            descriptions[group.type] || "此代理组由内核管理"
          }}，由 Mihomo 管理节点状态。
        </div>
        <div class="node-grid">
          <article
            v-for="name in pageMembers(group)"
            :key="name"
            class="node-row"
            :class="{
              'node-selected': group.now === name,
              'node-pending': pending === `${group.name}/${name}`,
            }"
          >
            <button
              class="node-select"
              :class="{ readonly: group.type !== 'Selector' }"
              :disabled="group.type !== 'Selector' || !!pending || !connected"
              :aria-label="`${group.type === 'Selector' ? '选择' : '查看'} ${name}`"
              :aria-pressed="group.now === name"
              @click="select(group, name)"
            >
              <span
                class="region-badge"
                :class="`region-${nodeRegion(name).code.toLowerCase()}`"
                >{{ nodeRegion(name).code }}</span
              ><span class="node-copy"
                ><strong>{{ name }}</strong
                ><span
                  >{{ props.proxies[name]?.type || "未知类型"
                  }}<span v-if="props.proxies[name]?.udp" class="node-protocol"
                    >UDP</span
                  ></span
                ></span
              ><span
                v-if="pending === `${group.name}/${name}`"
                class="spinner node-spinner"
              /><span v-else-if="group.now === name" class="selected-mark"
                ><Check :size="14"
              /></span>
            </button>
            <div class="node-actions">
              <button
                class="latency latency-button"
                :class="delayClass(latency(name))"
                :aria-label="`测试 ${name} 延迟`"
                :disabled="
                  batchBusy ||
                  delays[name]?.status === 'testing' ||
                  !connected ||
                  props.proxies[name]?.type === 'Reject'
                "
                @click="testNode(name)"
              >
                <span
                  v-if="delays[name]?.status === 'testing'"
                  class="spinner"
                />{{ latencyLabel(name) }}</button
              ><button
                class="icon-button node-history-button"
                :aria-label="`查看 ${name} 延迟历史`"
                @click="historyNode = props.proxies[name] || null"
              >
                <Info :size="14" />
              </button>
            </div>
          </article>
        </div>
        <div v-if="members(group).length > 60" class="node-pagination">
          <span
            >第 {{ (pages[group.name] || 0) + 1 }} 页 ·
            {{ members(group).length }} 个节点</span
          ><button
            class="icon-button"
            aria-label="上一页节点"
            :disabled="!pages[group.name]"
            @click="pages[group.name] = (pages[group.name] || 0) - 1"
          >
            <ChevronLeft :size="18" /></button
          ><button
            class="icon-button"
            aria-label="下一页节点"
            :disabled="
              ((pages[group.name] || 0) + 1) * 60 >= members(group).length
            "
            @click="pages[group.name] = (pages[group.name] || 0) + 1"
          >
            <ChevronRight :size="18" />
          </button>
        </div>
      </div>
    </section>
    <section v-if="standalone.length" class="surface standalone-nodes">
      <h2>独立节点</h2>
      <p>这些节点未被代理组引用。</p>
      <div class="node-grid">
        <div
          v-for="node in standalone.slice(0, 60)"
          :key="node.name"
          class="node-row"
        >
          <span class="region-badge">{{ nodeRegion(node.name).code }}</span
          ><span class="node-copy"
            ><strong>{{ node.name }}</strong
            ><span>{{ descriptions[node.type] || node.type }}</span></span
          ><button
            class="latency latency-button"
            :class="delayClass(latency(node.name))"
            :disabled="node.type === 'Reject' || batchBusy || !connected"
            @click="testNode(node.name)"
          >
            {{ latencyLabel(node.name) }}
          </button>
        </div>
      </div>
    </section>
    <Teleport to="body"
      ><Transition name="dialog"
        ><div
          v-if="historyNode"
          class="modal-backdrop"
          @click.self="historyNode = null"
        >
          <section
            class="modal history-modal"
            role="dialog"
            aria-modal="true"
            aria-label="节点延迟历史"
          >
            <button
              class="icon-button modal-close"
              aria-label="关闭延迟历史"
              @click="historyNode = null"
            >
              <X :size="18" />
            </button>
            <h2>{{ historyNode.name }}</h2>
            <p>最近的延迟记录 · {{ historyNode.type }}</p>
            <div v-if="!historyNode.history.length" class="small-empty">
              内核尚无延迟历史，请先执行测速。
            </div>
            <div
              v-for="(item, index) in historyNode.history.slice().reverse()"
              :key="index"
              class="history-row"
            >
              <time>{{
                item.time
                  ? new Date(item.time).toLocaleString("zh-CN")
                  : "时间未记录"
              }}</time
              ><span class="latency" :class="delayClass(item.delay)"
                >{{ item.delay || "超时"
                }}<small v-if="item.delay">ms</small></span
              >
            </div>
            <button
              class="button button-secondary full-width"
              @click="historyNode = null"
            >
              完成
            </button>
          </section>
        </div></Transition
      ></Teleport
    >
  </div>
</template>
