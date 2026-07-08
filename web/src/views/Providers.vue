<template>
  <div class="providers">
    <div class="page-header">
      <h1 class="page-title serif">供应商</h1>
      <p class="page-subtitle mono">管理 Provider 的 API 密钥、通道和模型列表</p>
    </div>

    <n-spin :show="loading">
      <div class="provider-grid">
        <div v-for="p in providers" :key="p.id" class="provider-card" :class="{ disabled: !p.is_enabled }" @click="openConfig(p)">
          <div class="card-top">
            <div class="card-name">
              <img v-if="p.icon" :src="iconUrl(p.icon)" class="card-icon" />
              <span class="card-name-text serif">{{ p.name }}</span>
            </div>
            <n-switch :value="p.is_enabled" @update:value="(v: boolean) => quickToggle(p, v)" size="small" @click.stop />
          </div>
          <div class="card-channels">
            <div v-for="ch in p.channels" :key="ch.channel_type" class="channel-tag" :class="{ off: !ch.is_enabled }">
              <span class="channel-label mono">{{ channelLabel(ch.channel_type) }}</span>
            </div>
          </div>
          <div class="card-footer">
            <span class="model-count mono">{{ p.model_count }} 个模型</span>
            <span class="card-arrow">→</span>
          </div>
        </div>
      </div>
    </n-spin>

    <n-modal
      v-model:show="showConfig"
      :title="editProvider?.name + ' 配置'"
      style="width: 760px"
      preset="card"
      class="config-modal"
      :mask-closable="false"
      :close-on-esc="false"
      @close="requestCloseConfig"
    >
      <div v-if="editProvider" class="config-shell">
        <n-form label-placement="left" label-width="90px">
          <div class="config-block">
            <div class="block-header">
              <div>
                <div class="section-label">基础配置</div>
                <p class="section-help">配置上游凭证。</p>
              </div>
            </div>
            <n-form-item label="API Key">
              <div class="api-key-row">
                <n-input v-model:value="form.api_key" :type="showApiKey ? 'text' : 'password'" placeholder="sk-..." class="mono api-key-input">
                  <template #suffix>
                    <button
                      type="button"
                      class="api-key-visibility-btn"
                      :aria-label="showApiKey ? '隐藏 API Key' : '显示 API Key'"
                      @click="showApiKey = !showApiKey"
                    >
                      <svg v-if="!showApiKey" xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                        <path d="M17.94 17.94A10.94 10.94 0 0 1 12 20c-7 0-11-8-11-8a21.76 21.76 0 0 1 5.06-6.94" />
                        <path d="M9.9 4.24A10.94 10.94 0 0 1 12 4c7 0 11 8 11 8a21.76 21.76 0 0 1-4.31 5.14" />
                        <path d="M14.12 14.12a3 3 0 1 1-4.24-4.24" />
                        <line x1="1" y1="1" x2="23" y2="23" />
                      </svg>
                      <svg v-else xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                        <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8S1 12 1 12z" />
                        <circle cx="12" cy="12" r="3" />
                      </svg>
                    </button>
                  </template>
                </n-input>
              </div>
            </n-form-item>
          </div>

          <div class="config-block">
            <div class="block-header">
              <div>
                <div class="section-label">通道配置</div>
                <p class="section-help">base_url 由 providers.json 决定，此处仅控制通道是否启用。</p>
              </div>
            </div>
            <div v-for="(ch, i) in form.channels" :key="ch.channel_type" class="channel-row-card">
              <div class="channel-row-main">
                <span class="channel-type-label mono">{{ channelLabel(ch.channel_type) }}</span>
                <n-input
                  v-model:value="form.channels[i].base_url"
                  size="small"
                  readonly
                  class="mono channel-url-input"
                  title="base_url 由配置文件决定，不可在此修改"
                />
              </div>
              <n-switch v-model:value="form.channels[i].is_enabled" size="small" />
            </div>
          </div>

          <div class="config-block">
            <div class="block-header model-block-header">
              <div>
                <div class="section-label">模型列表</div>
                <p class="section-help">左侧别名用于客户端请求；右侧填写实际上游模型或展示名。</p>
              </div>
              <n-space :size="8">
                <n-button
                  v-if="editProvider?.models_endpoint"
                  size="small"
                  secondary
                  @click="fetchModels"
                  :loading="fetching"
                  class="sync-btn mono"
                >
                  <template #icon>
                    <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                      <polyline points="23,4 23,10 17,10" />
                      <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" />
                    </svg>
                  </template>
                  同步
                </n-button>
                <n-button size="small" dashed @click="addModelRow" class="add-model-toolbar-btn mono">添加</n-button>
              </n-space>
            </div>
            <div class="model-toolbar">
              <span class="model-count-tip mono">{{ activeModelCount }} 条映射</span>
              <span v-if="editProvider?.models_endpoint" class="sync-tip">从 provider API 拉取模型并生成差异</span>
            </div>
            <div class="model-table">
              <div class="model-table-header mono">
                <span>别名</span>
                <span>上游模型 / 展示名</span>
                <span>操作</span>
              </div>
              <div class="model-list">
                <div v-for="(m, i) in form.models" :key="i" class="model-row">
                  <n-input v-model:value="form.models[i].model_id" size="small" placeholder="例如 claude-opus-4-1" class="mono model-id-input" />
                  <n-input v-model:value="form.models[i].model_name" size="small" placeholder="例如 claude-opus-4-1 ($15/$75)" class="mono model-name-input" />
                  <n-button size="small" type="error" tertiary @click="removeModelRow(i)" class="remove-btn mono">删除</n-button>
                </div>
                <div v-if="form.models.length === 0" class="model-empty">还没有模型映射，点击右上角“添加模型”开始配置。</div>
              </div>
            </div>
          </div>
        </n-form>
      </div>
      <template #footer>
        <n-space justify="space-between" align="center">
          <span class="dirty-tip mono" :class="{ active: isDirty }">{{ isDirty ? '有未保存变更' : '暂无变更' }}</span>
          <n-space justify="end">
            <n-button @click="requestCloseConfig">取消</n-button>
            <n-button type="primary" @click="handleSave" :loading="saving" :disabled="!isDirty">保存</n-button>
          </n-space>
        </n-space>
      </template>
    </n-modal>

    <n-modal v-model:show="showSync" title="同步差异" style="width: 520px" preset="card" class="sync-modal">
      <div v-if="diffResult.added.length" class="diff-section">
        <div class="diff-group-label added"><n-checkbox :checked="allChecked(diffResult.added)" :indeterminate="someChecked(diffResult.added)" @update:checked="(v) => toggleAll(diffResult.added, v)" /><span>✚ 新增 ({{ diffResult.added.length }})</span></div>
        <div v-for="(m, i) in diffResult.added" :key="'a' + i" class="diff-row"><n-checkbox v-model:checked="diffResult.added[i].checked" /><span class="mono">{{ m.model_id }}</span></div>
      </div>
      <div v-if="diffResult.removed.length" class="diff-section">
        <div class="diff-group-label removed"><n-checkbox :checked="allChecked(diffResult.removed)" :indeterminate="someChecked(diffResult.removed)" @update:checked="(v) => toggleAll(diffResult.removed, v)" /><span>✖ 删除 ({{ diffResult.removed.length }})</span></div>
        <div v-for="(m, i) in diffResult.removed" :key="'r' + i" class="diff-row"><n-checkbox v-model:checked="diffResult.removed[i].checked" /><span class="mono">{{ m.model_name }}</span></div>
      </div>
      <div v-if="diffResult.renamed.length" class="diff-section">
        <div class="diff-group-label renamed"><n-checkbox :checked="allChecked(diffResult.renamed)" :indeterminate="someChecked(diffResult.renamed)" @update:checked="(v) => toggleAll(diffResult.renamed, v)" /><span>✎ 改名 ({{ diffResult.renamed.length }})</span></div>
        <div v-for="(m, i) in diffResult.renamed" :key="'n' + i" class="diff-row"><n-checkbox v-model:checked="diffResult.renamed[i].checked" /><span class="mono">{{ m.local_name }} → {{ m.remote_name }}</span></div>
      </div>
      <template #footer><n-space justify="end"><n-button @click="showSync = false">取消</n-button><n-button type="primary" @click="applyDiff">应用选中</n-button></n-space></template>
    </n-modal>

    <n-modal v-model:show="showCloseConfirm" title="确认关闭" style="width: 420px" preset="card" class="confirm-modal">
      <div class="confirm-body">有未保存变更，确认关闭？</div>
      <template #footer>
        <n-space justify="end">
          <n-button @click="showCloseConfirm = false">继续编辑</n-button>
          <n-button class="confirm-close-btn" @click="confirmCloseConfig">确认关闭</n-button>
        </n-space>
      </template>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { NButton, NCheckbox, NForm, NFormItem, NInput, NModal, NSpace, NSpin, NSwitch, useMessage } from 'naive-ui'

