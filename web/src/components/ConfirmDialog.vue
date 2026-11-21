<script setup lang="ts">
import { nextTick, onBeforeUnmount, ref, watch } from "vue";
import { X, AlertTriangle } from "@lucide/vue";
const props = defineProps<{
  open: boolean;
  title: string;
  description: string;
  label?: string;
  busy?: boolean;
}>();
const emit = defineEmits<{ close: []; confirm: [] }>();
const dialog = ref<HTMLElement>();
let previousFocus: HTMLElement | null = null;
function keydown(event: KeyboardEvent) {
  if (!props.open) return;
  if (event.key === "Escape" && !props.busy) emit("close");
  if (event.key === "Tab") {
    const items = dialog.value?.querySelectorAll<HTMLElement>(
      "button:not(:disabled)",
    );
    if (!items?.length) return;
    const first = items[0],
      last = items[items.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  }
}
watch(
  () => props.open,
  async (open) => {
    if (open) {
      previousFocus = document.activeElement as HTMLElement;
      document.addEventListener("keydown", keydown);
      await nextTick();
      dialog.value?.focus();
    } else {
      document.removeEventListener("keydown", keydown);
      previousFocus?.focus();
    }
  },
);
onBeforeUnmount(() => document.removeEventListener("keydown", keydown));
</script>

<template>
  <Teleport to="body"
    ><Transition name="dialog"
      ><div
        v-if="open"
        class="modal-backdrop"
        @click.self="!busy && emit('close')"
      >
        <section
          ref="dialog"
          class="modal"
          role="dialog"
          aria-modal="true"
          aria-labelledby="confirm-title"
          tabindex="-1"
        >
          <button
            class="icon-button modal-close"
            aria-label="关闭确认窗口"
            :disabled="busy"
            @click="emit('close')"
          >
            <X :size="18" />
          </button>
          <div class="modal-warning"><AlertTriangle :size="24" /></div>
          <h2 id="confirm-title">{{ title }}</h2>
          <p>{{ description }}</p>
          <div class="modal-actions">
            <button
              class="button button-secondary"
              :disabled="busy"
              @click="emit('close')"
            >
              取消</button
            ><button
              class="button button-danger"
              :disabled="busy"
              @click="emit('confirm')"
            >
              <span v-if="busy" class="spinner" />{{ label || "确认操作" }}
            </button>
          </div>
        </section>
      </div></Transition
    ></Teleport
  >
</template>
