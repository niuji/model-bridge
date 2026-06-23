<template>
  <div class="api-keys">
    <div class="page-header">
      <div><h1 class="page-title serif">API 密钥</h1><p class="page-subtitle mono">管理访问令牌 · 密钥仅在创建时完整显示</p></div>
      <n-button type="primary" @click="showCreate = true" class="create-btn">
        <template #icon><svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" /></svg></template>生成密钥
      </n-button>
    </div>
    <div class="table-container">
      <n-data-table :columns="columns" :data="keys" :loading="loading" :bordered="false" size="small" class="keys-table" />
    </div>
    <div v-if="!loading && keys.length === 0" class="empty-state"><p class="empty-text">暂无 API 密钥</p><p class="empty-hint mono">点击「生成密钥」创建第一个访问令牌</p></div>
    <n-modal v-model:show="showCreate" title="生成 API 密钥" class="create-modal">
      <n-card style="width: 400px" :bordered="false" size="small">
        <n-form :model="form" label-placement="left" label-width="60px"><n-form-item label="名称"><n-input v-model:value="form.name" placeholder="为密钥命名以便识别" class="mono" /></n-form-item></n-form>
        <template #footer><n-space justify="end"><n-button @click="showCreate = false">取消</n-button><n-button type="primary" @click="handleCreate">生成</n-button></n-space></template>
      </n-card>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, h } from 'vue'
import { NDataTable, NButton, NModal, NCard, NForm, NFormItem, NInput, NSpace, NSwitch, NIcon, useMessage } from 'naive-ui'
import { formatLocalTime } from '../utils'
const message = useMessage(); const API_BASE = '/api/admin'
const keys = ref<any[]>([]); const loading = ref(true); const showCreate = ref(false); const form = ref({ name: '' })
const CopyIcon = () => h('svg', { xmlns: 'http://www.w3.org/2000/svg', width: '15', height: '15', viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '2', 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [h('rect', { x: '9', y: '9', width: '13', height: '13', rx: '2', ry: '2' }), h('path', { d: 'M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1' })])
const columns = [
  { title: '名称', key: 'name', width: 180, render: (row: any) => h('span', { class: 'key-name' }, row.name || '未命名') },
  { title: '密钥', key: 'key_preview', ellipsis: { tooltip: true }, render: (row: any) => h('div', { class: 'key-preview-cell' }, [h('code', { class: 'key-preview mono' }, row.key_preview || '—'), h(NButton, { size: 'tiny', text: true, onClick: (e: Event) => { e.stopPropagation(); handleCopy(row.id) }, class: 'copy-btn' }, { default: () => h(NIcon, { size: 15 }, { default: CopyIcon }) })]) },
  { title: '创建时间', key: 'created_at', width: 180, render: (row: any) => h('span', { class: 'mono time-cell' }, formatLocalTime(row.created_at) || '—') },
  { title: '状态', key: 'is_enabled', width: 80, render: (row: any) => h(NSwitch, { value: row.is_enabled, onUpdateValue: (v: boolean) => handleToggle(row.id, v), size: 'small' }) },
  { title: '操作', key: 'actions', width: 80, render: (row: any) => h(NButton, { size: 'small', type: 'error', text: true, onClick: () => handleDelete(row.id), class: 'delete-btn' }, { default: () => '删除' }) },
]
async function handleCopy(id: string) { try { const res = await fetch(`${API_BASE}/api-keys/${id}`); if (res.ok) { const data = await res.json(); await navigator.clipboard.writeText(data.key); message.success('密钥已复制到剪贴板') } else { message.error('获取密钥失败') } } catch { message.error('复制失败') } }
async function loadKeys() { loading.value = true; try { const res = await fetch(`${API_BASE}/api-keys`); keys.value = await res.json() } finally { loading.value = false } }
async function handleCreate() { const res = await fetch(`${API_BASE}/api-keys`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name: form.value.name }) }); if (res.ok) { showCreate.value = false; form.value.name = ''; loadKeys(); message.success('密钥已生成') } else { const err = await res.json(); message.error(err.error || '操作失败') } }
async function handleDelete(id: string) { await fetch(`${API_BASE}/api-keys/${id}`, { method: 'DELETE' }); loadKeys(); message.success('密钥已删除') }
async function handleToggle(id: string, enabled: boolean) { try { await fetch(`${API_BASE}/api-keys/${id}`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ is_enabled: enabled }) }); loadKeys() } catch { message.error('更新失败') } }
onMounted(loadKeys)
</script>

<style scoped>
.page-header { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 24px; }
.page-title { font-size: 28px; font-weight: 600; color: #2d2d2a; margin: 0; letter-spacing: -0.02em; }
.page-subtitle { margin: 6px 0 0; color: #a0a098; font-size: 13px; }
.create-btn { font-weight: 600; }
.table-container { background: #ffffff; border: 1px solid #e0dcd5; border-radius: 16px; overflow: hidden; }
.keys-table { --n-td-color: #ffffff; --n-th-color: #faf8f4; }
.key-name { font-weight: 500; color: #2d2d2a; }
.key-preview-cell { display: flex; align-items: center; gap: 8px; }
.key-preview { font-size: 12px; color: #787870; background: #faf8f4; padding: 3px 10px; border: 1px solid #e0dcd5; border-radius: 8px; }
.copy-btn { opacity: 0.3; transition: opacity 0.15s; color: #2d6a4f; }
.copy-btn:hover { opacity: 1; }
.time-cell { font-size: 12px; color: #787870; }
.delete-btn { opacity: 0.4; transition: opacity 0.15s; }
.delete-btn:hover { opacity: 1; }
.empty-state { display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 60px 20px; text-align: center; }
.empty-text { margin: 0; font-size: 16px; font-weight: 500; color: #787870; }
.empty-hint { margin: 6px 0 0; font-size: 13px; color: #a0a098; }
.create-modal { --n-title-text-color: #2d2d2a; }
</style>