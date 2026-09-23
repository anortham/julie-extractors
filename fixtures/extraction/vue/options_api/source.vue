<!-- Upload form that extends BaseForm and mixes in form helpers. -->
<template>
  <form @submit.prevent="upload(title)">
    <p v-if="open">{{ count }} {{ items.length }}</p>
    <button @click="reset">Reset</button>
  </form>
</template>

<script lang="ts">
import { defineComponent, ref } from 'vue'
import axios from 'axios'
import BaseForm from './BaseForm.vue'
import { formMixin } from './mixins/form'

export default defineComponent({
  name: 'UploadForm',
  extends: BaseForm,
  mixins: [formMixin],
  props: ['title', 'items'],
  data: () => ({ open: false }),
  setup(props, { emit }) {
    const count = ref(0)
    function reset(): void {
      count.value = 0
    }
    return { count, reset }
  },
  created() {
    this.init()
  },
  watch: {
    title(next) {
      this.open = Boolean(next)
    },
  },
  methods: {
    upload(payload) {
      return axios.request({ url: '/upload', data: payload, props: { retry: 1 } })
    },
  },
})
</script>
