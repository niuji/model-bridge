<template>
  <div class="help">
    <div class="page-header">
      <h1 class="page-title serif">接入指南</h1>
      <p class="page-subtitle mono">把你的 AI 工具指向 Model Bridge · 网关地址由服务端提供</p>
    </div>

    <div class="tool-cards">
      <div class="tool-card" @click="openTool('claude-code')">
        <div class="card-icon">
          <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="#D97757" role="img" aria-label="Claude"><path d="m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z" /></svg>
        </div>
        <div class="card-meta">
          <div class="card-name">Claude Code</div>
          <div class="card-tag mono">Anthropic 协议</div>
        </div>
      </div>
      <div class="tool-card" @click="openTool('cursor')">
        <div class="card-icon">
          <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="#000000" role="img" aria-label="Cursor"><path d="M11.503.131 1.891 5.678a.84.84 0 0 0-.42.726v11.188c0 .3.162.575.42.724l9.609 5.55a1 1 0 0 0 .998 0l9.61-5.55a.84.84 0 0 0 .42-.724V6.404a.84.84 0 0 0-.42-.726L12.497.131a1.01 1.01 0 0 0-.996 0M2.657 6.338h18.55c.263 0 .43.287.297.515L12.23 22.918c-.062.107-.229.064-.229-.06V12.335a.59.59 0 0 0-.295-.51l-9.11-5.257c-.109-.063-.064-.23.061-.23" /></svg>
        </div>
        <div class="card-meta">
          <div class="card-name">Cursor</div>
          <div class="card-tag mono">OpenAI 协议</div>
        </div>
      </div>
      <div class="tool-card" @click="openTool('copilot')">
        <div class="card-icon">
          <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="#2A84EE" role="img" aria-label="GitHub Copilot"><circle cx="12" cy="12" r="11.5" fill="#2A84EE" stroke="#2A84EE"/><path d="M9.2 8.2h5.6v1.2H9.2zm0 3h5.6v1.2H9.2zm0 3h3.2v1.2H9.2z" fill="#fff"/></svg>
        </div>
        <div class="card-meta">
          <div class="card-name">GitHub Copilot</div>
          <div class="card-tag mono">VS Code · LLM Gateway</div>
        </div>
      </div>
    </div>

    <n-modal
      :show="showModal"
      :auto-focus="false"
      :mask-closable="true"
      @update:show="showModal = $event"
    >
      <div class="detail-card">
        <n-button size="tiny" text class="close-btn" @click="showModal = false">
          <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><line x1="18" y1="6" x2="6" y2="18" /><line x1="6" y1="6" x2="18" y2="18" /></svg>
        </n-button>

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
        <div v-else-if="activeTool === 'cursor'" class="tool-section">
          <div class="detail-title">
            <span class="detail-name">Cursor</span>
            <span class="detail-tag mono">OpenAI Chat · base_url {{ proxyBase }}/openai-chat/v1</span>
          </div>
          <p class="tool-desc">在 Cursor 设置里覆盖 OpenAI 基址：</p>
          <ol class="steps">
            <li>打开 <strong>Settings → Models</strong></li>
            <li>勾选 <strong>"Override OpenAI Base URL"</strong>，填入（可复制）：
              <div class="code-block">
                <n-button size="tiny" text class="copy-btn" @click="copy(cursorBaseUrl, 'Cursor 基址')">
                  <template #icon><svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg></template>
                  复制
                </n-button>
                <pre class="mono"><code>{{ cursorBaseUrl }}</code></pre>
              </div>
            </li>
            <li><strong>"OpenAI API Key"</strong> 填入你的 <code class="chip mono">mb-</code> 密钥（见「API 密钥」页）</li>
            <li>选择一个网关已接入的模型（见「供应商」页）</li>
          </ol>
        </div>

        <!-- GitHub Copilot (VS Code via LLM Gateway) -->
        <div v-else class="tool-section">
          <div class="detail-title">
            <span class="detail-name">GitHub Copilot (VS Code)</span>
            <span class="detail-tag mono">OpenAI Chat · base_url {{ proxyBase }}/openai-chat/v1</span>
          </div>
          <p class="tool-desc">通过 VS Code 插件 <strong>LLM Gateway</strong> 将 GitHub Copilot 接入 Model Bridge：</p>
          <ol class="steps">
            <li>VS Code 扩展商店搜索安装 <strong>LLM Gateway</strong></li>
            <li>打开 <strong>Settings → Extensions → LLM Gateway</strong></li>
            <li>在 <strong>Base URL</strong> 填入（可复制）：
              <div class="code-block">
                <n-button size="tiny" text class="copy-btn" @click="copy(copilotBaseUrl, 'Copilot 基址')">
                  <template #icon><svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="13" height="13" rx="2" ry="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg></template>
                  复制
                </n-button>
                <pre class="mono"><code>{{ copilotBaseUrl }}</code></pre>
              </div>
            </li>
            <li>在 <strong>API Key</strong> 填入你的 <code class="chip mono">mb-</code> 密钥（见「API 密钥」页），插件会自动从 <code class="chip mono">{{ proxyBase }}/openai-chat/v1/models</code> 发现可用模型</li>
          </ol>
          <ul class="notes">
            <li>LLM Gateway 把 Copilot 的请求转为标准 OpenAI Chat API，网关以 <code class="chip mono">/openai-chat/v1</code> 协议接入。</li>
            <li>网关 chat 通道中的模型均可直接使用，无需额外配置。</li>
          </ul>
        </div>
      </div>
    </n-modal>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { NButton, NModal, useMessage } from 'naive-ui'

