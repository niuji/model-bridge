<template>
  <div>
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 16px">
      <h2>API 密钥</h2>
      <n-button type="primary" @click="showCreate = true">生成密钥</n-button>
    </div>

    <n-data-table :columns="columns" :data="keys" :loading="loading" :bordered="false" />

    <n-modal v-model:show="showCreate" title="生成 API 密钥">
      <n-card style="width: 400px" :bordered="false">
        <n-form :model="form" label-placement="left" label-width="80px">
          <n-form-item label="名称">
            <n-input v-model:value="form.name" placeholder="密钥名称" />
          </n-form-item>
        </n-form>
        <template #footer>
          <n-space justify="end">
            <n-button @click="showCreate = false">取消</n-button>
            <n-button type="primary" @click="handleCreate">生成</n-button>
          </n-space>
        </template>
      </n-card>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, h } from 'vue'
import { NDataTable, NButton, NModal, NCard, NForm, NFormItem, NInput, NSpace, NIcon, NSwitch, useMessage } from 'naive-ui'

const message = useMessage()
const API_BASE = '/api/admin'

const keys = ref<any[]>([])
const loading = ref(true)
const showCreate = ref(false)
const form = ref({ name: '' })

// 复制图标 SVG
const CopyIcon = () =>
  h('svg', { xmlns: 'http://www.w3.org/2000/svg', width: '16', height: '16', viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '2', 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [
    h('rect', { x: '9', y: '9', width: '13', height: '13', rx: '2', ry: '2' }),
    h('path', { d: 'M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1' }),
  ])

const columns = [
  { title: '名称', key: 'name', width: 160 },
  {
    title: '密钥',
    key: 'key_preview',
    ellipsis: { tooltip: true },
    render: (row: any) =>
      h('div', { style: { display: 'flex', alignItems: 'center', gap: '8px' } }, [
        h('code', { style: { fontSize: '13px' } }, row.key_preview || '-'),
        h(NButton, { size: 'tiny', text: true, onClick: () => handleCopy(row.id) }, {
          default: () => h(NIcon, { size: 16 }, { default: CopyIcon }),
        }),
      ]),
  },
  { title: '创建时间', key: 'created_at', width: 180 },
  { title: '启用', key: 'is_enabled', width: 80, render: (row: any) =>
    h(NSwitch, { value: row.is_enabled, onUpdateValue: (v: boolean) => handleToggle(row.id, v) }),
  },
  {
    title: '操作',
    key: 'actions',
    width: 80,
    render: (row: any) =>
      h(NButton, { size: 'small', type: 'error', onClick: () => handleDelete(row.id) }, { default: () => '删除' }),
  },
]

async function handleCopy(id: string) {
  try {
    const res = await fetch(`${API_BASE}/api-keys/${id}`)
    if (res.ok) {
      const data = await res.json()
      await navigator.clipboard.writeText(data.key)
      message.success('密钥已复制到剪贴板')
    } else {
      message.error('获取密钥失败')
    }
  } catch {
    message.error('复制失败')
  }
}

async function loadKeys() {
  loading.value = true
  try {
    const res = await fetch(`${API_BASE}/api-keys`)
    keys.value = await res.json()
  } finally {
    loading.value = false
  }
}

async function handleCreate() {
  const res = await fetch(`${API_BASE}/api-keys`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name: form.value.name }),
  })
  if (res.ok) {
    showCreate.value = false
    form.value.name = ''
    loadKeys()
    message.success('密钥已生成')
  } else {
    const err = await res.json()
    message.error(err.error || '操作失败')
  }
}

async function handleDelete(id: string) {
  await fetch(`${API_BASE}/api-keys/${id}`, { method: 'DELETE' })
  loadKeys()
  message.success('密钥已删除')
}

async function handleToggle(id: string, enabled: boolean) {
  try {
    await fetch(`${API_BASE}/api-keys/${id}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ is_enabled: enabled }),
    })
    loadKeys()
  } catch {
    message.error('更新失败')
  }
}

onMounted(loadKeys)
</script>
