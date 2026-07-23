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
          <span class="sk-bar w-res" />
        </div>
      </div>
      <n-data-table
        v-else-if="logs.length"
        :columns="columns"
        :data="logs"
        :bordered="false"
        :loading="loading"
        :scroll-x="scrollX"
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

// 拆 user-agent 为 { product, version }：
// - 取首个空白前的 token，再按 '/' 分产品 / 版本。
// - 例：'claude-cli/2.1.201 (external, cli)' -> { product: 'claude-cli', version: '2.1.201' }
// - 'OpenAI/JS 5.20.1' -> { product: 'openai-js',  version: '5.20.1' }
// - 'node'                -> { product: 'node',     version: null }
function parseClient(ua: any): { product: string; version: string | null } | null {
  if (!ua) return null
  const s = String(ua).trim()
  if (!s) return null
  // OpenAI/JS x.y.z: product 为 openai-js，version 为数字部分
  const openaiJs = s.match(/^OpenAI\/JS\s+([\d.]+)/)
  if (openaiJs) return { product: 'openai-js', version: openaiJs[1] || null }
  const sp = s.search(/\s/)
  const head = sp >= 0 ? s.slice(0, sp) : s
  const slash = head.indexOf('/')
  if (slash < 0) return { product: head, version: null }
  const product = head.slice(0, slash)
  const version = head.slice(slash + 1)
  if (!product) return { product: head, version: null }
  return { product, version: version || null }
}

// 通道类型 → 短标签 + 协议族着色。openai_chat/openai_responses 同属 openai 族（青），
// anthropic 单独一族（赭）。旧记录无 channel 时返回 null，单元格回退为「—」。
function channelMeta(ch: any): { text: string; tier: 'openai' | 'anthropic' } | null {
  if (ch === 'anthropic') return { text: 'anthropic', tier: 'anthropic' }
  if (ch === 'openai_responses') return { text: 'responses', tier: 'openai' }
  if (ch === 'openai_chat') return { text: 'chat', tier: 'openai' }
  return null
}

