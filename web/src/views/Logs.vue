<template>
  <div class="logs">
    <div class="page-header">
      <div><h1 class="page-title serif">日志</h1><p class="page-subtitle mono">请求记录</p></div>
      <span class="record-count mono">{{ total }} 条记录</span>
    </div>
    <div class="table-container">
      <n-data-table :columns="columns" :data="logs" :bordered="false" :loading="loading" size="small" class="logs-table" />
    </div>
    <div class="pagination-bar"><n-pagination v-model:page="page" :page-size="pageSize" :item-count="total" @update:page="loadLogs" /></div>
    <div v-if="!loading && logs.length === 0" class="empty-state"><p class="empty-text">暂无请求记录</p><p class="empty-hint mono">当有请求通过桥接代理时，日志将自动记录</p></div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, h } from 'vue'
import { NDataTable, NPagination } from 'naive-ui'
import { formatLocalTime } from '../utils'
const API_BASE = '/api/admin'
const logs = ref<any[]>([]); const loading = ref(true); const page = ref(1); const pageSize = 50; const total = ref(0)
const columns = [
  { title: '时间', key: 'created_at', width: 180, render: (row: any) => h('span', { class: 'mono time-cell' }, formatLocalTime(row.created_at) || '—') },
  { title: 'API Key', key: 'api_key', width: 170, ellipsis: { tooltip: true }, render: (row: any) => {
      if (row.api_key_name) return h('span', { class: 'mono apikey-cell' }, row.api_key_name)
      if (row.api_key_preview) return h('span', { class: 'mono apikey-cell' }, row.api_key_preview)
      if (row.api_key_id) return h('span', { class: 'apikey-deleted' }, '已删除')
      return h('span', { class: 'mono apikey-cell' }, '—')
    } },
  { title: '模型', key: 'model_id', width: 220, ellipsis: { tooltip: true }, render: (row: any) => h('span', { class: 'mono model-cell' }, row.model_id || '—') },
  { title: '供应商', key: 'provider_id', width: 180, render: (row: any) => h('span', { class: 'mono provider-cell' }, row.provider_id || '—') },
  { title: 'Token（输入/输出/缓存）', key: 'tokens', width: 190, align: 'right', render: (row: any) => {
      const cacheRead = row.cache_read_tokens ?? 0
      const cacheWrite = row.cache_write_tokens ?? 0
      const hasCache = cacheRead > 0 || cacheWrite > 0
      return h('span', { class: 'mono token-info' }, [
        h('div', { class: 'token-main' }, [
          h('span', { class: 'token-in' }, row.input_tokens ?? '—'),
          h('span', { class: 'token-sep' }, ' / '),
          h('span', { class: 'token-out' }, row.output_tokens ?? '—'),
        ]),
        hasCache ? h('div', { class: 'token-cache' }, [
          h('span', { class: 'cache-label' }, '缓存'),
          h('span', { class: 'cache-read' }, `读 ${cacheRead}`),
          h('span', { class: 'cache-sep' }, '·'),
          h('span', { class: 'cache-write' }, `写 ${cacheWrite}`),
        ]) : null,
      ])
    } },
  { title: '延迟', key: 'latency_ms', width: 110, align: 'right', render: (row: any) => h('span', { class: 'mono latency-cell' }, row.latency_ms != null ? `${row.latency_ms}ms` : '—') },
  { title: '状态', key: 'status', width: 90, align: 'center', render: (row: any) => { const ok = row.status === 'success'; return h('span', { class: `status-badge ${ok ? 'success' : 'error'}` }, [h('span', { class: 'status-dot-sm' }), h('span', { class: 'mono status-text' }, ok ? 'OK' : 'ERR')]) } },
  { title: '错误', key: 'error_msg', ellipsis: { tooltip: true }, render: (row: any) => h('span', { class: 'mono error-cell' }, row.error_msg || '—') },
]
async function loadLogs() { loading.value = true; try { const res = await fetch(`${API_BASE}/logs?page=${page.value}&page_size=${pageSize}`); const data = await res.json(); logs.value = data.logs; total.value = data.total } finally { loading.value = false } }
onMounted(loadLogs)
</script>

<style scoped>
.page-header { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 24px; }
.page-title { font-size: 28px; font-weight: 600; color: #17140f; margin: 0; letter-spacing: -0.02em; }
.page-subtitle { margin: 6px 0 0; color: #a89e8c; font-size: 13px; }
.record-count { font-size: 12px; color: #a89e8c; padding: 5px 12px; background: #ffffff; border: 1px solid #d9cfbf; border-radius: 999px; }

.table-container { background: #ffffff; border: 1px solid #d9cfbf; border-radius: 12px; overflow: hidden; }
.logs-table { --n-td-color: #ffffff; --n-th-color: #f4efe3; }

.time-cell { font-size: 12px; color: #74695a; }
.model-cell { font-size: 12px; color: #74695a; }
.provider-cell { font-size: 12px; color: #0d6e6b; }
.apikey-cell { font-size: 12px; color: #74695a; }
.apikey-deleted { font-size: 12px; color: #a89e8c; }
.token-info { font-size: 12px; display: inline-flex; flex-direction: column; align-items: flex-end; gap: 2px; }
.token-main { line-height: 1.4; }
.token-in { color: #9c6c00; }
.token-sep { color: #c9c0b0; }
.token-out { color: #0d6e6b; }
.token-cache { font-size: 11px; line-height: 1.3; white-space: nowrap; }
.cache-label { color: #a89e8c; margin-right: 5px; }
.cache-read { color: #0d6e6b; }
.cache-sep { color: #c9c0b0; margin: 0 5px; }
.cache-write { color: #b5842b; }
.latency-cell { font-size: 12px; color: #b5842b; }

.status-badge { display: inline-flex; align-items: center; gap: 5px; padding: 3px 10px; border-radius: 999px; }
.status-badge.success { background: rgba(46, 168, 106, 0.08); border: 1px solid rgba(46, 168, 106, 0.2); }
.status-badge.error { background: rgba(179, 38, 30, 0.08); border: 1px solid rgba(179, 38, 30, 0.2); }
.status-dot-sm { width: 5px; height: 5px; border-radius: 50%; }
.status-badge.success .status-dot-sm { background: #2ea86a; }
.status-badge.error .status-dot-sm { background: #b3261e; }
.status-text { font-size: 10px; font-weight: 600; letter-spacing: 0.05em; }
.status-badge.success .status-text { color: #1d7a4c; }
.status-badge.error .status-text { color: #b3261e; }
.error-cell { font-size: 12px; color: #b3261e; }

.pagination-bar { display: flex; justify-content: flex-end; margin-top: 16px; }
.empty-state { display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 60px 20px; text-align: center; }
.empty-text { margin: 0; font-size: 16px; font-weight: 500; color: #74695a; }
.empty-hint { margin: 6px 0 0; font-size: 13px; color: #a89e8c; }
</style>