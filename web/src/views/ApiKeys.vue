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
        <template #footer><n-space justify="end"><n-button @click="showCreate = false">取消</n-button><n-button type="primary" :loading="creating" @click="handleCreate">生成</n-button></n-space></template>
      </n-card>
    </n-modal>
    <n-modal v-model:show="showRename" title="重命名 API 密钥" class="create-modal">
      <n-card style="width: 400px" :bordered="false" size="small">
        <n-form :model="renameForm" label-placement="left" label-width="60px"><n-form-item label="名称"><n-input v-model:value="renameForm.name" placeholder="为密钥命名以便识别" class="mono" /></n-form-item></n-form>
        <template #footer><n-space justify="end"><n-button @click="showRename = false">取消</n-button><n-button type="primary" :loading="renaming" @click="handleRename">保存</n-button></n-space></template>
      </n-card>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, h } from 'vue'
import { NDataTable, NButton, NModal, NCard, NForm, NFormItem, NInput, NSpace, NSwitch, NIcon, NPopconfirm, useMessage } from 'naive-ui'
import { formatLocalTime } from '../utils'
import { adminRequest, errorMessage } from '../api'
const message = useMessage()
const creating = ref(false)
const renaming = ref(false)
const pendingKeys = ref(new Set<string>())
const keys = ref<any[]>([]); const loading = ref(true); const showCreate = ref(false); const form = ref({ name: '' }); const showRename = ref(false); const renameTargetId = ref(''); const renameForm = ref({ name: '' })
const CopyIcon = () => h('svg', { xmlns: 'http://www.w3.org/2000/svg', width: '15', height: '15', viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '2', 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [h('rect', { x: '9', y: '9', width: '13', height: '13', rx: '2', ry: '2' }), h('path', { d: 'M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1' })])
const EditIcon = () => h('svg', { xmlns: 'http://www.w3.org/2000/svg', width: '15', height: '15', viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '2', 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [h('path', { d: 'M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z' })])
const columns = [
  { title: '名称', key: 'name', width: 200, render: (row: any) => h('div', { class: 'name-cell' }, [h('span', { class: 'key-name' }, row.name || '未命名'), h(NButton, { size: 'tiny', text: true, disabled: pendingKeys.value.has(row.id), onClick: () => openRename(row), class: 'rename-icon-btn' }, { default: () => h(NIcon, { size: 15 }, { default: EditIcon }) })]) },
  { title: '密钥', key: 'key_preview', ellipsis: { tooltip: true }, render: (row: any) => h('div', { class: 'key-preview-cell' }, [h('code', { class: 'key-preview mono' }, row.key_preview || '—'), h(NButton, { size: 'tiny', text: true, onClick: (e: Event) => { e.stopPropagation(); handleCopy(row.id) }, class: 'copy-btn' }, { default: () => h(NIcon, { size: 15 }, { default: CopyIcon }) })]) },
  { title: '创建时间', key: 'created_at', width: 180, render: (row: any) => h('span', { class: 'mono time-cell' }, formatLocalTime(row.created_at) || '—') },
  { title: '状态', key: 'is_enabled', width: 80, render: (row: any) => h(NSwitch, { value: row.is_enabled, disabled: pendingKeys.value.has(row.id), loading: pendingKeys.value.has(row.id), onUpdateValue: (v: boolean) => handleToggle(row.id, v), size: 'small' }) },
  { title: '操作', key: 'actions', width: 80, render: (row: any) => h(NPopconfirm, { onPositiveClick: () => handleDelete(row.id), positiveText: '确认删除', negativeText: '取消' }, { default: () => '确定删除该密钥？删除后不可恢复。', trigger: () => h(NButton, { size: 'small', type: 'error', text: true, class: 'delete-btn', disabled: pendingKeys.value.has(row.id) }, { default: () => '删除' }) }) },
]
async function handleCopy(id: string) {
  try {
    const data = await (await adminRequest(`/api-keys/${id}`)).json()
    await navigator.clipboard.writeText(data.key)
    message.success('密钥已复制到剪贴板')
  } catch (error) { message.error(errorMessage(error, '复制失败')) }
}

async function loadKeys() {
  loading.value = true
  try {
    keys.value = await (await adminRequest('/api-keys')).json()
  } catch (error) { message.error(errorMessage(error, '加载密钥失败')) }
  finally { loading.value = false }
}

async function handleCreate() {
  if (creating.value) return
  creating.value = true
  try {
    await adminRequest('/api-keys', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: form.value.name })
    })
    showCreate.value = false
    form.value.name = ''
    message.success('密钥已生成')
    await loadKeys()
  } catch (error) { message.error(errorMessage(error, '生成失败')) }
  finally { creating.value = false }
}

