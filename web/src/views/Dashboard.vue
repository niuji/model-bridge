<template>
  <div class="dashboard">
    <div class="page-header">
      <h1 class="page-title serif">仪表盘</h1>
      <p class="page-subtitle mono">最近 7 天用量统计</p>
    </div>

    <div class="stat-grid">
      <div class="stat-card">
        <div class="stat-icon ico-g">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="22,12 18,12 15,21 9,3 6,12 2,12" /></svg>
        </div>
        <div class="stat-label">总请求数</div>
        <div class="stat-value mono">{{ formatNum(overview.total_requests) }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-icon ico-a">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="15,14 20,9 15,4" /><path d="M4 20v-7a4 4 0 0 1 4-4h12" /></svg>
        </div>
        <div class="stat-label">输入 Token</div>
        <div class="stat-value mono">{{ formatNum(overview.total_input_tokens) }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-icon ico-t">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="9,10 4,15 9,20" /><path d="M20 4v7a4 4 0 0 1-4 4H4" /></svg>
        </div>
        <div class="stat-label">输出 Token</div>
        <div class="stat-value mono">{{ formatNum(overview.total_output_tokens) }}</div>
      </div>
      <div class="stat-card">
        <div class="stat-icon ico-g">
          <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10" /><polyline points="12,6 12,12 16,14" /></svg>
        </div>
        <div class="stat-label">平均延迟</div>
        <div class="stat-value mono">{{ Math.round(overview.avg_latency_ms) }}<span class="stat-unit">ms</span></div>
      </div>
    </div>

    <div class="card chart-card">
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
      <div class="card-body"><n-spin :show="loading"><v-chart class="chart chart-bar" :option="hourlyOption" autoresize /></n-spin></div>
    </div>

    <div class="card chart-card">
      <div class="card-header"><h3 class="card-title">日历热力图</h3><span class="card-badge mono">近 1 年</span></div>
      <div class="card-body"><n-spin :show="loading"><v-chart class="chart chart-heatmap" :option="heatmapOption" autoresize /></n-spin></div>
    </div>

    <div class="card table-card">
      <div class="card-header"><h3 class="card-title">模型用量明细</h3><span class="card-badge mono">{{ modelData.length }} 个模型</span></div>
      <div class="card-body"><n-spin :show="loading"><n-data-table :columns="modelColumns" :data="modelData" :bordered="false" :single-line="false" size="small" class="dashboard-table" /></n-spin></div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, h } from 'vue'
import { NSpin, NDataTable, NSwitch } from 'naive-ui'
import { formatLocalHour } from '../utils'
import type { DataTableColumns } from 'naive-ui'
import VChart from 'vue-echarts'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { BarChart, HeatmapChart } from 'echarts/charts'
import { GridComponent, TooltipComponent, DataZoomComponent, CalendarComponent, VisualMapComponent } from 'echarts/components'
use([CanvasRenderer, BarChart, HeatmapChart, GridComponent, TooltipComponent, DataZoomComponent, CalendarComponent, VisualMapComponent])

const API_BASE = '/api/admin'; const loading = ref(true)
const overview = ref({ total_requests: 0, total_input_tokens: 0, total_output_tokens: 0, avg_latency_ms: 0, error_count: 0 })
interface ModelRow { model_id: string; request_count: number; total_tokens: number; total_input_tokens: number; total_output_tokens: number; cache_read_tokens: number; cache_write_tokens: number; cache_hit_rate: number }
const modelData = ref<ModelRow[]>([])

const hourlyOption = ref({
  backgroundColor: 'transparent',
  tooltip: { trigger: 'axis' as const, backgroundColor: '#ffffff', borderColor: '#d9cfbf', textStyle: { color: '#17140f', fontFamily: 'IBM Plex Mono, monospace', fontSize: 12 }, extraCssText: 'border-radius: 8px; box-shadow: 0 4px 16px rgba(23,20,15,0.06);' },
  grid: { left: 85, right: 24, bottom: 90, top: 30, borderColor: '#d9cfbf' },
  xAxis: { type: 'category' as const, data: [] as string[], axisLabel: { rotate: 45, fontSize: 10, fontFamily: 'IBM Plex Mono, monospace', color: '#74695a' }, axisLine: { lineStyle: { color: '#d9cfbf' } }, axisTick: { lineStyle: { color: '#d9cfbf' } } },
  yAxis: { type: 'value' as const, name: 'Token', nameLocation: 'middle' as const, nameGap: 40, nameTextStyle: { color: '#74695a', fontFamily: 'IBM Plex Mono, monospace', fontSize: 11 }, axisLabel: { fontFamily: 'IBM Plex Mono, monospace', fontSize: 10, color: '#74695a', formatter: (v: number) => formatNum(v) }, splitLine: { lineStyle: { color: '#ece6da', type: 'dashed' as const } } },
  dataZoom: [{ type: 'slider' as const, start: 0, end: 100, height: 20, bottom: 12, backgroundColor: '#f4efe3', borderColor: '#d9cfbf', borderRadius: 8, dataBackground: { lineStyle: { color: '#2ea86a', opacity: 0.15 }, areaStyle: { color: '#2ea86a', opacity: 0.04 } }, selectedDataBackground: { lineStyle: { color: '#2ea86a' }, areaStyle: { color: '#2ea86a', opacity: 0.08 } }, handleStyle: { color: '#2ea86a', borderRadius: 4 }, textStyle: { color: '#74695a', fontFamily: 'IBM Plex Mono, monospace', fontSize: 10 } }],
  series: [{ name: 'Token 用量', type: 'bar' as const, data: [] as number[], barWidth: '60%', itemStyle: { borderRadius: [4, 4, 0, 0], color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [{ offset: 0, color: '#2ea86a' }, { offset: 1, color: '#8dd8ae' }]) }, emphasis: { itemStyle: { color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [{ offset: 0, color: '#1d7a4c' }, { offset: 1, color: '#2ea86a' }]) } } }],
})

