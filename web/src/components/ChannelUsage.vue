<template>
  <section class="usage-panel" aria-label="最近7天通道用量">
    <div class="usage-heading">
      <div><h2>最近 7 天用量</h2><p>{{ channelLabel }} · 按客户端展示 · 模型分组 · 客户端不区分版本 · UTC</p></div>
      <div class="usage-totals">
        <div><span>请求总数</span><strong class="mono" :title="totals.requests.toLocaleString()">{{ formatUsage(totals.requests) }}</strong></div>
        <div><span>Token 总量</span><strong class="mono" :title="totals.tokens.toLocaleString()">{{ formatUsage(totals.tokens) }}</strong></div>
      </div>
    </div>
    <n-alert v-if="error" type="error" class="usage-error">{{ error }}</n-alert>
    <n-spin :show="loading">
      <div class="usage-charts">
        <div class="usage-chart"><h3>请求数量</h3><v-chart ref="requestChart" :option="chartOption('request_count')" autoresize @mouseover="showModelTooltip(requestChart, 'request_count', $event)" @mouseout="restoreClientTooltip(requestChart, 'request_count', $event)" /></div>
        <div class="usage-chart"><h3>Token 用量</h3><v-chart ref="tokenChart" :option="chartOption('tokens')" autoresize @mouseover="showModelTooltip(tokenChart, 'tokens', $event)" @mouseout="restoreClientTooltip(tokenChart, 'tokens', $event)" /></div>
      </div>
      <div class="detail-heading"><div><h3>用量明细 <span class="detail-count mono">{{ groups.length }}</span></h3></div><span class="detail-period">最近 7 天</span></div>
      <n-data-table class="usage-table" :columns="columns" :data="groups" :row-key="rowKey" :bordered="false" :scroll-x="1000" :pagination="{ pageSize: 10 }" size="small">
        <template #empty>最近 7 天该通道暂无请求记录</template>
      </n-data-table>
    </n-spin>
    <div class="usage-note">Token 为输入与输出之和；请求数包含已记录的失败请求。历史未记录通道的请求不计入当前通道。</div>
  </section>
</template>

<script setup lang="ts">
import { computed, h, ref, watch } from 'vue'
import { NAlert, NDataTable, NSpin } from 'naive-ui'
import type { DataTableColumns } from 'naive-ui'
import VChart from 'vue-echarts'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { CustomChart } from 'echarts/charts'
import { GridComponent, TooltipComponent, LegendComponent } from 'echarts/components'
import { adminRequest, errorMessage } from '../api'
import { usageSeries } from '../utils/usageSeries'
import { centeredBar } from '../utils/centeredBars'
import { isDark } from '../theme'

use([CanvasRenderer, CustomChart, GridComponent, TooltipComponent, LegendComponent])
interface UsageRow { date: string; channel: string; client: string; model_id: string; provider_id: string; request_count: number; input_tokens: number; output_tokens: number }
interface Group { client: string; model_id: string; provider_id: string; request_count: number; input_tokens: number; output_tokens: number }
const props = defineProps<{ channel: string; channelLabel: string; refreshKey: number; providers: Record<string, { name: string; icon?: string | null }> }>()
const data = ref<{ dates: string[]; rows: UsageRow[] }>({ dates: [], rows: [] })
let requestId = 0
const loading = ref(false)
const error = ref('')
const requestChart = ref<InstanceType<typeof VChart>>()
const tokenChart = ref<InstanceType<typeof VChart>>()
const rows = computed(() => data.value.rows.filter(row => row.channel === props.channel))
function rowKey(row: Group): string { return JSON.stringify([row.client, row.model_id, row.provider_id]) }
function tokens(row: Group | UsageRow): number { return row.input_tokens + row.output_tokens }
const groups = computed(() => {
  const map = new Map<string, Group>()
  for (const row of rows.value) {
    const key = rowKey(row)
    const group = map.get(key) || { client: row.client, model_id: row.model_id, provider_id: row.provider_id, request_count: 0, input_tokens: 0, output_tokens: 0 }
    group.request_count += row.request_count
    group.input_tokens += row.input_tokens
    group.output_tokens += row.output_tokens
    map.set(key, group)
  }
  return [...map.values()].sort((a, b) => b.request_count - a.request_count || rowKey(a).localeCompare(rowKey(b)))
})
const totals = computed(() => groups.value.reduce((sum, row) => ({ requests: sum.requests + row.request_count, tokens: sum.tokens + tokens(row) }), { requests: 0, tokens: 0 }))
const chartGroups = computed(() => usageSeries(rows.value, data.value.dates))
function iconUrl(icon: string): string { return /^https?:\/\//.test(icon) ? icon : `/icons/${icon}` }
function metric(value: number, primary = false) { return h('span', { class: ['metric mono', { 'metric-primary': primary }] }, value.toLocaleString()) }
const columns: DataTableColumns<Group> = [
  { title: '客户端', key: 'client', width: 170, render: row => h('div', { class: 'client-cell' }, [
    h('span', { class: 'client-mark', 'aria-hidden': 'true' }, (row.client || '?').charAt(0).toUpperCase()),
    h('span', { class: 'client-label' }, row.client || '未知客户端')
  ]) },
  { title: '模型', key: 'model_id', width: 230, render: row => h('code', { class: 'detail-model mono' }, row.model_id) },
  { title: '供应商', key: 'provider_id', width: 145, render: row => {
    const provider = props.providers[row.provider_id]
    const name = provider?.name || row.provider_id
    return h('div', { class: 'detail-provider' }, [
      h('span', { class: 'detail-provider-mark', 'aria-hidden': 'true' }, provider?.icon
        ? h('img', { src: iconUrl(provider.icon), alt: '' })
        : name.charAt(0).toUpperCase()),
      h('span', {}, name)
    ])
  } },
  { title: '请求数', key: 'request_count', width: 85, align: 'right', render: row => metric(row.request_count) },
  { title: '总 Token', key: 'tokens', width: 130, align: 'right', render: row => metric(tokens(row), true) },
  { title: '输入 Token', key: 'input_tokens', width: 130, align: 'right', render: row => metric(row.input_tokens) },
  { title: '输出 Token', key: 'output_tokens', width: 110, align: 'right', render: row => metric(row.output_tokens) }
]

type UsageMetric = 'request_count' | 'tokens'
const hoveredModels: Partial<Record<UsageMetric, number>> = {}
function refreshTooltip(chart: InstanceType<typeof VChart> | undefined, event: any) {
  // 清除同一类目的提示缓存，让移入或移出柱子时立即重新生成内容。
  chart?.dispatchAction({ type: 'hideTip' })
  chart?.dispatchAction({ type: 'showTip', x: event.event.offsetX, y: event.event.offsetY })
}
function showModelTooltip(chart: InstanceType<typeof VChart> | undefined, metric: UsageMetric, event: any) {
  if (event.componentType !== 'series') return
  hoveredModels[metric] = event.seriesIndex
  refreshTooltip(chart, event)
}
function restoreClientTooltip(chart: InstanceType<typeof VChart> | undefined, metric: UsageMetric, event: any) {
  delete hoveredModels[metric]
  refreshTooltip(chart, event)
}

function formatUsage(value: number): string { return Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 2 }).format(value) }
function tooltipValue(param: any): number { return Number(Array.isArray(param.value) ? param.value[1] : param.value) }

