<template>
  <div>
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 16px">
      <h2>Providers</h2>
      <n-button type="primary" @click="showCreate = true">Add Provider</n-button>
    </div>

    <n-data-table :columns="columns" :data="providers" :bordered="false" />

    <!-- 创建/编辑弹窗 -->
    <n-modal v-model:show="showCreate" title="Add Provider">
      <n-card style="width: 560px" :bordered="false">
        <n-form :model="form" label-placement="left" label-width="160px">
          <n-form-item label="Name">
            <n-input v-model:value="form.name" placeholder="e.g. OpenAI, DeepSeek" />
          </n-form-item>
          <n-form-item label="OpenAI Base URL">
            <n-input v-model:value="form.openai_base_url" placeholder="https://api.openai.com" />
          </n-form-item>
          <n-form-item label="Anthropic Base URL">
            <n-input v-model:value="form.anthropic_base_url" placeholder="https://api.anthropic.com" />
          </n-form-item>
          <n-form-item label="API Key">
            <n-input v-model:value="form.api_key" type="password" placeholder="sk-..." />
          </n-form-item>
        </n-form>
        <template #footer>
          <n-space justify="end">
            <n-button @click="showCreate = false">Cancel</n-button>
            <n-button type="primary" @click="handleCreate" :loading="submitting">Save</n-button>
          </n-space>
        </template>
      </n-card>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, h } from 'vue'
import {
  NDataTable, NButton, NModal, NCard, NForm, NFormItem, NInput, NSpace, NSwitch, useMessage,
} from 'naive-ui'

const message = useMessage()
const API_BASE = '/api/admin'

interface Provider {
  id: string
  name: string
  openai_base_url: string | null
  anthropic_base_url: string | null
  is_enabled: boolean
  created_at: string
}

const providers = ref<Provider[]>([])
const showCreate = ref(false)
const submitting = ref(false)
const form = ref({ name: '', openai_base_url: '', anthropic_base_url: '', api_key: '' })

const columns = [
  { title: 'Name', key: 'name', width: 160 },
  {
    title: 'OpenAI URL',
    key: 'openai_base_url',
    ellipsis: { tooltip: true },
    render: (row: Provider) => row.openai_base_url || '-',
  },
  {
    title: 'Anthropic URL',
    key: 'anthropic_base_url',
    ellipsis: { tooltip: true },
    render: (row: Provider) => row.anthropic_base_url || '-',
  },
  {
    title: 'Enabled',
    key: 'is_enabled',
    width: 90,
    render: (row: Provider) => row.is_enabled ? '✅' : '❌',
  },
  {
    title: 'Actions',
    key: 'actions',
    width: 150,
    render: (row: Provider) =>
      h(NSpace, {}, [
        h(NButton, { size: 'small', onClick: () => handleToggle(row) }, { default: () => row.is_enabled ? 'Disable' : 'Enable' }),
        h(NButton, { size: 'small', type: 'error', onClick: () => handleDelete(row.id) }, { default: () => 'Delete' }),
      ]),
  },
]

async function loadProviders() {
  const res = await fetch(`${API_BASE}/providers`)
  providers.value = await res.json()
}

async function handleCreate() {
  submitting.value = true
  try {
    const body: any = { name: form.value.name, api_key: form.value.api_key }
    if (form.value.openai_base_url) body.openai_base_url = form.value.openai_base_url
    if (form.value.anthropic_base_url) body.anthropic_base_url = form.value.anthropic_base_url
    const res = await fetch(`${API_BASE}/providers`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    if (res.ok) {
      message.success('Provider created')
      showCreate.value = false
      form.value = { name: '', openai_base_url: '', anthropic_base_url: '', api_key: '' }
      loadProviders()
    } else {
      const err = await res.json()
      message.error(err.error || 'Failed to create provider')
    }
  } finally {
    submitting.value = false
  }
}

async function handleToggle(provider: Provider) {
  try {
    const body: any = {
      name: provider.name,
      openai_base_url: provider.openai_base_url,
      anthropic_base_url: provider.anthropic_base_url,
      is_enabled: !provider.is_enabled,
    }
    await fetch(`${API_BASE}/providers/${provider.id}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    loadProviders()
  } catch (e) {
    message.error('Failed to update provider')
  }
}

async function handleDelete(id: string) {
  await fetch(`${API_BASE}/providers/${id}`, { method: 'DELETE' })
  loadProviders()
  message.success('Provider deleted')
}

onMounted(loadProviders)
</script>