async function handleDelete(id: string) {
  if (pendingKeys.value.has(id)) return
  pendingKeys.value.add(id)
  try {
    await adminRequest(`/api-keys/${id}`, { method: 'DELETE' })
    message.success('密钥已删除')
    await loadKeys()
  } catch (error) { message.error(errorMessage(error, '删除失败')) }
  finally { pendingKeys.value.delete(id) }
}

function openRename(row: any) { renameTargetId.value = row.id; renameForm.value = { name: row.name || '' }; showRename.value = true }

async function handleRename() {
  const id = renameTargetId.value
  if (renaming.value || pendingKeys.value.has(id)) return
  renaming.value = true
  pendingKeys.value.add(id)
  try {
    await adminRequest(`/api-keys/${id}`, {
      method: 'PUT', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: renameForm.value.name })
    })
    showRename.value = false
    message.success('名称已更新')
    await loadKeys()
  } catch (error) { message.error(errorMessage(error, '重命名失败')) }
  finally { renaming.value = false; pendingKeys.value.delete(id) }
}

async function handleToggle(id: string, enabled: boolean) {
  if (pendingKeys.value.has(id)) return
  pendingKeys.value.add(id)
  try {
    await adminRequest(`/api-keys/${id}`, {
      method: 'PUT', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ is_enabled: enabled })
    })
    await loadKeys()
  } catch (error) { message.error(errorMessage(error, '更新失败')) }
  finally { pendingKeys.value.delete(id) }
}
onMounted(loadKeys)
</script>

<style scoped>
.page-header { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 24px; }
.page-title { font-size: 28px; font-weight: 600; color: var(--mb-text-1); margin: 0; letter-spacing: -0.02em; }
.page-subtitle { margin: 6px 0 0; color: var(--mb-text-3); font-size: 13px; }
.create-btn { font-weight: 600; }
.table-container { background: var(--mb-surface); border: 1px solid var(--mb-border); border-radius: 12px; overflow: hidden; }
.keys-table { --n-td-color: var(--mb-surface); --n-th-color: var(--mb-surface-inset); }
:deep(.key-preview-cell) { display: flex; align-items: center; gap: 8px; }
:deep(.key-preview) { font-size: 12px; color: var(--mb-text-2); background: var(--mb-surface-inset); padding: 3px 10px; border: 1px solid var(--mb-border); border-radius: 8px; }
:deep(.copy-btn) { opacity: 0.3; transition: opacity 0.15s; color: var(--mb-success-d); }
:deep(.copy-btn:hover) { opacity: 1; }
:deep(.time-cell) { font-size: 12px; color: var(--mb-text-2); }
:deep(.name-cell) { display: flex; align-items: center; gap: 8px; }
:deep(.name-cell .key-name) { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: 500; color: var(--mb-text-1); }
:deep(.rename-icon-btn) { flex-shrink: 0; opacity: 0.35; transition: opacity 0.15s; color: var(--mb-text-2); }
:deep(.rename-icon-btn:hover) { opacity: 1; }
:deep(.delete-btn) { opacity: 0.4; transition: opacity 0.15s; }
:deep(.delete-btn:hover) { opacity: 1; }
.empty-state { display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 60px 20px; text-align: center; }
.empty-text { margin: 0; font-size: 16px; font-weight: 500; color: var(--mb-text-2); }
.empty-hint { margin: 6px 0 0; font-size: 13px; color: var(--mb-text-3); }
.create-modal { --n-title-text-color: var(--mb-text-1); }
</style>