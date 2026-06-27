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

    <n-modal v-model:show="showConfig" :title="editProvider?.name + ' 配置'" style="width: 640px" preset="card" class="config-modal">
      <n-form v-if="editProvider" label-placement="left" label-width="90px">
        <n-form-item label="API Key"><n-input v-model:value="form.api_key" type="password" placeholder="sk-..." class="mono" /></n-form-item>
        <div class="form-section"><div class="section-label">通道配置</div>
          <div v-for="(ch, i) in form.channels" :key="ch.channel_type" class="channel-row">
            <span class="channel-type-label mono">{{ channelLabel(ch.channel_type) }}</span>
            <n-input v-model:value="form.channels[i].base_url" size="small" readonly class="mono channel-url-input" title="base_url 由配置文件决定，不可在此修改" /><n-switch v-model:value="form.channels[i].is_enabled" size="small" />
          </div>
        </div>
        <div class="form-section"><div class="section-label">模型列表
          <n-button v-if="editProvider?.models_endpoint" size="tiny" quaternary @click="fetchModels" :loading="fetching" class="sync-btn mono">
            <template #icon><svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="23,4 23,10 17,10" /><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10" /></svg></template>从 API 同步</n-button></div>
          <div class="model-list">
            <div v-for="(m, i) in form.models" :key="i" class="model-row">
              <n-input v-model:value="form.models[i].model_id" size="small" placeholder="模型ID（发往上游）" class="mono model-id-input" />
              <n-input v-model:value="form.models[i].model_name" size="small" placeholder="展示名" class="mono model-name-input" />
              <n-button size="tiny" type="error" text @click="form.models.splice(i, 1)" class="remove-btn"><svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg></n-button>
            </div>
            <n-button size="small" dashed @click="form.models.push({ model_id: '', model_name: '' })" class="add-model-btn mono">+ 添加模型</n-button>
          </div>
        </div>
      </n-form>
      <template #footer><n-space justify="end"><n-button @click="showConfig = false">取消</n-button><n-button type="primary" @click="handleSave" :loading="saving">保存</n-button></n-space></template>
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
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { NButton, NModal, NForm, NFormItem, NInput, NSpace, NSwitch, NSpin, NCheckbox, useMessage } from 'naive-ui'
const message = useMessage(); const API_BASE = '/api/admin'
interface ChannelInfo { channel_type: string; base_url: string; is_enabled: boolean }
interface ProviderSummary { id: string; name: string; icon?: string; is_enabled: boolean; channels: ChannelInfo[]; model_count: number }
interface ProviderDetail { id: string; name: string; api_key: string; is_enabled: boolean; models_endpoint?: string; channels: ChannelInfo[]; models: { id: string; provider_id: string; model_id: string; model_name: string }[] }
interface ModelForm { model_id: string; model_name: string }
interface DiffItem { model_id: string; model_name: string; checked: boolean }
interface DiffRenamed { model_id: string; local_name: string; remote_name: string; checked: boolean }
interface DiffResult { added: DiffItem[]; removed: DiffItem[]; renamed: DiffRenamed[] }
interface ChannelForm { channel_type: string; base_url: string; is_enabled: boolean }
const providers = ref<ProviderSummary[]>([]); const loading = ref(false); const showConfig = ref(false); const editProvider = ref<ProviderDetail | null>(null); const saving = ref(false); const fetching = ref(false); const showSync = ref(false); const diffResult = ref<DiffResult>({ added: [], removed: [], renamed: [] })
const form = ref({ api_key: '', is_enabled: false, channels: [] as ChannelForm[], models: [] as ModelForm[] })
const CHANNEL_LABELS: Record<string, string> = { openai_chat: 'OpenAI Chat', openai_responses: 'OpenAI Responses', anthropic: 'Anthropic' }
function channelLabel(type: string): string { return CHANNEL_LABELS[type] || type }
function iconUrl(icon: string): string { return /^https?:\/\//.test(icon) ? icon : `/icons/${icon}` }
async function loadProviders() { loading.value = true; try { const res = await fetch(`${API_BASE}/providers`); providers.value = await res.json() } finally { loading.value = false } }
async function openConfig(summary: ProviderSummary) { const res = await fetch(`${API_BASE}/providers/${summary.id}`); editProvider.value = await res.json(); const p = editProvider.value!; form.value = { api_key: p.api_key || '', is_enabled: p.is_enabled, channels: p.channels.map(c => ({ channel_type: c.channel_type, base_url: c.base_url, is_enabled: c.is_enabled })), models: p.models.map(m => ({ model_id: m.model_id, model_name: m.model_name || m.model_id })) }; showConfig.value = true }
async function fetchModels() { if (!editProvider.value) return; fetching.value = true; try { const apiKey = form.value.api_key.trim(); const res = await fetch(`${API_BASE}/providers/${editProvider.value.id}/fetch-models?api_key=${encodeURIComponent(apiKey)}`); if (res.ok) { const data = await res.json(); const remote = (data.models || []).map((m: any) => ({ model_id: String(m.model_id), model_name: (m.model_name || String(m.model_id)).trim() })); diffResult.value = computeDiff(form.value.models, remote); const total = diffResult.value.added.length + diffResult.value.removed.length + diffResult.value.renamed.length; if (total === 0) { message.info('模型列表已是最新') } else { showSync.value = true } } else { const err = await res.json(); message.error(err.error || '获取模型列表失败') } } finally { fetching.value = false } }
function normId(s: string): string { return s.trim().toLowerCase() }
function computeDiff(local: ModelForm[], remote: { model_id: string; model_name: string }[]): DiffResult { const norm = (m: ModelForm) => m.model_name.trim() || m.model_id; const localValid = local.filter(m => m.model_id.trim()); const localMap = new Map<string, ModelForm>(); for (const m of localValid) { const k = normId(m.model_id); if (!localMap.has(k)) localMap.set(k, m) } const remoteMap = new Map<string, { model_id: string; model_name: string }>(); for (const m of remote) { const k = normId(m.model_id); if (!remoteMap.has(k)) remoteMap.set(k, m) } const added: DiffItem[] = []; const removed: DiffItem[] = []; const renamed: DiffRenamed[] = []; for (const r of remoteMap.values()) { const k = normId(r.model_id); const l = localMap.get(k); if (!l) { added.push({ model_id: r.model_id, model_name: r.model_name, checked: true }) } else if (norm(l) !== r.model_name) { renamed.push({ model_id: r.model_id, local_name: norm(l), remote_name: r.model_name, checked: false }) } } for (const l of localMap.values()) { const k = normId(l.model_id); if (!remoteMap.has(k)) { removed.push({ model_id: l.model_id, model_name: norm(l), checked: false }) } } return { added, removed, renamed } }
function applyDiff() { const removedKeys = new Set(diffResult.value.removed.filter(r => r.checked).map(r => normId(r.model_id))); const renamedMap = new Map(diffResult.value.renamed.map(r => [normId(r.model_id), r])); const result: ModelForm[] = []; let removedCount = 0; let renamedCount = 0; for (const l of form.value.models) { const k = normId(l.model_id); if (!k) { result.push(l); continue } if (removedKeys.has(k)) { removedCount++; continue } const rn = renamedMap.get(k); if (rn && rn.checked) { result.push({ model_id: l.model_id, model_name: rn.remote_name }); renamedCount++ } else { result.push(l) } } let addedCount = 0; for (const a of diffResult.value.added) { if (a.checked) { result.push({ model_id: a.model_id, model_name: a.model_name }); addedCount++ } } form.value.models = result; showSync.value = false; const n = addedCount + removedCount + renamedCount; message.success(`已应用 ${n} 项变更`) }
function allChecked(arr: { checked: boolean }[]): boolean { return arr.length > 0 && arr.every(m => m.checked) }
function someChecked(arr: { checked: boolean }[]): boolean { return arr.some(m => m.checked) && !arr.every(m => m.checked) }
function toggleAll(arr: { checked: boolean }[], v: boolean) { arr.forEach(m => { m.checked = v }) }
async function handleSave() { if (!editProvider.value) return; saving.value = true; try { const body = { api_key: form.value.api_key, is_enabled: form.value.is_enabled, channels: form.value.channels.map(c => ({ channel_type: c.channel_type, base_url: c.base_url, is_enabled: c.is_enabled })), models: form.value.models.filter(m => m.model_id.trim()).map(m => ({ model_id: m.model_id.trim(), model_name: m.model_name.trim() || m.model_id.trim() })) }; const res = await fetch(`${API_BASE}/providers/${editProvider.value.id}`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }); if (res.ok) { message.success('保存成功'); showConfig.value = false; loadProviders() } else { const err = await res.json(); message.error(err.error || '保存失败') } } finally { saving.value = false } }
async function quickToggle(p: ProviderSummary, enabled: boolean) { const res = await fetch(`${API_BASE}/providers/${p.id}`); const detail = await res.json(); const body = { api_key: detail.api_key || '', is_enabled: enabled, channels: detail.channels.map((c: ChannelInfo) => ({ channel_type: c.channel_type, base_url: c.base_url, is_enabled: c.is_enabled })), models: detail.models.map((m: any) => ({ model_id: m.model_id, model_name: m.model_name || m.model_id })) }; await fetch(`${API_BASE}/providers/${p.id}`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }); loadProviders() }
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
.provider-card.disabled:hover { border-color: rgba(179, 38, 30, 0.25); box-shadow: 0 4px 24px rgba(179, 38, 30, 0.06); }
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
.form-section { margin-top: 20px; }
.section-label { display: flex; align-items: center; gap: 8px; font-family: 'IBM Plex Mono', monospace; font-size: 12px; font-weight: 600; color: #a89e8c; text-transform: uppercase; letter-spacing: 0.06em; margin-bottom: 10px; }
.sync-btn { font-size: 11px; color: #1d7a4c; }
.sync-btn:focus:not(:focus-visible) { background-color: transparent; }
.channel-row { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
.channel-type-label { font-size: 12px; width: 120px; flex-shrink: 0; color: #74695a; }
.channel-url-input { flex: 1; }
.model-list { max-height: 240px; overflow-y: auto; }
.model-row { display: flex; align-items: center; gap: 6px; margin-bottom: 6px; }
.model-id-input { width: 140px; flex-shrink: 0; }
.model-name-input { flex: 1; }
.remove-btn { opacity: 0.3; transition: opacity 0.15s; }
.remove-btn:hover { opacity: 1; }
.add-model-btn { width: 100%; font-size: 12px; color: #a89e8c; border-color: #d9cfbf; border-radius: 8px; margin-top: 4px; }

.sync-modal { --n-title-text-color: #17140f; }
.diff-section { margin-bottom: 16px; }
.diff-section:last-child { margin-bottom: 0; }
.diff-group-label { display: flex; align-items: center; gap: 6px; font-family: 'IBM Plex Mono', monospace; font-size: 12px; font-weight: 600; margin-bottom: 8px; }
.diff-group-label.added { color: #1d7a4c; }
.diff-group-label.removed { color: #b3261e; }
.diff-group-label.renamed { color: #b5842b; }
.diff-row { display: flex; align-items: center; gap: 8px; padding: 3px 0; font-size: 13px; }
</style>