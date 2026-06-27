<template>
  <div class="logs">
    <div class="page-header">
      <div>
        <h1 class="page-title serif">日志</h1>
        <p class="page-subtitle mono">请求记录</p>
      </div>
      <div class="count-block">
        <span class="count-label mono">记录总数</span>
        <span class="count-num mono">{{ total.toLocaleString('en-US') }}</span>
      </div>
    </div>

    <div class="table-container">
      <div v-if="loading && !logs.length" class="skeleton">
        <div class="sk-row" v-for="r in 9" :key="r" :style="{ '--sk-d': (r * 0.07) + 's' }">
          <span class="sk-bar w-time" />
          <span class="sk-bar w-key" />
          <span class="sk-bar w-model" />
          <span class="sk-bar w-prov" />
          <span class="sk-bar w-tok" />
          <span class="sk-bar w-lat" />
          <span class="sk-bar w-st" />
          <span class="sk-bar w-err" />
        </div>
      </div>
      <n-data-table
        v-else-if="logs.length"
        :columns="columns"
        :data="logs"
        :bordered="false"
        :loading="loading"
        size="small"
        class="logs-table"
      />
      <div v-else class="empty-state">
        <div class="empty-icon">
          <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
            <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
            <polyline points="14 2 14 8 20 8" />
            <line x1="16" y1="13" x2="8" y2="13" />
            <line x1="16" y1="17" x2="8" y2="17" />
          </svg>
        </div>
        <p class="empty-text serif">暂无请求记录</p>
        <p class="empty-hint mono">当有请求通过桥接代理时，日志将自动记录</p>
      </div>
    </div>

    <footer v-if="total > 0" class="ledger-footer">
      <span class="range-text mono">{{ `显示 ${rangeStart.toLocaleString('en-US')}–${rangeEnd.toLocaleString('en-US')} 条` }}</span>
      <n-pagination v-model:page="page" :page-size="pageSize" :item-count="total" @update:page="loadLogs" />
    </footer>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, h } from 'vue'
import { NDataTable, NPagination, NTooltip } from 'naive-ui'
import { formatLocalTime } from '../utils'

const API_BASE = '/api/admin'
const logs = ref<any[]>([])
const loading = ref(true)
const page = ref(1)
const pageSize = 50
const total = ref(0)

// 供应商 id -> {name, icon} 查表：启动时拉一次，用于把 provider_id 渲染成图标。
const providerMap = ref<Record<string, { name: string; icon?: string }>>({})

// 延迟条的相对刻度：以当前页最大延迟为 100%，让一页内的延迟分布一眼可读。
const maxLatency = computed(() => {
  let m = 0
  for (const r of logs.value) if (r.latency_ms != null && r.latency_ms > m) m = r.latency_ms
  return m
})
const rangeStart = computed(() => (total.value === 0 ? 0 : (page.value - 1) * pageSize + 1))
const rangeEnd = computed(() => Math.min(page.value * pageSize, total.value))

