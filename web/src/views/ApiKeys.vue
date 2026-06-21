<template>
  <div>
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 16px">
      <h2>API Keys</h2>
      <n-button type="primary" @click="showCreate = true">Generate Key</n-button>
    </div>

    <n-data-table :columns="columns" :data="keys" :bordered="false" />

    <n-modal v-model:show="showCreate" title="Generate API Key">
      <n-card style="width: 400px" :bordered="false">
        <n-form :model="form" label-placement="left" label-width="80px">
          <n-form-item label="Name">
            <n-input v-model:value="form.name" placeholder="Key name" />
          </n-form-item>
        </n-form>
        <template #footer>
          <n-space justify="end">
            <n-button @click="showCreate = false">Cancel</n-button>
            <n-button type="primary" @click="handleCreate">Generate</n-button>
          </n-space>
        </template>
      </n-card>
    </n-modal>

    <!-- 显示 key 的弹窗 -->
    <n-modal v-model:show="showKey" title="API Key Generated" :mask-closable="false">
      <n-card style="width: 500px" :bordered="false">
        <n-alert type="warning" title="Copy this key now. It won't be shown again." style="margin-bottom: 12px" />
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
  { title: 'Name', key: 'name', width: 200 },
  { title: 'Created At', key: 'created_at', width: 200 },
  { title: 'Enabled', key: 'is_enabled', width: 100, render: (row: any) => row.is_enabled ? '✅' : '❌' },
  {
    title: 'Actions',
    key: 'actions',
    width: 100,
    render: (row: any) =>
      h(NButton, { size: 'small', type: 'error', onClick: () => handleDelete(row.id) }, { default: () => 'Delete' }),
  },
]

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
    message.error(err.error || 'Failed')
  }
}

async function handleDelete(id: string) {
  await fetch(`${API_BASE}/api-keys/${id}`, { method: 'DELETE' })
  loadKeys()
  message.success('Key deleted')
}

onMounted(loadKeys)
</script>
