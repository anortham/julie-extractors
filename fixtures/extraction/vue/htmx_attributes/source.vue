<script setup lang="ts">
const markup = "<span hx-get='/ignored'></span>";
</script>

<template>
  <div>
    <button hx-post="/clicked" hx-target="#result">Click</button>
    <button data-hx-delete="/todos/1">Delete</button>
    <button :hx-get="endpoint">Load</button>
  </div>
</template>
