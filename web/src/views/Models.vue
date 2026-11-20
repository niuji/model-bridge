<template>
  <div class="models-page">
    <header class="page-header">
      <div>
        <div class="page-eyebrow mono">CHANNEL OVERVIEW</div>
        <h1 class="page-title serif">通道信息<span class="title-dot">.</span></h1>
        <p class="page-subtitle">查看通道模型与最近 7 天的请求和 Token 用量。</p>
      </div>
      <n-button :loading="loading" class="refresh-button" @click="refresh">
        <template #icon>
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M20 7v5h-5" /><path d="M4 17v-5h5" /><path d="M6.1 7a7 7 0 0 1 11.6-1L20 9M4 15l2.3 3A7 7 0 0 0 17.9 17" /></svg>
        </template>
        刷新
      </n-button>
    </header>

    <n-alert v-if="error" type="error" class="load-error">{{ error }}</n-alert>

    <div class="channel-grid" role="group" aria-label="选择通道">
      <button
        v-for="channel in channelDefs"
        :key="channel.type"
        type="button"
        class="channel-card"
        :class="[channel.tone, { selected: selectedChannel === channel.type }]"
        :aria-pressed="selectedChannel === channel.type"
        @click="selectedChannel = channel.type"
      >
        <div class="channel-card-top">
          <span class="channel-symbol" aria-hidden="true">
            <svg v-if="channel.type === 'openai_chat'" width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M20 11.5a7.5 7.5 0 0 1-7.5 7.5H5l-3 3V11.5A7.5 7.5 0 0 1 9.5 4h3a7.5 7.5 0 0 1 7.5 7.5Z" /><path d="M7 10h8M7 14h5" /></svg>
            <svg v-else-if="channel.type === 'openai_responses'" width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="m8 5-6 7 6 7M16 5l6 7-6 7M14 4l-4 16" /></svg>
            <svg v-else-if="channel.type === 'system_one'" width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M9 12l2 2 4-4M12 3l9 9-9 9-9-9Z" /></svg>
            <svg v-else width="19" height="19" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="m12 3 0 18M3 12h18M5.6 5.6l12.8 12.8M5.6 18.4 18.4 5.6M8.5 3.5l7 17M3.5 8.5l17 7M3.5 15.5l17-7M8.5 20.5l7-17" /></svg>
          </span>
        </div>
        <div class="channel-heading">
          <span class="channel-name">{{ channel.label }}</span>
          <span class="channel-description mono">{{ channel.description }}</span>
        </div>
        <div class="channel-card-bottom">
          <div class="channel-total"><strong class="mono">{{ loading && !channels.length ? '—' : channelModels(channel.type).length }}</strong></div>
          <span class="channel-providers">{{ providerCount(channel.type) }} 家供应商</span>
        </div>
      </button>
    </div>

    <section class="models-panel" :aria-label="`${selectedDef.label} 模型列表`">
      <div class="panel-heading">
        <div class="panel-heading-main">
          <span class="section-marker" :class="selectedDef.tone" />
          <h2>{{ selectedDef.label }}</h2>
          <span class="list-count mono">{{ models.length }}</span>
        </div>
        <span class="route-status"><span />当前生效路由</span>
      </div>
      <div v-if="providerGroups.length" class="provider-grid">
        <div v-for="(column, index) in providerColumns" :key="index" class="provider-column">
          <section v-for="provider in column" :key="provider.id" class="provider-group" :aria-label="`${provider.name} 的模型`">
            <div class="provider-group-heading">
              <span class="provider-avatar" aria-hidden="true"><img v-if="provider.icon" class="provider-icon" :src="iconUrl(provider.icon)" alt="" /><span v-else>{{ provider.name.charAt(0).toUpperCase() }}</span></span>
              <h3>{{ provider.name }}</h3><span class="provider-model-count mono">{{ provider.models.length }} 个模型</span>
            </div>
            <ul class="provider-models">
              <li v-for="model in provider.models" :key="model.id" class="model-item">
                <span class="model-glyph mono" aria-hidden="true">◇</span>
                <div class="model-detail"><code class="model-id mono">{{ model.id }}</code><span v-if="model.name && model.name !== model.id" class="model-name">{{ model.name }}</span></div>
              </li>
            </ul>
          </section>
        </div>
      </div>
      <div v-if="!providerGroups.length" class="models-empty"><span class="empty-symbol mono">∅</span><span>{{ loading ? '正在加载模型' : '该通道暂无可用模型' }}</span><span class="empty-hint">在供应商页面配置模型并启用对应通道</span></div>
    </section>
    <channel-usage :channel="selectedChannel" :channel-label="selectedDef.label" :refresh-key="refreshKey" :providers="providerMeta" />
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { NAlert, NButton } from 'naive-ui'
import ChannelUsage from '../components/ChannelUsage.vue'
import { balancedColumns } from '../utils/balancedColumns'
import { adminRequest, errorMessage } from '../api'