// 单层表头 7 列：时间 / 调用方 / 模型（含供应商图标）/ 通道 / Token / 延迟 / 结果。
// 调用方主行强化客户端、副行弱化 key；供应商并入「模型」列，通道独立成列。
const columns = [
  { title: '时间', key: 'created_at', width: 148, titleAlign: 'center' as const, render: (row: any) => {
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
  // 「调用方」：一行排列——终端图标 + 客户端名 + 版本徽标 · 钥匙图标 + key 名。
  // 客户端为主（#475569）、key 为次（小号淡色），用 · 分隔；两段文本各自省略号。
  { title: '调用方', key: 'caller', width: 216, titleAlign: 'center' as const, render: (row: any) => {
      let keyTextClass = 'mono caller-key'
      let keyText: string
      if (row.api_key_name) { keyText = row.api_key_name }
      else if (row.api_key_preview) { keyText = row.api_key_preview; keyTextClass += ' caller-key-anon' }
      else if (row.api_key_id) { keyText = '已删除'; keyTextClass = 'mono caller-key-deleted' }
      else { keyText = '—'; keyTextClass += ' caller-key-na' }
      const parsed = parseClient(row.client)
      const clientIcon = h('svg', { class: 'caller-client-icon', width: 12, height: 12, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': 2, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [
        h('rect', { x: 3, y: 4, width: 18, height: 16, rx: 2 }),
        h('path', { d: 'M7 9l3 3-3 3' }),
      ])
      const keyIcon = h('svg', { class: 'caller-key-icon', width: 12, height: 12, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': 2, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [
        h('circle', { cx: 7.5, cy: 12, r: 3.5 }),
        h('path', { d: 'M11 12h10' }),
        h('path', { d: 'M18 12v3' }),
        h('path', { d: 'M15 12v2' }),
      ])
      const children: any[] = [clientIcon]
      if (parsed) {
        children.push(h('span', { class: 'caller-client-product' }, parsed.product))
        if (parsed.version) children.push(h('span', { class: 'caller-client-version' }, `v${parsed.version}`))
      } else {
        children.push(h('span', { class: 'caller-client-na' }, '—'))
      }
      children.push(h('span', { class: 'caller-sep' }, '·'), keyIcon, h('span', { class: keyTextClass }, keyText))
      const inner = h('div', { class: 'caller-body mono' }, children)
      if (!parsed) return inner
      return h(NTooltip, { placement: 'top' }, { trigger: () => inner, default: () => row.client })
    } },
  // 模型：供应商图标 + model id（省略号，hover 全名）。供应商列已并入此处，通道独立成列。
  { title: '模型', key: 'model_id', width: 176, titleAlign: 'center' as const, render: (row: any) => {
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
  { title: '通道', key: 'channel', width: 100, align: 'center' as const, titleAlign: 'center' as const, render: (row: any) => {
      const ch = channelMeta(row.channel)
      return ch
        ? h(NTooltip, { placement: 'top' }, {
            trigger: () => h('span', { class: `mono ch-tag ch-${ch.tier}` }, ch.text),
            default: () => row.channel,
          })
        : h('span', { class: 'mono ch-tag ch-na' }, '—')
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
  { title: '延迟', key: 'latency_ms', width: 108, align: 'right' as const, titleAlign: 'right' as const, render: (row: any) => {
      const tier = latencyTier(row.latency_ms)
      return h('div', { class: 'latency-cell' }, [
        h('span', { class: `latency-num mono tier-${tier}` }, humanizeLatency(row.latency_ms)),
        h('div', { class: 'latency-track' }, [
          h('div', { class: `latency-fill tier-${tier}`, style: { width: `${latencyPct(row.latency_ms)}%` } }),
        ]),
      ])
    } },
  // 结果：合并原「状态」+「错误」。成功只显 OK 徽标；
  // 失败显 ERR 徽标 + 截断错误信息，hover 看完整错误。少一列、少一组图标噪音。
  { title: '结果', key: 'result', width: 220, titleAlign: 'center' as const, render: (row: any) => {
      const ok = row.status === 'success'
      const badge = h('span', { class: `status-badge ${ok ? 'success' : 'error'}` }, [
        h('span', { class: 'status-dot-sm' }),
        h('span', { class: 'mono status-text' }, ok ? 'OK' : 'ERR'),
      ])
      if (ok) return h('div', { class: 'result-cell' }, badge)
      const msg = row.error_msg || '未知错误'
      const inner = h('div', { class: 'result-cell' }, [
        badge,
        h('span', { class: 'mono error-text' }, msg),
      ])
      return h(NTooltip, { placement: 'top' }, { trigger: () => inner, default: () => msg })
    } },
]

// 列宽总和：交给 n-data-table 的 scroll-x，窄屏时表格内部横向滚动（表头跟随），
// 不再被 .table-container 的 overflow:hidden 裁掉右侧列。
const scrollX = columns.reduce((s, c) => s + (Number(c.width) || 0), 0)

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

.page-header { display: flex; justify-content: space-between; align-items: flex-end; padding-bottom: 16px; border-bottom: 1px solid var(--mb-divider); }
.page-title { font-size: 28px; font-weight: 600; color: var(--mb-text-1); margin: 0; letter-spacing: -0.02em; line-height: 1; }
.page-subtitle { margin: 7px 0 0; color: var(--mb-text-3); font-size: 13px; }
.count-block { display: flex; flex-direction: column; align-items: flex-end; gap: 4px; }
.count-label { font-size: 11px; color: var(--mb-text-3); letter-spacing: 0.04em; }
.count-num { font-size: 26px; font-weight: 600; color: var(--mb-text-1); line-height: 1; }
.count-num::after { content: ''; display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: var(--mb-success); margin-left: 8px; vertical-align: 5px; box-shadow: 0 0 6px rgba(34,197,94,0.5); }

.table-container { background: var(--mb-surface); border: 1px solid var(--mb-border); border-radius: 12px; overflow: hidden; }
.logs-table { --n-td-color: var(--mb-surface); --n-th-color: var(--mb-surface-inset); --n-td-color-hover: var(--mb-surface-inset); min-height: 200px; }

.empty-state { display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 70px 20px 80px; text-align: center; }
.empty-icon { width: 56px; height: 56px; border-radius: 50%; background: var(--mb-surface-inset); border: 1px solid var(--mb-divider); display: flex; align-items: center; justify-content: center; color: var(--mb-muted); margin-bottom: 18px; }
.empty-text { margin: 0; font-size: 18px; font-weight: 500; color: var(--mb-text-2); }
.empty-hint { margin: 8px 0 0; font-size: 13px; color: var(--mb-text-3); }

.ledger-footer { display: flex; justify-content: space-between; align-items: center; }
.range-text { font-size: 12px; color: var(--mb-text-3); }

/* ====== 骨架屏（模板层） ====== */
.skeleton { padding: 4px 0 8px; }
.sk-row { display: flex; align-items: center; gap: 14px; padding: 12px 24px; }
.sk-bar { height: 10px; border-radius: 4px; background: linear-gradient(90deg, var(--mb-surface) 0%, var(--mb-surface-inset) 40%, var(--mb-surface) 80%); background-size: 200% 100%; animation: sk-shimmer 1.5s ease-in-out infinite; animation-delay: var(--sk-d, 0s); }
.w-time { width: 88px; }
.w-key { width: 90px; }
.w-model { width: 120px; }
.w-prov { width: 60px; }
.w-tok { width: 78px; }
.w-lat { width: 54px; }
.w-res { flex: 1; max-width: 200px; }
@keyframes sk-shimmer { 0% { background-position: 200% 0; } 100% { background-position: -200% 0; } }

@media (max-width: 600px) { .page-header { flex-direction: column; align-items: flex-start; gap: 14px; } .count-block { align-items: flex-start; } }
</style>

<!-- 单元格内容由 column.render 产生——这些 VNode 不携带本组件的 scoped id，
     scoped 选择器匹配不到，必须用 :deep(.foo)（编译为 .logs[data-v] .foo）才能命中。
     用 .logs 前缀兜底作用域，避免泄露到其它页面。 -->
<style scoped>
.logs :deep(.n-data-table-th) { font-weight: 600; }
.logs :deep(.n-data-table-td) { padding-top: 9px; padding-bottom: 9px; }
.logs :deep(.n-data-table-tr:hover .n-data-table-td) { background: var(--mb-surface-inset); }

.logs :deep(.cell-icon) { flex-shrink: 0; }

/* 时间 */
.logs :deep(.time-cell) { display: flex; flex-direction: column; align-items: center; gap: 1px; line-height: 1.25; }
.logs :deep(.time-date) { font-size: 11px; color: var(--mb-text-3); }
.logs :deep(.time-time) { font-size: 12px; color: var(--mb-text-2); font-weight: 500; }
.logs :deep(.time-na) { font-size: 12px; color: var(--mb-text-3); }

/* 调用方：一行排列——终端图标 + 客户端名 + 版本徽标 · 钥匙图标 + key 名。
   客户端为主（次级文本）、key 为次（小号淡色），用 · 分隔；两段文本各自省略号。 */
.logs :deep(.caller-body) { display: flex; align-items: center; gap: 6px; min-width: 0; max-width: 100%; line-height: 1.25; padding: 2px 0; }

.logs :deep(.caller-client-icon) { flex-shrink: 0; color: var(--mb-text-3); }
.logs :deep(.caller-client-product) { flex: 0 1 auto; font-size: 13px; font-weight: 500; color: var(--mb-text-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; letter-spacing: -0.01em; }
.logs :deep(.caller-client-version) { flex-shrink: 0; font-size: 10px; color: var(--mb-text-3); background: var(--mb-surface-inset); border: 1px solid var(--mb-divider); padding: 0 5px; border-radius: 4px; line-height: 1.4; white-space: nowrap; }
.logs :deep(.caller-client-na) { flex-shrink: 0; font-size: 13px; color: var(--mb-text-3); }

.logs :deep(.caller-sep) { flex-shrink: 0; color: var(--mb-divider); }
.logs :deep(.caller-key-icon) { flex-shrink: 0; color: var(--mb-muted); }
.logs :deep(.caller-key) { flex: 0 1 auto; font-size: 11px; color: var(--mb-text-3); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
.logs :deep(.caller-key-anon) { color: var(--mb-muted); }
.logs :deep(.caller-key-na) { color: var(--mb-divider); }
.logs :deep(.caller-key-deleted) { flex: 0 1 auto; font-size: 11px; color: var(--mb-error); text-decoration: line-through; text-decoration-color: rgba(239,68,68,0.5); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }

/* 模型：供应商图标 + model id（省略号）。flex 行，图标固定不缩、id 收缩省略。 */
.logs :deep(.model-cell) { display: flex; align-items: center; justify-content: center; gap: 7px; min-width: 0; max-width: 100%; }
.logs :deep(.model-id) { font-size: 12px; color: var(--mb-text-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }

/* 供应商图标：并入「模型」列前缀，缩小为 18px 内联贴片。 */
.logs :deep(.prov-icon-wrap) { display: inline-flex; align-items: center; justify-content: center; width: 18px; height: 18px; flex-shrink: 0; border-radius: 5px; background: var(--mb-surface-inset); border: 1px solid var(--mb-divider); transition: border-color 0.15s, background 0.15s; }
.logs :deep(.prov-icon) { width: 13px; height: 13px; object-fit: contain; display: block; }
.logs :deep(.prov-mono-wrap) { background: var(--mb-surface-inset); }
.logs :deep(.prov-mono) { font-size: 8px; font-weight: 600; color: var(--mb-cyan); letter-spacing: 0.02em; }
.logs :deep(.n-data-table-tr:hover .prov-icon-wrap) { border-color: var(--mb-tint-green); background: var(--mb-surface); }

/* 通道标签：按协议族着色（openai 系青、anthropic 赭），hover 显完整 channel 类型。 */
.logs :deep(.ch-tag) { font-size: 10px; font-weight: 500; padding: 1px 7px; border-radius: 999px; letter-spacing: 0.02em; white-space: nowrap; line-height: 1.4; }
.logs :deep(.ch-tag.ch-openai) { color: var(--mb-cyan); background: rgba(6,182,212,0.08); border: 1px solid rgba(6,182,212,0.18); }
.logs :deep(.ch-tag.ch-anthropic) { color: var(--mb-warning); background: rgba(245,158,11,0.08); border: 1px solid rgba(245,158,11,0.18); }
.logs :deep(.ch-tag.ch-na) { color: var(--mb-text-3); }

/* Token */
.logs :deep(.token-info) { display: inline-flex; flex-direction: column; align-items: flex-end; gap: 2px; }
.logs :deep(.token-main) { line-height: 1.3; font-size: 12px; }
.logs :deep(.token-in) { color: var(--mb-text-2); }
.logs :deep(.token-arrow) { color: var(--mb-muted); margin: 0 4px; }
.logs :deep(.token-out) { color: var(--mb-cyan); }
.logs :deep(.token-cache) { font-size: 10px; line-height: 1.2; white-space: nowrap; }
.logs :deep(.cache-label) { color: var(--mb-text-3); margin-right: 6px; }
.logs :deep(.cache-read) { color: var(--mb-success-d); }
.logs :deep(.cache-sep) { color: var(--mb-muted); margin: 0 4px; }
.logs :deep(.cache-write) { color: var(--mb-warning); }

/* 延迟 */
.logs :deep(.latency-cell) { display: flex; flex-direction: column; align-items: flex-end; gap: 5px; }
.logs :deep(.latency-num) { font-size: 12px; font-weight: 500; }
.logs :deep(.latency-num.tier-fast) { color: var(--mb-success-d); }
.logs :deep(.latency-num.tier-mid) { color: var(--mb-warning); }
.logs :deep(.latency-num.tier-slow) { color: var(--mb-error); }
.logs :deep(.latency-num.tier-na) { color: var(--mb-text-3); }
.logs :deep(.latency-track) { width: 64px; height: 3px; border-radius: 999px; background: var(--mb-divider); overflow: hidden; }
.logs :deep(.latency-fill) { height: 100%; border-radius: 999px; min-width: 2px; }
.logs :deep(.latency-fill.tier-fast) { background: var(--mb-success); }
.logs :deep(.latency-fill.tier-mid) { background: var(--mb-warning); }
.logs :deep(.latency-fill.tier-slow) { background: var(--mb-error); }

/* 结果（合并状态 + 错误） */
.logs :deep(.result-cell) { display: inline-flex; align-items: center; gap: 8px; max-width: 100%; min-width: 0; }
.logs :deep(.result-cell .error-text) { font-size: 12px; color: var(--mb-error); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
.logs :deep(.status-badge) { display: inline-flex; align-items: center; gap: 5px; padding: 3px 10px; border-radius: 999px; flex-shrink: 0; }
.logs :deep(.status-badge.success) { background: rgba(34,197,94,0.08); border: 1px solid rgba(34,197,94,0.22); }
.logs :deep(.status-badge.error) { background: rgba(239,68,68,0.07); border: 1px solid rgba(239,68,68,0.22); }
.logs :deep(.status-dot-sm) { width: 5px; height: 5px; border-radius: 50%; }
.logs :deep(.status-badge.success .status-dot-sm) { background: var(--mb-success); box-shadow: 0 0 4px rgba(34,197,94,0.5); }
.logs :deep(.status-badge.error .status-dot-sm) { background: var(--mb-error); }
.logs :deep(.status-text) { font-size: 10px; font-weight: 600; letter-spacing: 0.06em; }
.logs :deep(.status-badge.success .status-text) { color: var(--mb-success-d); }
.logs :deep(.status-badge.error .status-text) { color: var(--mb-error); }
</style>