function chartOption(metric: UsageMetric) {
  delete hoveredModels[metric]
  const text = isDark.value ? '#CBD5E1' : '#475569'
  const border = isDark.value ? '#334155' : '#E2E8F0'
  const models = chartGroups.value.series
  const clients = chartGroups.value.clients.map(client => client || '未知客户端')
  return {
    color: ['#3B82F6', '#06B6D4', '#F59E0B', '#22C55E', '#8B5CF6', '#F43F5E', '#14B8A6'],
    textStyle: { color: text },
    tooltip: { trigger: 'axis', renderMode: 'richText', confine: true, backgroundColor: isDark.value ? '#0F172A' : '#FFFFFF', borderColor: border, textStyle: { color: text }, formatter: (params: any[]) => {
      const hovered = hoveredModels[metric]
      return params.filter(p => tooltipValue(p) > 0 && (hovered === undefined || p.seriesIndex === hovered)).map(p => `${p.marker}${p.seriesName}: ${formatUsage(tooltipValue(p))}`).join('\n')
    } },
    legend: { type: 'scroll', bottom: 0, data: models.map(row => row.model_id), textStyle: { color: text, width: 230, overflow: 'truncate' }, pageTextStyle: { color: text } },
    grid: { left: 16, right: 18, top: 30, bottom: 75, containLabel: true },
    xAxis: { type: 'category', data: clients, axisLabel: { color: text, fontSize: 10, interval: 0, rotate: clients.length > 3 ? 25 : 0, width: 110, overflow: 'truncate' }, axisLine: { lineStyle: { color: border } }, axisTick: { show: false } },
    yAxis: { type: 'value', name: metric === 'tokens' ? 'Token' : '请求数', minInterval: 1, axisLabel: { color: text, formatter: formatUsage }, splitLine: { lineStyle: { color: border, type: 'dashed' } } },
    series: models.map((row, seriesIndex) => ({
      name: row.model_id,
      type: 'custom',
      dimensions: ['client', 'value'],
      encode: { x: 'client', y: 'value' },
      emphasis: { focus: 'series' },
      data: (metric === 'tokens' ? row.tokens : row.requests).map((value, index) => [index, value]),
      renderItem: (_params: any, api: any) => {
        const client = api.value(0)
        const value = api.value(1)
        if (value <= 0) return
        // 仅为当前客户端有用量且图例可见的模型分配位置，避免空柱位把整组推偏。
        const active = api.currentSeriesIndices().filter((index: number) => (metric === 'tokens' ? models[index].tokens : models[index].requests)[client] > 0)
        const bar = centeredBar(seriesIndex, active, api.size([1, 0])[0])
        if (!bar) return
        const point = api.coord([client, value])
        const baseline = api.coord([client, 0])[1]
        return { type: 'rect', shape: { x: point[0] + bar.offset, y: point[1], width: bar.width, height: baseline - point[1] }, style: { fill: api.visual('color') }, emphasis: { style: { opacity: 0.85 } } }
      }
    }))
  }
}
async function load() {
  const id = ++requestId
  loading.value = true
  error.value = ''
  try {
    const result = await (await adminRequest('/stats/channels')).json()
    if (id === requestId) data.value = result
  } catch (cause) {
    if (id === requestId) error.value = errorMessage(cause, '加载通道用量失败')
  } finally {
    if (id === requestId) loading.value = false
  }
}
watch(() => props.refreshKey, load, { immediate: true })
</script>