const heatmapOption = ref({
  backgroundColor: 'transparent',
  tooltip: { formatter: (p: any) => `${p.value[0]}<br/>Token: <b>${formatNum(p.value[1] as number)}</b>`, backgroundColor: '#ffffff', borderColor: '#d9cfbf', textStyle: { color: '#17140f', fontFamily: 'IBM Plex Mono, monospace', fontSize: 12 }, extraCssText: 'border-radius: 8px; box-shadow: 0 4px 16px rgba(23,20,15,0.06);' },
  visualMap: { min: 0, max: 1, type: 'continuous' as const, orient: 'vertical' as const, right: 10, top: 'middle', itemWidth: 10, itemHeight: 90, inRange: { color: ['#ece6da', '#bfe6cf', '#5bbf8a', '#2ea86a', '#1d7a4c'] }, textStyle: { color: '#74695a', fontFamily: 'IBM Plex Mono, monospace', fontSize: 9 } },
  calendar: { range: ['2026-01-01', '2026-01-31'], cellSize: [13, 13], left: 36, right: 56, top: 24, bottom: 8, orient: 'horizontal', itemStyle: { borderWidth: 2, borderColor: '#fff', color: '#faf7f0' }, yearLabel: { show: false }, monthLabel: { nameMap: 'cn', color: '#74695a', fontFamily: 'IBM Plex Mono, monospace', fontSize: 11, margin: 8 }, dayLabel: { firstDay: 1, nameMap: 'cn', color: '#a89e8c', fontFamily: 'IBM Plex Mono, monospace', fontSize: 10 }, splitLine: { show: false } },
  series: [{ type: 'heatmap' as const, coordinateSystem: 'calendar', data: [] as any[] }],
})
import * as echarts from 'echarts/core'

const modelColumns: DataTableColumns<ModelRow> = [
  { title: '模型', key: 'model_id', width: 260, ellipsis: { tooltip: true }, render: (row) => h('span', { class: 'mono model-id-cell' }, row.model_id) },
  { title: '调用次数', key: 'request_count', width: 110, align: 'right', render: (row) => h('span', { class: 'mono' }, formatNum(row.request_count)) },
  { title: '总 Token', key: 'total_tokens', width: 120, align: 'right', render: (row) => h('span', { class: 'mono token-cell' }, formatNum(row.total_tokens)) },
  { title: '输入', key: 'total_input_tokens', width: 110, align: 'right', render: (row) => h('span', { class: 'mono' }, formatNum(row.total_input_tokens)) },
  { title: '输出', key: 'total_output_tokens', width: 110, align: 'right', render: (row) => h('span', { class: 'mono' }, formatNum(row.total_output_tokens)) },
  { title: '缓存读取', key: 'cache_read_tokens', width: 110, align: 'right', render: (row) => h('span', { class: 'mono' }, formatNum(row.cache_read_tokens)) },
  { title: '缓存写入', key: 'cache_write_tokens', width: 110, align: 'right', render: (row) => h('span', { class: 'mono' }, formatNum(row.cache_write_tokens)) },
  { title: '命中率', key: 'cache_hit_rate', width: 90, align: 'right', render: (row) => h('span', { class: 'mono', style: { color: row.cache_hit_rate > 0 ? '#1d7a4c' : '#a89e8c', fontWeight: row.cache_hit_rate > 0 ? '600' : '400' } }, `${row.cache_hit_rate}%`) },
]
function formatNum(n: number): string { if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M'; if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K'; return String(n) }
const byHour = ref(true)

