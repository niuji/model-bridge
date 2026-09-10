<template>
  <div class="dashboard">
    <div class="page-header">
      <h1 class="page-title serif">仪表盘</h1>
      <p class="page-subtitle mono">用量概览 · 最近 7 天</p>
    </div>

    <n-alert v-if="errors.overview" type="error" class="section-error" data-section="overview">
      {{ errors.overview }} <n-button size="small" @click="loadSection('overview')">重试</n-button>
    </n-alert>
    <n-spin v-else :show="loading.overview">
      <div class="stat-grid">
        <div class="stat-card">
          <div class="stat-icon ico-g">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="22,12 18,12 15,21 9,3 6,12 2,12" /></svg>
          </div>
          <div class="stat-label">总请求数</div>
          <div class="stat-value mono">{{ overview ? formatNum(overview.total_requests) : '—' }}</div>
        </div>
        <div class="stat-card">
          <div class="stat-icon ico-a">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15,14 20,9 15,4" /><path d="M4 20v-7a4 4 0 0 1 4-4h12" /></svg>
          </div>
          <div class="stat-label">输入 Token</div>
          <div class="stat-value mono">{{ overview ? formatNum(overview.total_input_tokens) : '—' }}</div>
        </div>
        <div class="stat-card">
          <div class="stat-icon ico-t">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9,10 4,15 9,20" /><path d="M20 4v7a4 4 0 0 1-4 4H4" /></svg>
          </div>
          <div class="stat-label">输出 Token</div>
          <div class="stat-value mono">{{ overview ? formatNum(overview.total_output_tokens) : '—' }}</div>
        </div>
        <div class="stat-card">
          <div class="stat-icon ico-l">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10" /><polyline points="12,6 12,12 16,14" /></svg>
          </div>
          <div class="stat-label">平均延迟</div>
          <div class="stat-value mono">{{ overview ? Math.round(overview.avg_latency_ms) : '—' }}<span class="stat-unit">ms</span></div>
        </div>
      </div>

    </n-spin>

    <div class="card chart-card" data-section="usage">
      <div class="card-header">
        <h3 class="card-title">{{ byHour ? '每小时 Token 用量' : '每天 Token 用量' }}</h3>
        <div class="chart-header-right">
          <span class="card-badge mono">{{ byHour ? '近 7 天' : '近 30 天' }}</span>
          <n-switch :value="byHour" size="small" @update:value="onModeChange">
            <template #checked>小时</template>
            <template #unchecked>天</template>
          </n-switch>
        </div>
      </div>
      <div class="card-body">
        <n-alert v-if="errors[barSection]" type="error" class="section-error">
          {{ errors[barSection] }} <n-button size="small" @click="loadSection(barSection)">重试</n-button>
        </n-alert>
        <n-spin v-else :show="loading[barSection]">
          <div class="chart chart-bar"><v-chart v-if="data[barSection]" :option="hourlyOption" autoresize /></div>
        </n-spin>
      </div>
    </div>

    <div class="card chart-card" data-section="heat">
      <div class="card-header"><h3 class="card-title">日历热力图</h3><span class="card-badge mono">近 1 年</span></div>
      <div class="card-body">
        <n-alert v-if="errors.heat" type="error" class="section-error">
          {{ errors.heat }} <n-button size="small" @click="loadSection('heat')">重试</n-button>
        </n-alert>
        <n-spin v-else :show="loading.heat">
          <div class="chart chart-heatmap">
            <v-chart v-if="data.heat?.length" :option="heatmapOption" autoresize />
            <n-empty v-else-if="data.heat" description="暂无用量数据" />
          </div>
        </n-spin>
      </div>
    </div>

    <div class="card table-card" data-section="models">
      <div class="card-header"><h3 class="card-title">模型用量明细</h3><div class="card-badges"><span class="card-badge mono">近 30 天</span><span class="card-badge mono">{{ filteredModels.length }} 条明细</span></div></div>
      <div class="card-body">
        <div class="model-filters">
          <n-select v-model:value="selectedProvider" :options="providerOptions" placeholder="全部供应商" aria-label="供应商筛选" clearable filterable />
          <n-select v-model:value="selectedChannel" :options="channelOptions" placeholder="全部通道" aria-label="通道筛选" clearable />
        </div>
        <n-alert v-if="errors.models" type="error" class="section-error">
          {{ errors.models }} <n-button size="small" @click="loadSection('models')">重试</n-button>
        </n-alert>
        <n-spin v-else :show="loading.models">
          <n-data-table :columns="modelColumns" :data="filteredModels" :row-key="modelRowKey" :bordered="false" :single-line="false" :scroll-x="scrollX" size="small" class="dashboard-table" />
        </n-spin>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted, h } from 'vue'