const message = useMessage()
const API_BASE = '/api/admin'
const proxyBase = ref('http://localhost:10010')
const activeTool = ref<'claude-code' | 'cursor' | 'copilot'>('claude-code')
const showModal = ref(false)

const claudeSnippet = computed(() =>
`{
  "env": {
    "ANTHROPIC_BASE_URL": "${proxyBase.value}/anthropic",
    "ANTHROPIC_AUTH_TOKEN": "<your-api-key>",
    "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY": "1"
  }
}`)

const cursorBaseUrl = computed(() => `${proxyBase.value}/openai-chat/v1`)
const copilotBaseUrl = computed(() => `${proxyBase.value}/openai-chat/v1`)

function openTool(tool: 'claude-code' | 'cursor' | 'copilot') {
  activeTool.value = tool
  showModal.value = true
}

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
.page-title { font-size: 28px; font-weight: 600; color: var(--mb-text-1); margin: 0; letter-spacing: -0.02em; }
.page-subtitle { margin: 6px 0 0; color: var(--mb-text-3); font-size: 13px; }

.tool-cards { display: flex; gap: 12px; }
.tool-card { display: flex; align-items: center; gap: 12px; padding: 14px 18px; background: var(--mb-surface); border: 1px solid var(--mb-border); border-radius: 12px; cursor: pointer; transition: border-color 0.15s, background 0.15s, transform 0.1s; min-width: 200px; }
.tool-card:hover { border-color: var(--mb-muted); background: var(--mb-surface-inset); }
.tool-card:active { transform: translateY(1px); }
.tool-card .card-icon { display: flex; align-items: center; }
.card-meta { display: flex; flex-direction: column; gap: 2px; }
.card-name { font-size: 14px; font-weight: 600; color: var(--mb-text-1); transition: color 0.15s; }
.tool-card:hover .card-name { color: var(--mb-success-d); }
.card-tag { font-size: 11px; color: var(--mb-text-3); }

.detail-card { position: relative; background: var(--mb-surface); border: 1px solid var(--mb-border); border-radius: 14px; padding: 20px 24px 24px; width: min(680px, 92vw); max-height: 85vh; overflow-y: auto; }
.detail-title { display: flex; align-items: baseline; gap: 12px; margin-bottom: 16px; padding: 0 36px 14px 0; border-bottom: 1px solid var(--mb-border); }
.detail-name { font-size: 17px; font-weight: 600; color: var(--mb-text-1); }
.detail-tag { font-size: 12px; color: var(--mb-text-2); }

.close-btn { position: absolute; top: 14px; right: 14px; color: var(--mb-text-3); opacity: 0.6; transition: opacity 0.15s, color 0.15s; }
.close-btn:hover { opacity: 1; color: var(--mb-text-1); }

.tool-section { padding-top: 4px; }
.tool-desc { color: var(--mb-text-2); font-size: 14px; line-height: 1.7; margin: 0 0 16px; }

.code-block { position: relative; background: var(--mb-surface-inset); border: 1px solid var(--mb-border); border-radius: 12px; margin: 10px 0 14px; }
.code-block pre { margin: 0; padding: 16px 56px 16px 16px; overflow-x: auto; font-size: 13px; line-height: 1.6; color: var(--mb-text-1); white-space: pre; }
.code-block .copy-btn { position: absolute; top: 8px; right: 8px; color: var(--mb-success-d); opacity: 0.45; transition: opacity 0.15s; font-size: 12px; }
.code-block .copy-btn:hover { opacity: 1; }

.chip { font-size: 12px; color: var(--mb-success-d); background: var(--mb-surface-inset); padding: 1px 7px; border: 1px solid var(--mb-border); border-radius: 6px; }

.notes { color: var(--mb-text-2); font-size: 13px; line-height: 1.85; padding-left: 18px; margin: 12px 0 0; }
.notes li { margin: 3px 0; }
.notes li::marker { color: var(--mb-text-3); }

.steps { color: var(--mb-text-1); font-size: 14px; line-height: 1.9; padding-left: 20px; margin: 8px 0 0; }
.steps li { margin: 8px 0; }
.steps li::marker { color: var(--mb-success-d); font-weight: 600; }
</style>
