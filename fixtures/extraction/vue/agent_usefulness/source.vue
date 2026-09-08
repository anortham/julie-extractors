<script setup>
const attrs = { 'hx-post': '/save', 'hx-trigger': 'click', 'hx-get': `/workspaces/${id}/tests/start` };
const logging = { 'hx-get': '/not-consumed' };
</script>
<template><button v-bind="attrs">Save</button></template>