const message = useMessage()
const API_BASE = '/api/admin'

interface ChannelInfo { channel_type: string; base_url: string; is_enabled: boolean }
interface ProviderSummary { id: string; name: string; icon?: string; is_enabled: boolean; channels: ChannelInfo[]; model_count: number }
interface ProviderDetail { id: string; name: string; api_key: string; is_enabled: boolean; models_endpoint?: string; channels: ChannelInfo[]; models: { id: string; provider_id: string; model_id: string; model_name: string }[] }
interface ModelForm { model_id: string; model_name: string }
interface DiffItem { model_id: string; model_name: string; checked: boolean }
interface DiffRenamed { model_id: string; local_name: string; remote_name: string; checked: boolean }
interface DiffResult { added: DiffItem[]; removed: DiffItem[]; renamed: DiffRenamed[] }
interface ChannelForm { channel_type: string; base_url: string; is_enabled: boolean }
interface FormState { api_key: string; is_enabled: boolean; channels: ChannelForm[]; models: ModelForm[] }

const providers = ref<ProviderSummary[]>([])
const loading = ref(false)
const showConfig = ref(false)
const editProvider = ref<ProviderDetail | null>(null)
const saving = ref(false)
const fetching = ref(false)
const showSync = ref(false)
const showCloseConfirm = ref(false)
const showApiKey = ref(false)
const initialSnapshot = ref('')
const diffResult = ref<DiffResult>({ added: [], removed: [], renamed: [] })
const form = ref<FormState>({ api_key: '', is_enabled: false, channels: [], models: [] })

