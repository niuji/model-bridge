<template>
  <div>
    <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 16px">
      <h2>供应商</h2>
      <n-button type="primary" @click="showCreate = true">添加供应商</n-button>
    </div>

    <n-data-table :columns="columns" :data="providers" :bordered="false" />

    <!-- 创建/编辑弹窗 -->
    <n-modal v-model:show="showCreate" title="添加供应商">
      <n-card style="width: 560px" :bordered="false">
        <n-form :model="form" label-placement="left" label-width="160px">
          <n-form-item label="名称">
            <n-input v-model:value="form.name" placeholder="例如：OpenAI、DeepSeek" />
          </n-form-item>
          <n-form-item label="OpenAI 基础 URL">
            <n-input v-model:value="form.openai_base_url" placeholder="https://api.openai.com" />
          </n-form-item>
          <n-form-item label="Anthropic 基础 URL">
            <n-input v-model:value="form.anthropic_base_url" placeholder="https://api.anthropic.com" />
          </n-form-item>
          <n-form-item label="API 密钥">
            <n-input v-model:value="form.api_key" type="password" placeholder="sk-..." />
          </n-form-item>
        </n-form>
        <template #footer>
          <n-space justify="end">
            <n-button @click="showCreate = false">取消</n-button>
            <n-button type="primary" @click="handleCreate" :loading="submitting">保存</n-button>
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
  { title: '名称', key: 'name', width: 160 },
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
    title: '启用',
    key: 'is_enabled',
    width: 80,
    render: (row: Provider) =>
      h(NSwitch, { value: row.is_enabled, onUpdateValue: (v: boolean) => handleToggle(row, v) }),
  },
  {
    title: '操作',
    key: 'actions',
    width: 80,
    render: (row: Provider) =>
      h(NButton, { size: 'small', type: 'error', onClick: () => handleDelete(row.id) }, { default: () => '删除' }),
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
      message.success('供应商创建成功')
      showCreate.value = false
      form.value = { name: '', openai_base_url: '', anthropic_base_url: '', api_key: '' }
      loadProviders()
    } else {
      const err = await res.json()
      message.error(err.error || '创建供应商失败')
    }
  } finally {
    submitting.value = false
  }
}

async function handleToggle(provider: Provider, enabled: boolean) {
  try {
    const body: any = {
      name: provider.name,
      openai_base_url: provider.openai_base_url,
      anthropic_base_url: provider.anthropic_base_url,
      is_enabled: enabled,
    }
    await fetch(`${API_BASE}/providers/${provider.id}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })
    loadProviders()
  } catch (e) {
    message.error('更新供应商失败')
  }
}

async function handleDelete(id: string) {
  await fetch(`${API_BASE}/providers/${id}`, { method: 'DELETE' })
  loadProviders()
  message.success('供应商已删除')
}

onMounted(loadProviders)
</script>
