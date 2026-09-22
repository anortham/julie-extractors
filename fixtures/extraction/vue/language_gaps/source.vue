<template>
  <div class="list">
    <template v-if="loading">
      <LoadingSpinner />
    </template>
    <ul v-else>
      <li v-for="item in items" :key="item.id" @click.stop="select(item)">{{ formatLabel(item) }}</li>
    </ul>
    <user-card :user="current" v-on:refresh="reload" />
    <BaseButton :disabled="!canSave" @click="save()">Save</BaseButton>
  </div>
</template>

<script lang="ts">
import { defineComponent } from 'vue'
/** Shape of a row. */
export interface Row { id: number; name: string }
export type Mode = 'view' | 'edit'
export enum Status { Active, Inactive }
/** Formats a row label. */
export function formatRow(row: Row): string {
  if (row.id > 0) { return row.name }
  return ''
}
export class RowStore { rows: Row[] = []; add(row: Row): void { this.rows.push(row) } }
export const MAX_ROWS = 50

export default defineComponent({
  name: 'GapPanel',
  props: { userId: Number },
  data() { return { first: '', last: '' } },
  computed: { fullName() { return `${this.first} ${this.last}` } },
  methods: {
    async load(id: number) {
      const user = await fetchUser(id)
      if (user.last) { this.last = user.last }
    },
    persist() { this.load(this.userId) },
  },
})
</script>

<script setup lang="ts">
import * as api from '@/api'
import { formatLabel } from '@/utils/format'
interface Props { pageSize?: number; title: string }
type Filter = 'all' | 'active'
enum Tab { List, Grid }
let timer: number | undefined
class Poller { start() { next() } }
const items: Row[] = []
const { rows: loadedRows, ...pager } = useRows()
const loading = false
const current = null
const canSave = true
function select(item: Row) { api.track(item.id) }
function reload() { select(items[0]) }
function save() { return formatRow(items[0]) }
</script>