import { NSpin, NDataTable, NSwitch, NSelect, NAlert, NButton, NEmpty, NTooltip } from 'naive-ui'
import { adminRequest, errorMessage } from '../api'
import { formatLocalHour } from '../utils'
import type { DataTableColumns } from 'naive-ui'
import VChart from 'vue-echarts'
import { use } from 'echarts/core'
import * as echarts from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { BarChart, HeatmapChart } from 'echarts/charts'
import { GridComponent, TooltipComponent, DataZoomComponent, CalendarComponent, VisualMapComponent } from 'echarts/components'
import { isDark } from '../theme'
use([CanvasRenderer, BarChart, HeatmapChart, GridComponent, TooltipComponent, DataZoomComponent, CalendarComponent, VisualMapComponent])

// 图表调色随主题翻转：暗色用 slate 表面 + 同族强调；品牌蓝/青/绿渐变保持。
const CHART = computed(() => isDark.value ? {
  ink: '#F1F5F9', text2: '#CBD5E1', text3: '#64748B',
  border: '#1E293B', divider: '#0F172A', canvas: '#1E293B',
  card: '#0F172A', tooltipBg: '#0F172A',
  blue: '#3B82F6', cyan: '#06B6D4', green: '#22C55E', greenD: '#16A34A',
  ramp: ['#1E293B', '#1D4ED8', '#0891B2', '#22C55E', '#86EFAC'],
  font: 'JetBrains Mono, monospace',
} : {
  ink: '#0F172A', text2: '#475569', text3: '#94A3B8',
  border: '#E2E8F0', divider: '#F1F5F9', canvas: '#F8FAFC',
  card: '#FFFFFF', tooltipBg: '#FFFFFF',
  blue: '#3B82F6', cyan: '#06B6D4', green: '#22C55E', greenD: '#16A34A',
  ramp: ['#F1F5F9', '#BFDBFE', '#3B82F6', '#0D9488', '#14532D'],
  font: 'JetBrains Mono, monospace',
})
interface Overview { total_requests: number; total_input_tokens: number; total_output_tokens: number; avg_latency_ms: number; error_count: number }
interface ModelRow { provider_id: string; channel: string; model_id: string; request_count: number; total_tokens: number; total_input_tokens: number; total_output_tokens: number; cache_read_tokens: number; cache_write_tokens: number; cache_hit_rate: number }
interface DailyRow { date: string; total_tokens: number }
interface HourlyRow { hour: string; total_tokens: number }
const data = reactive<{
  overview: Overview | null
  models: ModelRow[] | null
  hourly: HourlyRow[] | null
  daily: DailyRow[] | null
  heat: DailyRow[] | null
}>({ overview: null, models: null, hourly: null, daily: null, heat: null })
type Section = keyof typeof data
const loading = reactive<Record<Section, boolean>>({ overview: false, models: false, hourly: false, daily: false, heat: false })
const errors = reactive<Record<Section, string>>({ overview: '', models: '', hourly: '', daily: '', heat: '' })
const overview = computed(() => data.overview)
const modelData = computed(() => data.models || [])
const byHour = ref(true)
const barSection = computed(() => byHour.value ? 'hourly' : 'daily')

// 两种粒度独立缓存，由当前选择派生展示；较早请求完成不会覆盖另一种粒度。
const barLabels = computed(() => byHour.value
  ? (data.hourly || []).map(d => formatLocalHour(d.hour))
  : (data.daily || []).slice(-30).map(d => d.date.slice(5)))