// 复用 Providers.vue 的图标解析：http(s) 直链，否则走 /icons/<file>。
function iconUrl(icon: string): string { return /^https?:\/\//.test(icon) ? icon : `/icons/${icon}` }

function fmtToken(n: any): string {
  if (n == null) return '—'
  const v = Number(n)
  return isNaN(v) ? '—' : v.toLocaleString('en-US')
}

// 延迟分级配色复用主题语义色：fast=success 绿、mid=warning 赭、slow=error 红。
function latencyTier(ms: any): 'fast' | 'mid' | 'slow' | 'na' {
  if (ms == null) return 'na'
  const v = Number(ms)
  if (isNaN(v)) return 'na'
  if (v <= 1500) return 'fast'
  if (v <= 4500) return 'mid'
  return 'slow'
}
function humanizeLatency(ms: any): string {
  if (ms == null) return '—'
  const v = Number(ms)
  if (isNaN(v)) return '—'
  if (v < 1000) return `${v}ms`
  return `${(v / 1000).toFixed(v < 10000 ? 1 : 0)}s`
}
function latencyPct(ms: any): number {
  if (ms == null || maxLatency.value <= 0) return 0
  const v = Number(ms)
  if (isNaN(v) || v <= 0) return 0
  return Math.round((v / maxLatency.value) * 100)
}

// 行内小图标（12px）。stroke 直接传色，按状态变色。
function keyIcon(color: string) {
  return h('svg', { width: 12, height: 12, viewBox: '0 0 24 24', fill: 'none', stroke: color, 'stroke-width': 1.8, 'stroke-linecap': 'round', 'stroke-linejoin': 'round', class: 'cell-icon' }, [
    h('path', { d: 'M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4' }),
  ])
}
function alertIcon(color: string) {
  return h('svg', { width: 12, height: 12, viewBox: '0 0 24 24', fill: 'none', stroke: color, 'stroke-width': 1.8, 'stroke-linecap': 'round', 'stroke-linejoin': 'round', class: 'cell-icon' }, [
    h('path', { d: 'M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z' }),
    h('line', { x1: 12, y1: 9, x2: 12, y2: 13 }),
    h('line', { x1: 12, y1: 17, x2: 12.01, y2: 17 }),
  ])
}

const columns = [
  { title: '时间', key: 'created_at', width: 150, render: (row: any) => {
      const ts = formatLocalTime(row.created_at)
      if (!ts) return h('span', { class: 'mono time-na' }, '—')
      const sp = ts.indexOf(' ')
      const date = sp >= 0 ? ts.slice(0, sp) : ts
      const time = sp >= 0 ? ts.slice(sp + 1) : ''
      return h('div', { class: 'time-cell' }, [
        h('div', { class: 'time-date mono' }, date),
        h('div', { class: 'time-time mono' }, time),
      ])
    } },
  // 不用 naive 的 ellipsis：它会包裹自定义渲染内容，既会干扰「已删除」删除线，
  // 又会把钥匙图标一起塞进 tooltip。改用 .apikey-text 上的 CSS ellipsis 截断。
  { title: 'API Key', key: 'api_key', width: 160, render: (row: any) => {
      if (row.api_key_name) return h('span', { class: 'apikey-cell' }, [keyIcon('#0d6e6b'), h('span', { class: 'mono apikey-text' }, row.api_key_name)])
      if (row.api_key_preview) return h('span', { class: 'apikey-cell' }, [keyIcon('#9c6c00'), h('span', { class: 'mono apikey-text' }, row.api_key_preview)])
      if (row.api_key_id) return h('span', { class: 'apikey-cell' }, [keyIcon('#b3261e'), h('span', { class: 'mono apikey-deleted' }, '已删除')])
      return h('span', { class: 'apikey-cell' }, [keyIcon('#c9c0b0'), h('span', { class: 'mono apikey-text apikey-na' }, '—')])
    } },
  { title: '模型', key: 'model_id', width: 190, ellipsis: { tooltip: true }, render: (row: any) => h('span', { class: 'mono model-cell' }, row.model_id || '—') },
  { title: '供应商', key: 'provider_id', width: 76, align: 'center' as const, titleAlign: 'center' as const, render: (row: any) => {
      const p = providerMap.value[row.provider_id]
      const name = p?.name || row.provider_id
      const trigger = p?.icon
        ? h('span', { class: 'prov-icon-wrap' }, h('img', { src: iconUrl(p.icon), class: 'prov-icon', alt: '' }))
        : h('span', { class: 'prov-icon-wrap prov-mono-wrap' }, h('span', { class: 'mono prov-mono' }, row.provider_id.slice(0, 2).toUpperCase()))
      return h(NTooltip, { placement: 'top' }, { trigger: () => trigger, default: () => name })
    } },
  { title: 'Token（入 / 出）', key: 'tokens', width: 150, align: 'right' as const, titleAlign: 'right' as const, render: (row: any) => {
      const cacheRead = row.cache_read_tokens ?? 0
      const cacheWrite = row.cache_write_tokens ?? 0
      const hasCache = cacheRead > 0 || cacheWrite > 0
      return h('div', { class: 'token-info' }, [
        h('div', { class: 'token-main' }, [
          h('span', { class: 'token-in' }, fmtToken(row.input_tokens)),
          h('span', { class: 'token-arrow' }, '→'),
          h('span', { class: 'token-out' }, fmtToken(row.output_tokens)),
        ]),
        hasCache ? h('div', { class: 'token-cache' }, [
          h('span', { class: 'cache-label' }, '缓存'),
          h('span', { class: 'cache-read' }, `读 ${fmtToken(cacheRead)}`),
          h('span', { class: 'cache-sep' }, '·'),
          h('span', { class: 'cache-write' }, `写 ${fmtToken(cacheWrite)}`),
        ]) : null,
      ])
    } },
  { title: '延迟', key: 'latency_ms', width: 120, align: 'right' as const, titleAlign: 'right' as const, render: (row: any) => {
      const tier = latencyTier(row.latency_ms)
      return h('div', { class: 'latency-cell' }, [
        h('span', { class: `latency-num mono tier-${tier}` }, humanizeLatency(row.latency_ms)),
        h('div', { class: 'latency-track' }, [
          h('div', { class: `latency-fill tier-${tier}`, style: { width: `${latencyPct(row.latency_ms)}%` } }),
        ]),
      ])
    } },
  { title: '状态', key: 'status', width: 78, align: 'center' as const, titleAlign: 'center' as const, render: (row: any) => {
      const ok = row.status === 'success'
      return h('span', { class: `status-badge ${ok ? 'success' : 'error'}` }, [
        h('span', { class: 'status-dot-sm' }),
        h('span', { class: 'mono status-text' }, ok ? 'OK' : 'ERR'),
      ])
    } },
  { title: '错误', key: 'error_msg', render: (row: any) => {
      if (row.error_msg) return h(NTooltip, { placement: 'top' }, {
        trigger: () => h('span', { class: 'error-cell' }, [alertIcon('#b3261e'), h('span', { class: 'mono error-text' }, row.error_msg)]),
        default: () => row.error_msg,
      })
      return h('span', { class: 'mono error-na' }, '—')
    } },
]

async function loadProviders() {
  try {
    const res = await fetch(`${API_BASE}/providers`)
    const data = await res.json()
    const map: Record<string, { name: string; icon?: string }> = {}
    for (const p of data) map[p.id] = { name: p.name, icon: p.icon }
    providerMap.value = map
  } catch { /* 供应商图标缺失时单元格会回退到字母 monogram */ }
}

async function loadLogs() {
  loading.value = true
  try {
    const res = await fetch(`${API_BASE}/logs?page=${page.value}&page_size=${pageSize}`)
    const data = await res.json()
    logs.value = data.logs
    total.value = data.total
  } finally {
    loading.value = false
  }
}

// 先建好 providerMap 再渲染表格，避免首屏图标未就绪时出现 monogram 闪动。
async function init() {
  await loadProviders()
  await loadLogs()
}
onMounted(init)
</script>

<style scoped>
/* ====== 模板层元素：scoped 正常生效 ====== */
.logs { display: flex; flex-direction: column; gap: 18px; }

.page-header { display: flex; justify-content: space-between; align-items: flex-end; padding-bottom: 16px; border-bottom: 1px solid #ece6da; }
.page-title { font-size: 28px; font-weight: 600; color: #17140f; margin: 0; letter-spacing: -0.02em; line-height: 1; }
.page-subtitle { margin: 7px 0 0; color: #a89e8c; font-size: 13px; }
.count-block { display: flex; flex-direction: column; align-items: flex-end; gap: 4px; }
.count-label { font-size: 11px; color: #a89e8c; letter-spacing: 0.04em; }
.count-num { font-size: 26px; font-weight: 600; color: #17140f; line-height: 1; }
.count-num::after { content: ''; display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: #2ea86a; margin-left: 8px; vertical-align: 5px; box-shadow: 0 0 6px rgba(46,168,106,0.5); }

.table-container { background: #ffffff; border: 1px solid #d9cfbf; border-radius: 12px; overflow: hidden; }
.logs-table { --n-td-color: #ffffff; --n-th-color: #f4efe3; --n-td-color-hover: #f9f5ec; min-height: 200px; }

.empty-state { display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 70px 20px 80px; text-align: center; }
.empty-icon { width: 56px; height: 56px; border-radius: 50%; background: #f4efe3; border: 1px solid #ece6da; display: flex; align-items: center; justify-content: center; color: #c9c0b0; margin-bottom: 18px; }
.empty-text { margin: 0; font-size: 18px; font-weight: 500; color: #74695a; }
.empty-hint { margin: 8px 0 0; font-size: 13px; color: #a89e8c; }

.ledger-footer { display: flex; justify-content: space-between; align-items: center; }
.range-text { font-size: 12px; color: #a89e8c; }

/* ====== 骨架屏（模板层） ====== */
.skeleton { padding: 4px 0 8px; }
.sk-row { display: flex; align-items: center; gap: 14px; padding: 12px 24px; }
.sk-bar { height: 10px; border-radius: 4px; background: linear-gradient(90deg, #f4efe3 0%, #ece6da 40%, #f4efe3 80%); background-size: 200% 100%; animation: sk-shimmer 1.5s ease-in-out infinite; animation-delay: var(--sk-d, 0s); }
.w-time { width: 92px; }
.w-key { width: 84px; }
.w-model { width: 130px; }
.w-prov { width: 26px; height: 26px; border-radius: 7px; }
.w-tok { width: 78px; }
.w-lat { width: 54px; }
.w-st { width: 44px; }
.w-err { flex: 1; max-width: 220px; }
@keyframes sk-shimmer { 0% { background-position: 200% 0; } 100% { background-position: -200% 0; } }

@media (max-width: 600px) { .page-header { flex-direction: column; align-items: flex-start; gap: 14px; } .count-block { align-items: flex-start; } }
</style>

<!-- 单元格内容由 column.render 产生——这些 VNode 不携带本组件的 scoped id，
     scoped 选择器匹配不到，必须用 :deep(.foo)（编译为 .logs[data-v] .foo）才能命中。
     用 .logs 前缀兜底作用域，避免泄露到其它页面。 -->
<style scoped>
.logs :deep(.n-data-table-th) { font-weight: 600; }
.logs :deep(.n-data-table-td) { padding-top: 9px; padding-bottom: 9px; }
.logs :deep(.n-data-table-tr:hover .n-data-table-td) { background: #f9f5ec; }

.logs :deep(.cell-icon) { flex-shrink: 0; }

/* 时间 */
.logs :deep(.time-cell) { display: flex; flex-direction: column; gap: 1px; line-height: 1.25; }
.logs :deep(.time-date) { font-size: 11px; color: #a89e8c; }
.logs :deep(.time-time) { font-size: 12px; color: #17140f; font-weight: 500; }
.logs :deep(.time-na) { font-size: 12px; color: #a89e8c; }

/* API Key */
.logs :deep(.apikey-cell) { display: inline-flex; align-items: center; gap: 6px; max-width: 100%; }
.logs :deep(.apikey-text) { font-size: 12px; color: #74695a; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
.logs :deep(.apikey-text.apikey-na) { color: #a89e8c; }
.logs :deep(.apikey-deleted) { font-size: 12px; color: #b3261e; text-decoration: line-through; }

/* 模型 */
.logs :deep(.model-cell) { font-size: 12px; color: #74695a; }

/* 供应商图标 */
.logs :deep(.prov-icon-wrap) { display: inline-flex; align-items: center; justify-content: center; width: 26px; height: 26px; border-radius: 7px; background: #faf7f0; border: 1px solid #ece6da; transition: border-color 0.15s, background 0.15s; }
.logs :deep(.prov-icon) { width: 18px; height: 18px; object-fit: contain; display: block; }
.logs :deep(.prov-mono-wrap) { background: #f4efe3; }
.logs :deep(.prov-mono) { font-size: 10px; font-weight: 600; color: #0d6e6b; letter-spacing: 0.02em; }
.logs :deep(.n-data-table-tr:hover .prov-icon-wrap) { border-color: #bfe6cf; background: #f4fbf6; }

/* Token */
.logs :deep(.token-info) { display: inline-flex; flex-direction: column; align-items: flex-end; gap: 2px; }
.logs :deep(.token-main) { line-height: 1.3; font-size: 12px; }
.logs :deep(.token-in) { color: #9c6c00; }
.logs :deep(.token-arrow) { color: #c9c0b0; margin: 0 4px; }
.logs :deep(.token-out) { color: #0d6e6b; }
.logs :deep(.token-cache) { font-size: 10px; line-height: 1.2; white-space: nowrap; }
.logs :deep(.cache-label) { color: #a89e8c; margin-right: 6px; }
.logs :deep(.cache-read) { color: #0d6e6b; }
.logs :deep(.cache-sep) { color: #c9c0b0; margin: 0 4px; }
.logs :deep(.cache-write) { color: #b5842b; }

/* 延迟 */
.logs :deep(.latency-cell) { display: flex; flex-direction: column; align-items: flex-end; gap: 5px; }
.logs :deep(.latency-num) { font-size: 12px; font-weight: 500; }
.logs :deep(.latency-num.tier-fast) { color: #1d7a4c; }
.logs :deep(.latency-num.tier-mid) { color: #9c6c00; }
.logs :deep(.latency-num.tier-slow) { color: #b3261e; }
.logs :deep(.latency-num.tier-na) { color: #a89e8c; }
.logs :deep(.latency-track) { width: 64px; height: 3px; border-radius: 999px; background: #ece6da; overflow: hidden; }
.logs :deep(.latency-fill) { height: 100%; border-radius: 999px; min-width: 2px; }
.logs :deep(.latency-fill.tier-fast) { background: #2ea86a; }
.logs :deep(.latency-fill.tier-mid) { background: #b5842b; }
.logs :deep(.latency-fill.tier-slow) { background: #b3261e; }

/* 状态 */
.logs :deep(.status-badge) { display: inline-flex; align-items: center; gap: 5px; padding: 3px 10px; border-radius: 999px; }
.logs :deep(.status-badge.success) { background: rgba(46,168,106,0.08); border: 1px solid rgba(46,168,106,0.22); }
.logs :deep(.status-badge.error) { background: rgba(179,38,30,0.07); border: 1px solid rgba(179,38,30,0.22); }
.logs :deep(.status-dot-sm) { width: 5px; height: 5px; border-radius: 50%; }
.logs :deep(.status-badge.success .status-dot-sm) { background: #2ea86a; box-shadow: 0 0 4px rgba(46,168,106,0.5); }
.logs :deep(.status-badge.error .status-dot-sm) { background: #b3261e; }
.logs :deep(.status-text) { font-size: 10px; font-weight: 600; letter-spacing: 0.06em; }
.logs :deep(.status-badge.success .status-text) { color: #1d7a4c; }
.logs :deep(.status-badge.error .status-text) { color: #b3261e; }

/* 错误 */
.logs :deep(.error-cell) { display: inline-flex; align-items: center; gap: 6px; max-width: 100%; }
.logs :deep(.error-text) { font-size: 12px; color: #b3261e; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
.logs :deep(.error-na) { font-size: 12px; color: #a89e8c; }
</style>
