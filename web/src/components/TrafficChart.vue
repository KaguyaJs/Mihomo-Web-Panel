<script setup lang="ts">
import { computed } from "vue";
import { formatBytes } from "../lib/format";
import type { TrafficPoint } from "../types";
const props = defineProps<{ points: TrafficPoint[] }>();
const width = 1000,
  height = 200;
const maximum = computed(
  () =>
    Math.max(
      1024,
      ...props.points.map((point) => Math.max(point.down, point.up)),
    ) * 1.2,
);
const series = (field: "up" | "down") =>
  computed(() => {
    const points = props.points;
    if (!points.length) return `M 0 ${height} L ${width} ${height}`;
    const offset = Math.max(0, 60 - points.length);
    return points
      .map(
        (point, index) =>
          `${index === 0 ? "M" : "L"} ${(((offset + index) / 59) * width).toFixed(1)} ${(height - (point[field] / maximum.value) * height).toFixed(1)}`,
      )
      .join(" ");
  });
const download = series("down"),
  upload = series("up");
const area = computed(
  () =>
    `${download.value} L ${width} ${height} L ${(Math.max(0, 60 - props.points.length) / 59) * width} ${height} Z`,
);
</script>

<template>
  <div
    class="chart-wrap"
    role="img"
    aria-label="最近 60 次采样的下载与上传速度曲线"
  >
    <div class="chart-scale">
      <span>{{ formatBytes(maximum) }}/s</span
      ><span>{{ formatBytes(maximum / 2) }}/s</span><span>0</span>
    </div>
    <svg
      class="traffic-chart"
      :viewBox="`0 0 ${width} ${height}`"
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <line
        v-for="y in [0, 50, 100, 150, 200]"
        :key="y"
        x1="0"
        :y1="y"
        :x2="width"
        :y2="y"
        class="chart-grid"
      />
      <path :d="area" class="chart-area" />
      <path :d="download" class="chart-download" />
      <path :d="upload" class="chart-upload" />
    </svg>
    <div class="chart-times">
      <span>60 次采样前</span><span>30 次采样前</span><span>现在</span>
    </div>
  </div>
</template>