interface Model { id: string; name: string; provider_id: string; provider_name: string; provider_icon?: string | null }
interface Channel { channel_type: string; models: Model[] }

const props = defineProps<{ systemOne?: boolean }>()
const channelDefs = props.systemOne ? [
  { type: 'system_one', label: 'System One', description: 'Decisions API', tone: 'tone-chat' },
] : [
  { type: 'openai_chat', label: 'OpenAI Chat', description: 'Chat Completions', tone: 'tone-chat' },
  { type: 'openai_responses', label: 'OpenAI Responses', description: 'Responses API', tone: 'tone-responses' },
  { type: 'anthropic', label: 'Anthropic', description: 'Messages API', tone: 'tone-anthropic' }
]
const refreshKey = ref(0)
const channels = ref<Channel[]>([])
const selectedChannel = ref(channelDefs[0].type)
const loading = ref(false)
const error = ref('')
const selectedDef = computed(() => channelDefs.find(c => c.type === selectedChannel.value) || channelDefs[0])
const currentChannel = computed(() => channels.value.find(c => c.channel_type === selectedChannel.value))
const models = computed(() => currentChannel.value?.models || [])
const providerMeta = computed(() => Object.fromEntries(channels.value.flatMap(channel => channel.models.map(model => [model.provider_id, { name: model.provider_name, icon: model.provider_icon }]))))
const providerGroups = computed(() => {
  const groups = new Map<string, { id: string; name: string; icon?: string | null; models: Model[] }>()
  for (const model of models.value) {
    const group = groups.get(model.provider_id) || { id: model.provider_id, name: model.provider_name || model.provider_id, icon: model.provider_icon, models: [] }
    group.models.push(model)
    groups.set(model.provider_id, group)
  }
  return [...groups.values()].sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id))
})
// 估算值包含卡片头部、间距及模型行；相差不超过一行时，卡片少的列优先放左侧。
const providerColumns = computed(() => balancedColumns(providerGroups.value, provider => 74 + provider.models.length * 48, 48).filter(column => column.length))

function channelModels(type: string): Model[] {
  return channels.value.find(c => c.channel_type === type)?.models || []
}

function providerCount(type: string): number {
  return new Set(channelModels(type).map(model => model.provider_id)).size
}

function iconUrl(icon: string): string {
  return /^https?:\/\//.test(icon) ? icon : `/icons/${icon}`
}

function refresh() {
  refreshKey.value += 1
  void loadModels()
}

async function loadModels() {
  if (loading.value) return
  loading.value = true
  error.value = ''
  try {
    const list: Channel[] = await (await adminRequest('/channels/models')).json()
    channels.value = list.filter(channel => (channel.channel_type === 'system_one') === !!props.systemOne)
  } catch (cause) {
    error.value = errorMessage(cause, '加载通道模型失败')
  } finally {
    loading.value = false
  }
}