<style scoped>
.usage-panel { margin-top: 20px; background: var(--mb-surface); border: 1px solid var(--mb-border); border-radius: 14px; overflow: hidden; }
.usage-heading { display: flex; justify-content: space-between; align-items: center; flex-wrap: wrap; gap: 12px; padding: 22px 24px 10px; }
.usage-heading h2 { margin: 0; font-size: 15px; color: var(--mb-text-1); }
.usage-heading p { font-size: 11px; color: var(--mb-text-3); }
.usage-heading p { margin: 7px 0 0; }
.usage-error { margin: 12px 24px; }
.usage-charts { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 16px; padding: 16px 24px 24px; }
.usage-chart { min-width: 0; }
.usage-chart h3 { font-size: 12px; font-weight: 500; color: var(--mb-text-2); margin: 0 0 8px; }
.usage-chart .echarts { height: 300px; }
.usage-totals { display: flex; gap: 24px; }
.usage-totals > div { display: flex; flex-direction: column; align-items: flex-end; gap: 5px; }
.usage-totals span { font-size: 10px; color: var(--mb-text-3); }
.usage-totals strong { font-size: 18px; font-weight: 500; color: var(--mb-text-1); font-variant-numeric: tabular-nums; }
.detail-heading { display: flex; justify-content: space-between; align-items: center; gap: 12px; padding: 16px 20px; border-top: 1px solid var(--mb-divider); }
.detail-heading h3 { display: flex; align-items: center; gap: 8px; margin: 0; color: var(--mb-text-1); font-size: 13px; font-weight: 600; }
.detail-count { padding: 2px 7px; background: var(--mb-surface-inset); border: 1px solid var(--mb-border); border-radius: 5px; font-size: 10px; color: var(--mb-text-3); }
.detail-period { font-size: 10px; color: var(--mb-text-3); white-space: nowrap; }
.usage-table :deep(.n-data-table-th) { padding: 10px 12px; font-size: 11px; font-weight: 600; color: var(--mb-text-2); background: var(--mb-surface); border-color: var(--mb-divider); }
.usage-table :deep(.n-data-table-td) { padding: 9px 12px; border-color: var(--mb-divider); }
.usage-table :deep(.n-data-table-tr:hover .n-data-table-td) { background: var(--mb-surface-inset); }
.usage-table :deep(.n-data-table-tr:hover .detail-provider-mark) { border-color: var(--mb-tint-green); background: var(--mb-surface); }
:deep(.client-cell) { display: flex; align-items: center; gap: 7px; }
:deep(.client-mark) { display: inline-flex; align-items: center; justify-content: center; flex-shrink: 0; width: 20px; height: 20px; border-radius: 5px; color: var(--mb-cyan); background: var(--mb-surface-inset); font-size: 11px; font-weight: 600; }
:deep(.client-label) { font-size: 12px; color: var(--mb-text-2); overflow-wrap: anywhere; }
:deep(.detail-model) { font-size: 12px; color: var(--mb-text-2); overflow-wrap: anywhere; }
:deep(.detail-provider) { display: flex; align-items: center; gap: 7px; font-size: 12px; color: var(--mb-text-2); overflow-wrap: anywhere; }
:deep(.detail-provider-mark) { display: inline-flex; align-items: center; justify-content: center; width: 18px; height: 18px; flex-shrink: 0; border: 1px solid var(--mb-divider); background: var(--mb-surface-inset); border-radius: 5px; font-size: 10px; }
:deep(.detail-provider-mark img) { width: 13px; height: 13px; object-fit: contain; }
:deep(.metric) { font-size: 12px; color: var(--mb-text-2); font-variant-numeric: tabular-nums; white-space: nowrap; }
:deep(.metric-primary) { color: var(--mb-success-d); font-weight: 500; }
.usage-panel :deep(.n-data-table__pagination) { padding: 0 20px 12px; }
.usage-note { padding: 14px 24px; font-size: 10px; line-height: 1.7; color: var(--mb-text-3); border-top: 1px solid var(--mb-divider); }
@media (max-width: 800px) { .usage-charts { grid-template-columns: 1fr; padding: 16px; } .usage-heading { padding: 18px 16px 8px; } .usage-totals { gap: 18px; } .usage-totals strong { font-size: 15px; } .detail-heading { padding: 18px 16px 14px; } .usage-note { padding: 12px 16px; } }
</style>
