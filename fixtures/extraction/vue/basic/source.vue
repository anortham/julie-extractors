<template>
  <section class="worker" v-if="title">
    <HeaderBar />
    <h1>{{ title }}</h1>
    <RouterLink to="/calendar">Calendar</RouterLink>
    <button @click.prevent="evaluate(1, true)" :class="{ active: title }">Run</button>
  </section>
</template>

<script setup lang="ts">
import CalendarView from "../views/CalendarView.vue";

defineOptions({ name: "Worker" });

const props = defineProps<{ title: string }>();
const emit = defineEmits<{ update: [] }>();

const title = format("Worker");
const workerIndex: Map<string, Array<number>> = new Map();
const routes = [{ path: "/calendar", name: "calendar", component: CalendarView }];

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
.worker {
  color: #0f766e;
}
</style>
