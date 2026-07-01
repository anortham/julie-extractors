<template>
  <nav>
    <NuxtLink to="/about">About</NuxtLink>
    <nuxt-link to="/contact">Contact</nuxt-link>
    <NuxtLink :to="{ name: 'posts-id', params: { id: post.id } }">Post</NuxtLink>
    <NuxtLink to="/download.pdf" external>Download</NuxtLink>
  </nav>
</template>

<script setup>
const post = { id: 123 }
</script>