const CHANNEL_LABELS: Record<string, string> = { openai_chat: 'OpenAI Chat', openai_responses: 'OpenAI Responses', anthropic: 'Anthropic' }

const isDirty = computed(() => snapshotForm(form.value) !== initialSnapshot.value)
const activeModelCount = computed(() => form.value.models.filter(m => m.model_id.trim()).length)

function channelLabel(type: string): string { return CHANNEL_LABELS[type] || type }
function iconUrl(icon: string): string { return /^https?:\/\//.test(icon) ? icon : `/icons/${icon}` }
function normId(s: string): string { return s.trim().toLowerCase() }

function snapshotForm(state: FormState): string {
  return JSON.stringify({
    api_key: state.api_key.trim(),
    is_enabled: state.is_enabled,
    channels: state.channels.map(c => ({ channel_type: c.channel_type, base_url: c.base_url.trim(), is_enabled: c.is_enabled })),
    models: state.models.map(m => ({ model_id: m.model_id.trim(), model_name: m.model_name.trim() }))
  })
}

async function loadProviders() {
  loading.value = true
  try {
    const res = await fetch(`${API_BASE}/providers`)
    const list: ProviderSummary[] = await res.json()
    providers.value = list.sort((a, b) => a.name.localeCompare(b.name))
  } finally {
    loading.value = false
  }
}

async function openConfig(summary: ProviderSummary) {
  const res = await fetch(`${API_BASE}/providers/${summary.id}`)
  editProvider.value = await res.json()
  const p = editProvider.value!
  form.value = {
    api_key: p.api_key || '',
    is_enabled: p.is_enabled,
    channels: p.channels.map(c => ({ channel_type: c.channel_type, base_url: c.base_url, is_enabled: c.is_enabled })),
    models: p.models.map(m => ({ model_id: m.model_id, model_name: m.model_name || m.model_id }))
  }
  showApiKey.value = false
  diffResult.value = { added: [], removed: [], renamed: [] }
  initialSnapshot.value = snapshotForm(form.value)
  showConfig.value = true
}

async function fetchModels() {
  if (!editProvider.value) return
  fetching.value = true
  try {
    const apiKey = form.value.api_key.trim()
    const res = await fetch(`${API_BASE}/providers/${editProvider.value.id}/fetch-models?api_key=${encodeURIComponent(apiKey)}`)
    if (res.ok) {
      const data = await res.json()
      const remote = (data.models || []).map((m: any) => ({ model_id: String(m.model_id), model_name: (m.model_name || String(m.model_id)).trim() }))
      diffResult.value = computeDiff(form.value.models, remote)
      const total = diffResult.value.added.length + diffResult.value.removed.length + diffResult.value.renamed.length
      if (total === 0) {
        message.info('模型列表已是最新')
      } else {
        showSync.value = true
      }
    } else {
      const err = await res.json()
      message.error(err.error || '获取模型列表失败')
    }
  } finally {
    fetching.value = false
  }
}

function computeDiff(local: ModelForm[], remote: { model_id: string; model_name: string }[]): DiffResult {
  const norm = (m: ModelForm) => m.model_name.trim() || m.model_id
  const localValid = local.filter(m => m.model_id.trim())
  const localMap = new Map<string, ModelForm>()
  for (const m of localValid) {
    const k = normId(m.model_id)
    if (!localMap.has(k)) localMap.set(k, m)
  }

  const remoteMap = new Map<string, { model_id: string; model_name: string }>()
  for (const m of remote) {
    const k = normId(m.model_id)
    if (!remoteMap.has(k)) remoteMap.set(k, m)
  }

  const added: DiffItem[] = []
  const removed: DiffItem[] = []
  const renamed: DiffRenamed[] = []

  for (const r of remoteMap.values()) {
    const k = normId(r.model_id)
    const l = localMap.get(k)
    if (!l) {
      added.push({ model_id: r.model_id, model_name: r.model_name, checked: true })
    } else if (norm(l) !== r.model_name) {
      renamed.push({ model_id: r.model_id, local_name: norm(l), remote_name: r.model_name, checked: false })
    }
  }

  for (const l of localMap.values()) {
    const k = normId(l.model_id)
    if (!remoteMap.has(k)) {
      removed.push({ model_id: l.model_id, model_name: norm(l), checked: false })
    }
  }

  return { added, removed, renamed }
}

function applyDiff() {
  const removedKeys = new Set(diffResult.value.removed.filter(r => r.checked).map(r => normId(r.model_id)))
  const renamedMap = new Map(diffResult.value.renamed.map(r => [normId(r.model_id), r]))
  const result: ModelForm[] = []

  let removedCount = 0
  let renamedCount = 0
  for (const l of form.value.models) {
    const k = normId(l.model_id)
    if (!k) {
      result.push(l)
      continue
    }
    if (removedKeys.has(k)) {
      removedCount++
      continue
    }
    const renamed = renamedMap.get(k)
    if (renamed && renamed.checked) {
      result.push({ model_id: l.model_id, model_name: renamed.remote_name })
      renamedCount++
    } else {
      result.push(l)
    }
  }

  let addedCount = 0
  for (const added of diffResult.value.added) {
    if (added.checked) {
      result.push({ model_id: added.model_id, model_name: added.model_name })
      addedCount++
    }
  }

  form.value.models = result
  showSync.value = false
  message.success(`已应用 ${addedCount + removedCount + renamedCount} 项变更`)
}

function allChecked(arr: { checked: boolean }[]): boolean { return arr.length > 0 && arr.every(m => m.checked) }
function someChecked(arr: { checked: boolean }[]): boolean { return arr.some(m => m.checked) && !arr.every(m => m.checked) }
function toggleAll(arr: { checked: boolean }[], v: boolean) { arr.forEach(m => { m.checked = v }) }
function addModelRow() { form.value.models.push({ model_id: '', model_name: '' }) }
function removeModelRow(index: number) { form.value.models.splice(index, 1) }

function closeConfig() {
  showConfig.value = false
  showCloseConfirm.value = false
  showApiKey.value = false
}

function requestCloseConfig() {
  if (isDirty.value) {
    showCloseConfirm.value = true
    return
  }
  closeConfig()
}

function confirmCloseConfig() {
  closeConfig()
}

async function handleSave() {
  if (!editProvider.value) return
  saving.value = true
  try {
    const body = {
      api_key: form.value.api_key,
      is_enabled: form.value.is_enabled,
      channels: form.value.channels.map(c => ({ channel_type: c.channel_type, base_url: c.base_url, is_enabled: c.is_enabled })),
      models: form.value.models.filter(m => m.model_id.trim()).map(m => ({ model_id: m.model_id.trim(), model_name: m.model_name.trim() || m.model_id.trim() }))
    }
    const res = await fetch(`${API_BASE}/providers/${editProvider.value.id}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body)
    })
    if (res.ok) {
      message.success('保存成功')
      initialSnapshot.value = snapshotForm(form.value)
      closeConfig()
      loadProviders()
    } else {
      const err = await res.json()
      message.error(err.error || '保存失败')
    }
  } finally {
    saving.value = false
  }
}

async function quickToggle(p: ProviderSummary, enabled: boolean) {
  const res = await fetch(`${API_BASE}/providers/${p.id}`)
  const detail = await res.json()
  const body = {
    api_key: detail.api_key || '',
    is_enabled: enabled,
    channels: detail.channels.map((c: ChannelInfo) => ({ channel_type: c.channel_type, base_url: c.base_url, is_enabled: c.is_enabled })),
    models: detail.models.map((m: any) => ({ model_id: m.model_id, model_name: m.model_name || m.model_id }))
  }
  await fetch(`${API_BASE}/providers/${p.id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body)
  })
  loadProviders()
}

onMounted(loadProviders)
</script>

<style scoped>
.page-header { margin-bottom: 24px; }
.page-title { font-size: 28px; font-weight: 600; color: #17140f; margin: 0; letter-spacing: -0.02em; }
.page-subtitle { margin: 6px 0 0; color: #a89e8c; font-size: 13px; }

.provider-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 16px; }
.provider-card { background: #ffffff; border: 1px solid #d9cfbf; border-radius: 12px; padding: 22px; cursor: pointer; transition: box-shadow 0.2s, border-color 0.2s, transform 0.15s; }
.provider-card:hover { border-color: rgba(46, 168, 106, 0.3); box-shadow: 0 6px 22px rgba(23,20,15,0.06); transform: translateY(-1px); }
.provider-card.disabled { opacity: 0.55; }
.provider-card.disabled:hover { border-color: rgba(181, 132, 43, 0.25); box-shadow: 0 4px 24px rgba(181, 132, 43, 0.06); }
.card-top { display: flex; justify-content: space-between; align-items: center; margin-bottom: 14px; }
.card-name { display: flex; align-items: center; gap: 10px; }
.card-icon { width: 24px; height: 24px; object-fit: contain; flex-shrink: 0; }
.card-name-text { font-size: 17px; font-weight: 600; color: #17140f; letter-spacing: -0.01em; }
.card-channels { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 16px; }
.channel-tag { display: flex; align-items: center; gap: 5px; font-size: 11px; padding: 4px 10px; background: #f4efe3; border: 1px solid #d9cfbf; border-radius: 8px; color: #74695a; }
.channel-tag.off { text-decoration: line-through; opacity: 0.5; }
.card-footer { display: flex; justify-content: space-between; align-items: center; }
.model-count { font-size: 12px; color: #a89e8c; }
.card-arrow { font-size: 14px; color: #c9c0b0; transition: color 0.2s, transform 0.2s; }
.provider-card:hover .card-arrow { color: #1d7a4c; transform: translateX(3px); }

.config-modal { --n-title-text-color: #17140f; }
.config-shell { display: flex; flex-direction: column; gap: 16px; }
.config-block { border: 1px solid #e5dccd; border-radius: 12px; background: #fcfaf6; padding: 16px; margin-top: 16px; }
.block-header { display: flex; justify-content: space-between; align-items: flex-start; gap: 12px; margin-bottom: 12px; }
.model-block-header { align-items: center; }
.section-label { display: flex; align-items: center; gap: 8px; font-family: 'IBM Plex Mono', monospace; font-size: 12px; font-weight: 600; color: #8c7f6c; text-transform: uppercase; letter-spacing: 0.06em; margin-bottom: 4px; }
.section-help { margin: 0; color: #a89e8c; font-size: 12px; line-height: 1.5; }
.api-key-row { width: 100%; }
.api-key-input { width: 100%; }
.api-key-visibility-btn { display: inline-flex; align-items: center; justify-content: center; border: 0; background: transparent; padding: 0; color: #8c7f6c; cursor: pointer; }
.api-key-visibility-btn:hover { color: #1d7a4c; }
.sync-btn { font-size: 12px; color: #1d7a4c; }
.sync-btn:focus:not(:focus-visible) { background-color: transparent; }
.channel-row-card { display: flex; align-items: center; justify-content: space-between; gap: 12px; border: 1px solid #e5dccd; border-radius: 10px; background: #fff; padding: 10px 12px; margin-bottom: 8px; }
.channel-row-main { display: flex; align-items: center; gap: 10px; flex: 1; min-width: 0; }
.channel-type-label { font-size: 12px; width: 120px; flex-shrink: 0; color: #74695a; }
.channel-url-input { flex: 1; }
.model-toolbar { display: flex; justify-content: space-between; align-items: center; gap: 12px; margin-bottom: 10px; }
.model-count-tip { font-size: 12px; color: #8c7f6c; }
.sync-tip { color: #a89e8c; font-size: 12px; }
.model-table { border: 1px solid #e5dccd; border-radius: 10px; background: #fff; overflow: hidden; }
.model-table-header { display: grid; grid-template-columns: 180px minmax(0, 1fr) 76px; gap: 8px; padding: 10px 12px; background: #f7f2e8; color: #8c7f6c; font-size: 11px; text-transform: uppercase; letter-spacing: 0.06em; }
.model-list { max-height: 280px; overflow-y: auto; padding: 8px; }
.model-row { display: grid; grid-template-columns: 180px minmax(0, 1fr) 76px; gap: 8px; align-items: center; margin-bottom: 8px; }
.model-row:last-child { margin-bottom: 0; }
.model-id-input { width: 100%; flex-shrink: 0; }
.model-name-input { flex: 1; }
.remove-btn { width: 100%; }
.add-model-toolbar-btn { border-radius: 8px; }
.model-empty { padding: 28px 12px; color: #a89e8c; text-align: center; font-size: 13px; }
.dirty-tip { font-size: 12px; color: #a89e8c; }
.dirty-tip.active { color: #b5842b; }

.sync-modal { --n-title-text-color: #17140f; }
.confirm-modal { --n-title-text-color: #17140f; }
.confirm-body { color: #4b443a; font-size: 14px; line-height: 1.6; }
.confirm-close-btn {
  /* !important 覆盖 Naive NButton default type 的 inline `--n-color: initial`，否则 scoped 选择器优先级不够 */
  --n-color: #b3261e !important;
  --n-color-hover: #9f2019 !important;
  --n-color-pressed: #851a15 !important;
  --n-border: 1px solid #b3261e !important;
  --n-border-hover: 1px solid #9f2019 !important;
  --n-border-pressed: 1px solid #851a15 !important;
  --n-text-color: #fff7f5 !important;
  --n-text-color-hover: #fff7f5 !important;
  --n-text-color-pressed: #fff7f5 !important;
}
.diff-section { margin-bottom: 16px; }
.diff-section:last-child { margin-bottom: 0; }
.diff-group-label { display: flex; align-items: center; gap: 6px; font-family: 'IBM Plex Mono', monospace; font-size: 12px; font-weight: 600; margin-bottom: 8px; }
.diff-group-label.added { color: #1d7a4c; }
.diff-group-label.removed { color: #b3261e; }
.diff-group-label.renamed { color: #b5842b; }
.diff-row { display: flex; align-items: center; gap: 8px; padding: 3px 0; font-size: 13px; }

@media (max-width: 780px) {
  .model-table-header,
  .model-row {
    grid-template-columns: 1fr;
  }

  .model-table-header span:last-child {
    display: none;
  }

  .remove-btn {
    width: auto;
    justify-self: end;
  }
}
</style>
