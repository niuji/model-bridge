<template>
  <div class="help">
    <div class="page-header">
      <h1 class="page-title serif">接入指南</h1>
      <p class="page-subtitle mono">把你的 AI 工具指向 Model Bridge · 网关地址由服务端提供</p>
    </div>

    <div class="tool-cards">
      <div class="tool-card" :class="{ active: activeTool === 'claude-code' }" @click="activeTool = 'claude-code'">
        <div class="card-icon">
          <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="4 17 10 11 4 5" /><line x1="12" y1="19" x2="20" y2="19" /></svg>
        </div>
        <div class="card-meta">
          <div class="card-name">Claude Code</div>
          <div class="card-tag mono">Anthropic 协议</div>
        </div>
      </div>
      <div class="tool-card" :class="{ active: activeTool === 'cursor' }" @click="activeTool = 'cursor'">
        <div class="card-icon">
          <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M3 3l7.07 16.97 2.51-7.39 7.39-2.51L3 3z" /><path d="M13 13l6 6" /></svg>
        </div>
        <div class="card-meta">
          <div class="card-name">Cursor</div>
          <div class="card-tag mono">OpenAI 协议</div>
        </div>
      </div>
    </div>

    <div class="detail-card">
      <!-- Claude Code -->
      <div v-if="activeTool === 'claude-code'" class="tool-section">
        <div class="detail-title">
          <span class="detail-name">Claude Code</span>
          <span class="detail-tag mono">Anthropic · base_url {{ proxyBase }}/anthropic</span>
        </div>
        <p class="tool-desc">推荐写入 <code class="chip mono">~/.claude/settings.json</code>（比 shell <code class="chip mono">export</code> 更持久，且对 CLI / IDE 插件等所有 Claude Code 入口统一生效；settings 的 <code class="chip mono">env</code> 优先于 shell 环境变量）。</p>
        <div class="code-block">
          <n-button size="tiny" text class="copy-btn" @click="copy(claudeSnippet, 'Claude Code 配置')">
            <template #icon><svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg></template>
            复制
          </n-button>
          <pre class="mono"><code>{{ claudeSnippet }}</code></pre>
        </div>
        <ul class="notes">
          <li><code class="chip mono">ANTHROPIC_AUTH_TOKEN</code> 走 <code class="chip mono">Authorization: Bearer</code>，网关已支持（<code class="chip mono">ANTHROPIC_API_KEY</code> 走 <code class="chip mono">x-api-key</code>，亦可用）。</li>
          <li><code class="chip mono">CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY=1</code> 让 Claude Code 启动时拉取网关 <code class="chip mono">GET /v1/models</code>，在 <code class="chip mono">/model</code> 选择器以「From gateway」列出可用模型，选中即用。</li>
          <li>网关对非 <code class="chip mono">claude-</code> / <code class="chip mono">anthropic-</code> 命名的模型会补 <code class="chip mono">claude-</code> 前缀，故所有接入 anthropic 通道的模型都能被发现。</li>
          <li>可在 settings.json 顶层加 <code class="chip mono">"model": "claude-…"</code> 设默认模型；<code class="chip mono">ANTHROPIC_DEFAULT_*_MODEL</code> 在自定义 base_url 下不生效。</li>
          <li>请求中的 <code class="chip mono">model</code> 会被网关改写为上游真实 id，无需关心大小写。</li>
        </ul>
      </div>

      <!-- Cursor -->
      <div v-else class="tool-section">
        <div class="detail-title">
          <span class="detail-name">Cursor</span>
          <span class="detail-tag mono">OpenAI · base_url {{ proxyBase }}/openai/v1</span>
        </div>
        <p class="tool-desc">在 Cursor 设置里覆盖 OpenAI 基址：</p>
        <ol class="steps">
          <li>打开 <strong>Settings → Models</strong></li>
          <li>勾选 <strong>“Override OpenAI Base URL”</strong>，填入（可复制）：
            <div class="code-block">
              <n-button size="tiny" text class="copy-btn" @click="copy(cursorBaseUrl, 'Cursor 基址')">
                <template #icon><svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg></template>
                复制
              </n-button>
              <pre class="mono"><code>{{ cursorBaseUrl }}</code></pre>
            </div>
          </li>
          <li><strong>“OpenAI API Key”</strong> 填入你的 <code class="chip mono">mb-</code> 密钥（见「API 密钥」页）</li>
          <li>选择一个网关已接入的模型（见「供应商」页）</li>
        </ol>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { NButton, useMessage } from 'naive-ui'

