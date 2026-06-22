<template>
  <div>
    <h2>日志</h2>
    <n-data-table :columns="columns" :data="logs" :bordered="false" :loading="loading" />
    <div style="margin-top: 16px">
      <n-pagination
        v-model:page="page"
        :page-size="pageSize"
        :item-count="total"
        @update:page="loadLogs"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { NDataTable, NPagination } from 'naive-ui'

const API_BASE = '/api/admin'

const logs = ref<any[]>([])
const loading = ref(true)
const page = ref(1)
const pageSize = 50
const total = ref(0)

const columns = [
  { title: '时间', key: 'created_at', width: 180 },
  { title: '模型', key: 'model_id', width: 180 },
  { title: '供应商', key: 'provider_id', width: 180 },
  {
    title: 'Token（输入/输出）',
    key: 'tokens',
    width: 140,
    render: (row: any) => `${row.input_tokens} / ${row.output_tokens}`,
  },
  { title: '延迟 (ms)', key: 'latency_ms', width: 120 },
  {
    title: '状态',
    key: 'status',
    width: 100,
    render: (row: any) => row.status === 'success' ? '✅' : '❌',
  },
  { title: '错误', key: 'error_msg', ellipsis: { tooltip: true } },
]

async function loadLogs() {
  loading.value = true
  try {
    // 从 stats endpoint 获取数据
    const res = await fetch(`${API_BASE}/stats/daily?days=30`)
    const data = await res.json()
    // 这里实际需要单独的使用记录查询 API
    // 目前 logs API 复用 stats 数据做简单展示
    logs.value = data
    total.value = data.length
  } finally {
    loading.value = false
  }
}

onMounted(loadLogs)
</script>