const barData = computed(() => byHour.value
  ? (data.hourly || []).map(d => d.total_tokens)
  : (data.daily || []).slice(-30).map(d => d.total_tokens))
const heatData = computed<[string, number][]>(() => (data.heat || []).map(d => [d.date, d.total_tokens]))
const heatMax = computed(() => Math.max(1, ...(data.heat || []).map(d => d.total_tokens)))
const heatRange = computed(() => data.heat?.length ? [data.heat[0].date, data.heat[data.heat.length - 1].date] : [])
const selectedProvider = ref<string | null>(null)
const selectedChannel = ref<string | null>(null)
function channelLabel(channel: string): string {
  return ({ openai_chat: 'OpenAI Chat', openai_responses: 'OpenAI Responses', anthropic: 'Anthropic' } as Record<string, string>)[channel] || channel || '历史未记录'
}
const providerOptions = computed(() => [...new Set(modelData.value.map(r => r.provider_id))].sort().map(id => ({ label: id, value: id })))
const channelOptions = computed(() => [...new Set(modelData.value.map(r => r.channel))].sort().map(channel => ({ label: channelLabel(channel), value: channel })))
const filteredModels = computed(() => modelData.value.filter(row =>
  (selectedProvider.value === null || row.provider_id === selectedProvider.value) &&
  (selectedChannel.value === null || row.channel === selectedChannel.value)))
function modelRowKey(row: ModelRow): string { return JSON.stringify([row.provider_id, row.channel, row.model_id]) }

function buildHourlyOption(p: typeof CHART.value) {
  return {
    backgroundColor: 'transparent',
    tooltip: { trigger: 'axis' as const, backgroundColor: p.tooltipBg, borderColor: p.border, textStyle: { color: p.ink, fontFamily: p.font, fontSize: 12 }, extraCssText: 'border-radius: 8px; box-shadow: 0 1px 2px rgba(15,23,42,0.04);' },
    grid: { left: 85, right: 24, bottom: 90, top: 30, borderColor: p.border },
    xAxis: { type: 'category' as const, data: barLabels.value, axisLabel: { rotate: 45, fontSize: 10, fontFamily: p.font, color: p.text2 }, axisLine: { lineStyle: { color: p.border } }, axisTick: { lineStyle: { color: p.border } } },
    yAxis: { type: 'value' as const, name: 'Token', nameLocation: 'middle' as const, nameGap: 40, nameTextStyle: { color: p.text2, fontFamily: p.font, fontSize: 11 }, axisLabel: { fontFamily: p.font, fontSize: 10, color: p.text2, formatter: (v: number) => formatNum(v) }, splitLine: { lineStyle: { color: p.divider, type: 'dashed' as const } } },
    dataZoom: [{ type: 'slider' as const, start: 0, end: 100, height: 20, bottom: 12, backgroundColor: p.canvas, borderColor: p.border, borderRadius: 8, dataBackground: { lineStyle: { color: p.blue, opacity: 0.15 }, areaStyle: { color: p.blue, opacity: 0.04 } }, selectedDataBackground: { lineStyle: { color: p.blue }, areaStyle: { color: p.blue, opacity: 0.08 } }, handleStyle: { color: p.blue, borderRadius: 4 }, textStyle: { color: p.text2, fontFamily: p.font, fontSize: 10 } }],
    series: [{ name: 'Token 用量', type: 'bar' as const, data: barData.value, barWidth: '60%', itemStyle: { borderRadius: [4, 4, 0, 0], color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [{ offset: 0, color: p.blue }, { offset: 0.52, color: p.cyan }, { offset: 1, color: p.green }]) }, emphasis: { itemStyle: { color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [{ offset: 0, color: '#2563EB' }, { offset: 0.52, color: p.cyan }, { offset: 1, color: p.greenD }]) } } }],
  }
}
function buildHeatmapOption(p: typeof CHART.value) {
  return {
    backgroundColor: 'transparent',
    tooltip: { formatter: (d: any) => `${d.value[0]}<br/>Token: <b>${formatNum(d.value[1] as number)}</b>`, backgroundColor: p.tooltipBg, borderColor: p.border, textStyle: { color: p.ink, fontFamily: p.font, fontSize: 12 }, extraCssText: 'border-radius: 8px; box-shadow: 0 1px 2px rgba(15,23,42,0.04);' },
    visualMap: { min: 0, max: heatMax.value, type: 'continuous' as const, orient: 'vertical' as const, right: 10, top: 'middle', itemWidth: 10, itemHeight: 90, inRange: { color: p.ramp }, textStyle: { color: p.text2, fontFamily: p.font, fontSize: 9 }, formatter: (v: number) => formatNum(v) },
    calendar: { range: heatRange.value, cellSize: [13, 13], left: 36, right: 56, top: 24, bottom: 8, orient: 'horizontal', itemStyle: { borderWidth: 2, borderColor: p.card, color: p.card }, yearLabel: { show: false }, monthLabel: { nameMap: 'cn', color: p.text2, fontFamily: p.font, fontSize: 11, margin: 8 }, dayLabel: { firstDay: 1, nameMap: 'cn', color: p.text3, fontFamily: p.font, fontSize: 10 }, splitLine: { show: false } },
    series: [{ type: 'heatmap' as const, coordinateSystem: 'calendar', data: heatData.value }],
  }
}
const hourlyOption = computed(() => buildHourlyOption(CHART.value))
const heatmapOption = computed(() => buildHeatmapOption(CHART.value))