const message = useMessage()
const API_BASE = '/api/admin'
const proxyBase = ref('http://localhost:10010')
const activeTool = ref<'claude-code' | 'cursor'>('claude-code')

const claudeSnippet = computed(() =>
`{
  "env": {
    "ANTHROPIC_BASE_URL": "${proxyBase.value}/anthropic",
    "ANTHROPIC_AUTH_TOKEN": "<your-api-key>",
    "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY": "1"
  }
}`)

const cursorBaseUrl = computed(() => `${proxyBase.value}/openai/v1`)

async function copy(text: string, label: string) {
  try {
    await navigator.clipboard.writeText(text)
    message.success(`${label}已复制到剪贴板`)
  } catch {
    message.error('复制失败')
  }
}

onMounted(async () => {
  try {
    const res = await fetch(`${API_BASE}/settings`)
    if (res.ok) {
      const data = await res.json()
      if (data.proxy_base_url) proxyBase.value = data.proxy_base_url
    }
  } catch {
    // 取不到则回退默认 localhost:10010
  }
})
</script>

<style scoped>
.page-header { margin-bottom: 24px; }
.page-title { font-size: 28px; font-weight: 600; color: #2d2d2a; margin: 0; letter-spacing: -0.02em; }
.page-subtitle { margin: 6px 0 0; color: #a0a098; font-size: 13px; }

.tool-cards { display: flex; gap: 12px; margin-bottom: 20px; }
.tool-card { display: flex; align-items: center; gap: 12px; padding: 14px 18px; background: #ffffff; border: 1px solid #e0dcd5; border-radius: 14px; cursor: pointer; transition: border-color 0.15s, background 0.15s; min-width: 200px; }
.tool-card:hover { border-color: #c5c0b8; background: #faf8f4; }
.tool-card.active { border-color: #2d6a4f; background: rgba(45, 106, 79, 0.04); }
.tool-card .card-icon { color: #a0a098; display: flex; align-items: center; transition: color 0.15s; }
.tool-card.active .card-icon { color: #2d6a4f; }
.card-meta { display: flex; flex-direction: column; gap: 2px; }
.card-name { font-size: 14px; font-weight: 600; color: #2d2d2a; transition: color 0.15s; }
.tool-card.active .card-name { color: #2d6a4f; }
.card-tag { font-size: 11px; color: #a0a098; }

.detail-card { background: #ffffff; border: 1px solid #e0dcd5; border-radius: 16px; padding: 20px 24px 24px; }
.detail-title { display: flex; align-items: baseline; gap: 12px; margin-bottom: 16px; padding-bottom: 14px; border-bottom: 1px solid #e0dcd5; }
.detail-name { font-size: 17px; font-weight: 600; color: #2d2d2a; }
.detail-tag { font-size: 12px; color: #787870; }

.tool-section { padding-top: 4px; }
.tool-desc { color: #787870; font-size: 14px; line-height: 1.7; margin: 0 0 16px; }

.code-block { position: relative; background: #faf8f4; border: 1px solid #e0dcd5; border-radius: 12px; margin: 10px 0 14px; }
.code-block pre { margin: 0; padding: 16px 56px 16px 16px; overflow-x: auto; font-size: 13px; line-height: 1.6; color: #2d2d2a; white-space: pre; }
.code-block .copy-btn { position: absolute; top: 8px; right: 8px; color: #2d6a4f; opacity: 0.45; transition: opacity 0.15s; font-size: 12px; }
.code-block .copy-btn:hover { opacity: 1; }

.chip { font-size: 12px; color: #5a7a68; background: #f0ebe0; padding: 1px 7px; border: 1px solid #e0dcd5; border-radius: 6px; }

.notes { color: #787870; font-size: 13px; line-height: 1.85; padding-left: 18px; margin: 12px 0 0; }
.notes li { margin: 3px 0; }
.notes li::marker { color: #a0a098; }

.steps { color: #2d2d2a; font-size: 14px; line-height: 1.9; padding-left: 20px; margin: 8px 0 0; }
.steps li { margin: 8px 0; }
.steps li::marker { color: #2d6a4f; font-weight: 600; }
</style>