async function loadData() {
  loading.value = true
  try {
    const dailyRes = await fetch(`${API_BASE}/stats/daily?days=365`).then(r => r.json())
    const [overviewRes, barRes, modelsRes] = await Promise.all([
      fetch(`${API_BASE}/stats/overview`).then(r => r.json()),
      byHour.value
        ? fetch(`${API_BASE}/stats/hourly`).then(r => r.json())
        : Promise.resolve(dailyRes),
      fetch(`${API_BASE}/stats/models`).then(r => r.json()),
    ])
    overview.value = overviewRes
    modelData.value = modelsRes
    const labels = byHour.value
      ? barRes.map((d: any) => formatLocalHour(d.hour))
      : barRes.map((d: any) => d.date.slice(5))
    const barData = barRes.map((d: any) => d.total_tokens)
    hourlyOption.value = { ...hourlyOption.value, xAxis: { ...hourlyOption.value.xAxis, data: labels } as any, series: [{ ...hourlyOption.value.series[0], data: barData }] }
    const heatData = dailyRes.map((d: any) => [d.date, d.total_tokens])
    const heatMax = Math.max(1, ...dailyRes.map((d: any) => d.total_tokens as number))
    const heatRange = dailyRes.length ? [dailyRes[0].date, dailyRes[dailyRes.length - 1].date] : ['', '']
    heatmapOption.value = { ...heatmapOption.value, visualMap: { ...heatmapOption.value.visualMap, max: heatMax }, calendar: { ...heatmapOption.value.calendar, range: heatRange }, series: [{ ...heatmapOption.value.series[0], data: heatData }] }
  } catch (e) { console.error('加载仪表盘数据失败:', e) } finally { loading.value = false }
}
function onModeChange(v: boolean) { byHour.value = v; loadData() }
onMounted(loadData)
</script>

<style scoped>
.dashboard { display: flex; flex-direction: column; gap: 20px; }
.page-header { margin-bottom: 4px; }
.page-title { font-size: 28px; font-weight: 600; color: #17140f; margin: 0; letter-spacing: -0.02em; }
.page-subtitle { margin: 6px 0 0; color: #a89e8c; font-size: 13px; }

.stat-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 16px; }
.stat-card { background: #ffffff; border: 1px solid #d9cfbf; border-radius: 12px; padding: 22px; transition: box-shadow 0.2s, transform 0.2s; }
.stat-card:hover { box-shadow: 0 6px 22px rgba(23,20,15,0.06); transform: translateY(-1px); }
.stat-icon { width: 36px; height: 36px; border-radius: 9px; display: flex; align-items: center; justify-content: center; margin-bottom: 12px; }
.stat-icon svg { width: 20px; height: 20px; }
.stat-icon.ico-g { background: #bfe6cf; color: #1d7a4c; }
.stat-icon.ico-a { background: #f6e6b6; color: #9c6c00; }
.stat-icon.ico-t { background: #c2e8e6; color: #0d6e6b; }
.stat-label { font-size: 12px; font-weight: 500; color: #a89e8c; text-transform: uppercase; letter-spacing: 0.05em; margin-bottom: 6px; }
.stat-value { font-size: 28px; font-weight: 600; color: #17140f; line-height: 1; }
.stat-unit { font-size: 16px; color: #a89e8c; font-weight: 400; margin-left: 3px; }

.card { background: #ffffff; border: 1px solid #d9cfbf; border-radius: 12px; overflow: hidden; }
.card-header { display: flex; justify-content: space-between; align-items: center; padding: 18px 24px; border-bottom: 1px solid #ece6da; }
.chart-header-right { display: flex; align-items: center; gap: 12px; }
.card-title { margin: 0; font-family: 'Fraunces', 'Georgia', serif; font-size: 17px; font-weight: 600; color: #17140f; letter-spacing: -0.01em; }
.card-badge { font-size: 11px; color: #74695a; padding: 4px 10px; background: #f4efe3; border: 1px solid #d9cfbf; border-radius: 999px; }
.card-body { padding: 24px; }

.chart { height: 320px; }
.chart-bar { height: 380px; }
.chart-heatmap { height: 170px; }
.dashboard-table { --n-td-color: #ffffff; --n-th-color: #f4efe3; }
/* column.render 产生的 VNode 不带本组件 scoped id，plain scoped 命中不到，必须 :deep（同 Logs.vue） */
.dashboard :deep(.model-id-cell) { font-size: 12px; color: #74695a; }
.dashboard :deep(.token-cell) { color: #1d7a4c; font-weight: 500; }

@media (max-width: 900px) { .stat-grid { grid-template-columns: repeat(2, 1fr); } }
@media (max-width: 600px) { .stat-grid { grid-template-columns: 1fr; } .content { padding: 16px; } }
</style>