onMounted(loadModels)
</script>

<style scoped>
.models-page { max-width: 1200px; margin: 0 auto; }
.page-header { display: flex; align-items: center; justify-content: space-between; gap: 20px; margin-bottom: 30px; }
.page-eyebrow { color: var(--mb-text-3); font-size: 10px; font-weight: 500; letter-spacing: 0.16em; margin-bottom: 8px; }
.page-title { margin: 0; font-size: 30px; line-height: 1.25; font-weight: 600; letter-spacing: -0.04em; color: var(--mb-text-1); }
.title-dot { color: var(--mb-primary); margin-left: 3px; }
.page-subtitle { margin: 10px 0 0; font-size: 13px; color: var(--mb-text-2); }
.refresh-button { flex-shrink: 0; }
.load-error { margin-bottom: 18px; }
.channel-grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; margin-bottom: 20px; }
.tone-chat { --channel-color: var(--mb-cyan); --channel-tint: var(--mb-tint-cyan); }
.tone-responses { --channel-color: var(--mb-primary); --channel-tint: var(--mb-tint-blue); }
.tone-anthropic { --channel-color: var(--mb-warning); --channel-tint: var(--mb-tint-amber); }
.channel-card { display: grid; grid-template-columns: 32px minmax(0, 1fr) auto; align-items: center; gap: 12px; appearance: none; position: relative; padding: 16px; text-align: left; font: inherit; color: var(--mb-text-1); background: var(--mb-surface); border: 1px solid var(--mb-border); border-radius: 14px; cursor: pointer; transition: border-color 0.18s, box-shadow 0.18s, background 0.18s; }
.channel-card:hover { border-color: var(--channel-color); box-shadow: var(--mb-shadow-2); }
.channel-card:focus-visible { outline: 2px solid var(--channel-color); outline-offset: 4px; }
.channel-card.selected { border-color: var(--channel-color); background: linear-gradient(135deg, var(--channel-tint), var(--mb-surface) 70%); box-shadow: 0 0 0 1px var(--channel-color); }
.channel-card-top { display: flex; align-items: center; justify-content: space-between; }
.channel-symbol { display: inline-flex; align-items: center; justify-content: center; width: 32px; height: 32px; color: var(--channel-color); background: var(--channel-tint); border-radius: 10px; }
.channel-heading { display: flex; flex-direction: column; gap: 4px; }
.channel-name { font-size: 14px; font-weight: 600; letter-spacing: -0.02em; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.channel-description { font-size: 10px; color: var(--mb-text-3); }
.channel-card-bottom { display: flex; flex-direction: column; align-items: flex-end; gap: 6px; }
.channel-total { display: flex; align-items: baseline; gap: 9px; }
.channel-total strong { font-size: 24px; font-weight: 500; line-height: 1; letter-spacing: -0.06em; font-variant-numeric: tabular-nums; }
.channel-providers { font-size: 11px; color: var(--mb-text-2); }
.provider-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); align-items: start; gap: 14px; padding: 0 20px 20px; }
.provider-column { display: flex; flex-direction: column; min-width: 0; gap: 14px; }
.provider-group { min-width: 0; border: 1px solid var(--mb-border); border-radius: 10px; overflow: hidden; }
.provider-group-heading { display: flex; align-items: center; gap: 10px; padding: 14px 16px; background: var(--mb-surface-inset); border-bottom: 1px solid var(--mb-divider); }
.provider-group-heading h3 { margin: 0; font-size: 13px; font-weight: 600; color: var(--mb-text-1); overflow-wrap: anywhere; }
.provider-model-count { flex-shrink: 0; margin-left: auto; font-size: 10px; color: var(--mb-text-3); }
.provider-models { list-style: none; padding: 0; margin: 0; }
.model-item { display: flex; align-items: flex-start; gap: 10px; padding: 12px 16px; }
.model-item + .model-item { border-top: 1px solid var(--mb-divider); }
.model-glyph { color: var(--mb-primary); font-size: 15px; line-height: 20px; }
.model-detail { display: flex; flex-direction: column; min-width: 0; gap: 4px; }
.model-id { font-size: 12px; line-height: 20px; color: var(--mb-text-1); overflow-wrap: anywhere; user-select: all; }
.model-name { font-size: 11px; line-height: 1.5; color: var(--mb-text-3); overflow-wrap: anywhere; }
.provider-avatar { display: inline-flex; align-items: center; justify-content: center; width: 30px; height: 30px; flex-shrink: 0; border: 1px solid var(--mb-border); border-radius: 9px; background: var(--mb-surface); font-size: 12px; font-weight: 600; color: var(--mb-text-2); }
.provider-icon { width: 20px; height: 20px; object-fit: contain; }
.models-panel { min-width: 0; border: 1px solid var(--mb-border); border-radius: 14px; background: var(--mb-surface); overflow: hidden; }
.panel-heading { display: flex; align-items: center; justify-content: space-between; flex-wrap: wrap; gap: 12px; padding: 20px 24px; }
.panel-heading-main { display: flex; align-items: center; gap: 10px; }
.section-marker { width: 3px; height: 16px; border-radius: 2px; background: var(--channel-color); }
.panel-heading h2 { margin: 0; font-size: 15px; font-weight: 600; color: var(--mb-text-1); }
.list-count { padding: 2px 7px; font-size: 10px; line-height: 1.5; color: var(--mb-text-2); background: var(--mb-surface-inset); border: 1px solid var(--mb-border); border-radius: 5px; }
.route-status { display: inline-flex; align-items: center; gap: 6px; font-size: 10px; color: var(--mb-text-3); }
.route-status > span { height: 5px; width: 5px; background: var(--mb-success); border-radius: 50%; }
.models-empty { display: flex; align-items: center; flex-direction: column; gap: 8px; padding: 28px 16px; font-size: 13px; color: var(--mb-text-2); }
.empty-symbol { font-size: 30px; color: var(--mb-text-3); margin-bottom: 5px; }
.empty-hint { font-size: 11px; color: var(--mb-text-3); }
@media (max-width: 1000px) {
  .channel-grid { gap: 12px; }
  .channel-card { padding: 12px; gap: 10px; }
  .channel-name { font-size: 12px; }
  .channel-providers { font-size: 10px; }
}
@media (max-width: 850px) {
  .provider-grid { grid-template-columns: 1fr; }
}
@media (max-width: 640px) {
  .page-header { align-items: center; gap: 12px; margin-bottom: 24px; }
  .page-title { font-size: 26px; }
  .page-subtitle { font-size: 12px; line-height: 1.6; }
  .page-eyebrow { font-size: 9px; letter-spacing: 0.1em; }
  .channel-grid { grid-template-columns: 1fr; gap: 10px; margin-bottom: 20px; }
  .channel-card { display: grid; grid-template-columns: 32px minmax(0, 1fr) auto; align-items: center; gap: 12px; padding: 16px; }
  .channel-card-top { margin: 0; }
  .channel-symbol { width: 32px; height: 32px; border-radius: 9px; }
  .channel-name { font-size: 13px; }
  .channel-description { font-size: 9px; }
  .channel-card-bottom { flex-direction: column; align-items: flex-end; gap: 5px; margin: 0; }
  .channel-total strong { font-size: 24px; }
  .channel-providers { font-size: 9px; padding: 0; }
  .panel-heading { padding: 16px; gap: 8px; }
  .route-status { font-size: 9px; }
  .provider-grid { grid-template-columns: 1fr; padding: 0 16px 16px; }
  .provider-group-heading, .model-item { padding-left: 12px; padding-right: 12px; }
}
@media (prefers-reduced-motion: reduce) {
  .channel-card { transition: none; }
}
</style>
