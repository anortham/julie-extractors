<template>
  <section class="worker">
    <HeaderBar />
    <h1>{{ title }}</h1>
  </section>
</template>

<script setup lang="ts">
const title = format("Worker");

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
</script>

<style scoped>
.worker {
  color: #0f766e;
}
</style>
