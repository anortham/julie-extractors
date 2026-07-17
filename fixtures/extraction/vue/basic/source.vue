<template>
  <section class="worker" v-if="title">
    <HeaderBar />
    <h1>{{ title }}</h1>
    <RouterLink to="/calendar">Calendar</RouterLink>
    <template #actions>
      <RouterLink to="/inside-slot">Inside</RouterLink>
    </template>
    <RouterLink to="/after-slot">After</RouterLink>
    <button @click.prevent="evaluate(1, true)" :class="{ active: title }">Run</button>
  </section>
</template>

<script setup lang="ts">
import CalendarView from "../views/CalendarView.vue";
import SettingsView from "../views/SettingsView.vue";

defineOptions({ name: "Worker" });

const props = defineProps<{ title: string }>();
const emit = defineEmits<{ update: [] }>();

const title = format("Worker");
const workerIndex: Map<string, Array<number>> = new Map();
const routes = [
    {
        meta: { requiresAuth: true },
        path: "/calendar",
        name: "calendar",
        component: CalendarView,
        children: [{ path: "settings", component: SettingsView }]
    }
];

function format(value: string): string {
    return value.trim();
}

function evaluate(count: number, enabled: boolean): number {
    let total = 0;
    if (enabled) {
        for (let i = 1; i <= count; i++) {
            total += i;
        }
    } else if (count > 0) {
        total = 1;
    }
    return total;
}

defineExpose({ format, evaluate });
</script>

<style scoped>
@charset "UTF-8";
@namespace url(http://www.w3.org/1999/xhtml);

:root {
  --accent: #0f766e;
}

.worker {
  color: #0f766e;
}

@media (min-width: 40rem) {
  .worker {
    display: block;
  }
}

@keyframes spin {
  from { opacity: 0; }
  to { opacity: 1; }
}

@supports (display: grid) {
  .worker { display: grid; }
}

@container (min-width: 20rem) {
  .worker { padding: 1rem; }
}

@font-face {
  font-family: "Worker";
  src: url("/worker.woff2");
}

@layer utilities {
  .m-0 { margin: 0; }
}
</style>