const providerMap = ref<Record<string, { name: string; icon?: string }>>({})
function iconUrl(icon: string): string { return /^https?:\/\//.test(icon) ? icon : `/icons/${icon}` }
async function loadProviderMap() {
  try {
    const providers: { id: string; name: string; icon?: string }[] = await (await adminRequest('/providers')).json()
    providerMap.value = Object.fromEntries(providers.map(p => [p.id, { name: p.name, icon: p.icon }]))
  } catch { /* 与日志列表一致：供应商信息不可用时回退到字母缩写，不阻塞用量。 */ }
}

function channelMeta(ch: any): { text: string; tier: 'openai' | 'anthropic' } | null {
  if (ch === 'anthropic') return { text: 'anthropic', tier: 'anthropic' }
  if (ch === 'openai_responses') return { text: 'responses', tier: 'openai' }
  if (ch === 'openai_chat') return { text: 'chat', tier: 'openai' }
  return null
}

const modelColumns = computed<DataTableColumns<ModelRow>>(() => [
  { title: '模型', key: 'model_id', width: 260, titleAlign: 'left' as const, render: (row: ModelRow) => {
      const p = providerMap.value[row.provider_id]
      const name = p?.name || row.provider_id
      const iconNode = p?.icon
        ? h('span', { class: 'prov-icon-wrap' }, h('img', { src: iconUrl(p.icon), class: 'prov-icon', alt: '' }))
        : h('span', { class: 'prov-icon-wrap prov-mono-wrap' }, h('span', { class: 'mono prov-mono' }, row.provider_id.slice(0, 2).toUpperCase()))
      return h('div', { class: 'model-cell' }, [
        h(NTooltip, { placement: 'top' }, { trigger: () => iconNode, default: () => name }),
        h(NTooltip, { placement: 'top' }, {
          trigger: () => h('span', { class: 'mono model-id' }, row.model_id || '—'),
          default: () => row.model_id || '—',
        }),
      ])
    } },
  // 通道：独立列，按协议族着色（openai 系青、anthropic 赭），hover 显完整 channel 类型。
  { title: '通道', key: 'channel', width: 100, align: 'center' as const, titleAlign: 'center' as const, render: (row: ModelRow) => {
      const ch = channelMeta(row.channel)
      return ch
        ? h(NTooltip, { placement: 'top' }, {
            trigger: () => h('span', { class: `mono ch-tag ch-${ch.tier}` }, ch.text),
            default: () => row.channel,
          })
        : h('span', { class: 'mono ch-tag ch-na' }, '—')
    } },
  { title: '调用次数', key: 'request_count', width: 110, align: 'right', render: (row) => h('span', { class: 'mono' }, formatNum(row.request_count)) },
  { title: '总 Token', key: 'total_tokens', width: 120, align: 'right', render: (row) => h('span', { class: 'mono token-cell' }, formatNum(row.total_tokens)) },
  { title: '输入', key: 'total_input_tokens', width: 110, align: 'right', render: (row) => h('span', { class: 'mono' }, formatNum(row.total_input_tokens)) },
  { title: '输出', key: 'total_output_tokens', width: 110, align: 'right', render: (row) => h('span', { class: 'mono' }, formatNum(row.total_output_tokens)) },
  { title: '缓存读取', key: 'cache_read_tokens', width: 110, align: 'right', render: (row) => h('span', { class: 'mono' }, formatNum(row.cache_read_tokens)) },
  { title: '缓存写入', key: 'cache_write_tokens', width: 110, align: 'right', render: (row) => h('span', { class: 'mono' }, formatNum(row.cache_write_tokens)) },
  { title: '命中率', key: 'cache_hit_rate', width: 90, align: 'right', render: (row) => h('span', { class: 'mono', style: { color: row.cache_hit_rate > 0 ? CHART.value.greenD : CHART.value.text3, fontWeight: row.cache_hit_rate > 0 ? '600' : '400' } }, `${row.cache_hit_rate}%`) },
])
// 列宽总和：交给 n-data-table 的 scroll-x，窄屏时表格内部横向滚动（表头跟随），
// 不再被 .card 的 overflow:hidden 裁掉右侧列。
const scrollX = computed(() => modelColumns.value.reduce((s, c) => s + (Number((c as any).width) || 0), 0))
function formatNum(n: number): string { if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M'; if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K'; return String(n) }
const endpoints: Record<Section, string> = {
  overview: '/stats/overview',
  hourly: '/stats/hourly',
  daily: '/stats/daily?days=30',
  heat: '/stats/daily?days=365',
  models: '/stats/models'
}
async function loadSection(section: Section) {
  if (loading[section] || data[section] !== null) return
  loading[section] = true
  errors[section] = ''
  try {
    data[section] = await (await adminRequest(endpoints[section])).json()
  } catch (error) {
    errors[section] = errorMessage(error, '加载失败')
  } finally {
    loading[section] = false
  }
}
function onModeChange(value: boolean) {
  byHour.value = value
  void loadSection(barSection.value)
}
onMounted(() => {
  void loadProviderMap()
  for (const section of ['overview', 'hourly', 'heat', 'models'] as const) void loadSection(section)
})
</script>

<style scoped>
.dashboard { display: flex; flex-direction: column; gap: 20px; }
.page-header { margin-bottom: 4px; }
.page-title { font-size: 28px; font-weight: 600; color: var(--mb-text-1); margin: 0; letter-spacing: -0.02em; }
.page-subtitle { margin: 6px 0 0; color: var(--mb-text-3); font-size: 13px; }

.stat-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 16px; }
.stat-card { background: var(--mb-surface); border: 1px solid var(--mb-border); border-radius: 14px; padding: 22px; transition: box-shadow 0.2s, transform 0.2s, background-color 0.2s, border-color 0.2s; }
.stat-card:hover { box-shadow: var(--mb-shadow-2); transform: translateY(-1px); }
.stat-icon { width: 36px; height: 36px; border-radius: 9px; display: flex; align-items: center; justify-content: center; margin-bottom: 12px; }
.stat-icon svg { width: 20px; height: 20px; }
.stat-icon.ico-g { background: var(--mb-tint-blue); color: var(--mb-primary); }
.stat-icon.ico-a { background: var(--mb-tint-cyan); color: var(--mb-cyan); }
.stat-icon.ico-t { background: var(--mb-tint-green); color: var(--mb-success-d); }
.stat-icon.ico-l { background: var(--mb-tint-amber); color: var(--mb-warning); }
.stat-label { font-size: 12px; font-weight: 500; color: var(--mb-text-3); text-transform: uppercase; letter-spacing: 0.05em; margin-bottom: 6px; }
.stat-value { font-size: 28px; font-weight: 600; color: var(--mb-text-1); line-height: 1; }
.stat-unit { font-size: 16px; color: var(--mb-text-3); font-weight: 400; margin-left: 3px; }

