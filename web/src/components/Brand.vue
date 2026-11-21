<script setup lang="ts">
defineProps<{ compact?: boolean }>();
</script>

<template>
  <div class="brand" aria-label="Mihomo Web Panel">
    <svg
      class="brand-symbol"
      viewBox="0 0 40 40"
      fill="none"
      aria-hidden="true"
    >
      <rect width="40" height="40" rx="12" fill="currentColor" />
      <path
        d="m10 27 7-14 7 14 7-14"
        stroke="white"
        stroke-width="2.7"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
      <circle cx="10" cy="27" r="1.9" fill="white" />
      <circle cx="31" cy="13" r="1.9" fill="white" />
    </svg>
    <div v-if="!compact" class="brand-wordmark">
      <strong>mihomo<span class="brand-dot">.</span></strong
      ><span>WEB PANEL</span>
    </div>
  </div>
</template>
