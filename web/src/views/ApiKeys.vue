<template>
  <div>
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 16px">
      <h2>API 密钥</h2>
      <n-button type="primary" @click="showCreate = true">生成密钥</n-button>
    </div>

    <n-data-table :columns="columns" :data="keys" :bordered="false" />

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

    <!-- 显示 key 的弹窗 -->
    <n-modal v-model:show="showKey" title="API 密钥已生成" :mask-closable="false">
      <n-card style="width: 500px" :bordered="false">
        <n-alert type="warning" title="请立即复制此密钥，关闭后将不再显示。" style="margin-bottom: 12px" />
        <n-input v-model:value="newKey" readonly />
      </n-card>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, h } from 'vue'
import { NDataTable, NButton, NModal, NCard, NForm, NFormItem, NInput, NSpace, NAlert, useMessage } from 'naive-ui'

const message = useMessage()
const API_BASE = '/api/admin'

const keys = ref<any[]>([])
const showCreate = ref(false)
const showKey = ref(false)
const newKey = ref('')
const form = ref({ name: '' })

const columns = [
  { title: '名称', key: 'name', width: 200 },
  { title: '创建时间', key: 'created_at', width: 200 },
  { title: '启用', key: 'is_enabled', width: 80, render: (row: any) => row.is_enabled ? '✅' : '❌' },
  {
    title: '操作',
    key: 'actions',
    width: 160,
    render: (row: any) =>
      h(NSpace, {}, [
        h(NButton, { size: 'small', onClick: () => handleCopy(row.id) }, { default: () => '复制' }),
        h(NButton, { size: 'small', type: 'error', onClick: () => handleDelete(row.id) }, { default: () => '删除' }),
      ]),
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
  const res = await fetch(`${API_BASE}/api-keys`)
  keys.value = await res.json()
}

async function handleCreate() {
  const res = await fetch(`${API_BASE}/api-keys`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name: form.value.name }),
  })
  if (res.ok) {
    const data = await res.json()
    newKey.value = data.key
    showCreate.value = false
    showKey.value = true
    form.value.name = ''
    loadKeys()
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

onMounted(loadKeys)
</script>