.card { background: var(--mb-surface); border: 1px solid var(--mb-border); border-radius: 14px; overflow: hidden; transition: background-color 0.2s, border-color 0.2s; }
.card-header { display: flex; flex-wrap: wrap; gap: 12px; justify-content: space-between; align-items: center; padding: 18px 24px; border-bottom: 1px solid var(--mb-divider); }
.chart-header-right { display: flex; align-items: center; gap: 12px; }
.card-title { margin: 0; font-family: 'Inter', sans-serif; font-size: 17px; font-weight: 600; color: var(--mb-text-1); letter-spacing: -0.01em; }
.card-badge { font-size: 11px; color: var(--mb-text-2); padding: 4px 10px; background: var(--mb-surface-inset); border: 1px solid var(--mb-border); border-radius: 999px; }
.card-badges { display: flex; align-items: center; gap: 8px; }
.card-body { padding: 24px; }
.model-filters { display: flex; flex-wrap: wrap; gap: 12px; margin-bottom: 16px; }
.model-filters > * { flex: 1 1 180px; max-width: 280px; }
.section-error :deep(.n-button) { margin-left: 8px; }

.chart { height: 320px; }
.chart-bar { height: 380px; }
.chart-heatmap { height: 170px; }
.dashboard-table { --n-td-color: var(--mb-surface); --n-th-color: var(--mb-surface-inset); }
/* column.render 产生的 VNode 不带本组件 scoped id，plain scoped 命中不到，必须 :deep（同 Logs.vue） */
/* 模型：供应商图标 + model id（省略号）。flex 行，图标固定不缩、id 收缩省略。 */
.dashboard :deep(.model-cell) { display: flex; align-items: center; justify-content: flex-start; gap: 7px; min-width: 0; max-width: 100%; }
.dashboard :deep(.model-id) { font-size: 12px; color: var(--mb-text-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }

/* 供应商图标：并入「模型」列前缀，缩小为 18px 内联贴片。 */
.dashboard :deep(.prov-icon-wrap) { display: inline-flex; align-items: center; justify-content: center; width: 18px; height: 18px; flex-shrink: 0; border-radius: 5px; background: var(--mb-surface-inset); border: 1px solid var(--mb-divider); transition: border-color 0.15s, background 0.15s; }
.dashboard :deep(.prov-icon) { width: 13px; height: 13px; object-fit: contain; display: block; }
.dashboard :deep(.prov-mono-wrap) { background: var(--mb-surface-inset); }
.dashboard :deep(.prov-mono) { font-size: 8px; font-weight: 600; color: var(--mb-cyan); letter-spacing: 0.02em; }
.dashboard :deep(.n-data-table-tr:hover .prov-icon-wrap) { border-color: var(--mb-tint-green); background: var(--mb-surface); }

/* 通道标签：按协议族着色（openai 系青、anthropic 赭），hover 显完整 channel 类型。 */
.dashboard :deep(.ch-tag) { font-size: 10px; font-weight: 500; padding: 1px 7px; border-radius: 999px; letter-spacing: 0.02em; white-space: nowrap; line-height: 1.4; }
.dashboard :deep(.ch-tag.ch-openai) { color: var(--mb-cyan); background: rgba(6,182,212,0.08); border: 1px solid rgba(6,182,212,0.18); }
.dashboard :deep(.ch-tag.ch-anthropic) { color: var(--mb-warning); background: rgba(245,158,11,0.08); border: 1px solid rgba(245,158,11,0.18); }
.dashboard :deep(.ch-tag.ch-na) { color: var(--mb-text-3); }
.dashboard :deep(.token-cell) { color: var(--mb-success-d); font-weight: 500; }

@media (max-width: 900px) { .stat-grid { grid-template-columns: repeat(2, 1fr); } }
@media (max-width: 600px) { .stat-grid { grid-template-columns: 1fr; } .content { padding: 16px; } }
</style>