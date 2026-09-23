<template>
  <dialog :open="visible">
    <user-card :name="name" @select="emit('select', count)" />
    <p>{{ modelValue }} {{ count }}</p>
    <button @click="goHome">Home</button>
  </dialog>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useRouter } from 'vue-router'
import UserCard from './UserCard.vue'

interface Props {
  /** Display name of the user. */
  name: string
  count?: number
}

const props = withDefaults(defineProps<Props>(), { count: 0 })
const emit = defineEmits<{ (e: 'select', id: number): void; (e: 'close'): void }>()
const visible = defineModel<boolean>('visible', { type: Boolean })
defineModel()

const router = useRouter()
const page = ref<number>(0)
const label = computed<string>(() => `${props.name} (${page.value})`)
const { data: posts } = await useFetch('/api/posts')
const me = await $fetch('/api/me')

// Navigate by path, not by route name.
function goHome() {
  router.push('/home')
}

/** Leave the dialog and sign out. */
async function logout() {
  await navigateTo('/login')
}
</script>

<style lang="scss" scoped>
$radius: 4px;
.card {
  border-radius: $radius;
  &__title { font-weight: 600; }
}
</style>
