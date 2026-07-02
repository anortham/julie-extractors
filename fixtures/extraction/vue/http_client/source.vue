<template>
  <button @click="load">load users</button>
</template>

<script setup lang="ts">
import axios from "axios";

async function load() {
  await fetch("/api/session");
  await axios.get("/api/users");
  await axios("/api/users", { method: "post" });
}
</script>
