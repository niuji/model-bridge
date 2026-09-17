<template>
  <button
    type="button" class="update-version mono" :class="{ available: candidate }"
    :title="candidate ? `可更新至 v${candidate.version}` : '检查更新'"
    :aria-label="`当前版本 ${currentVersion}，检查更新`" @click="show = true"
  >
    {{ collapsed ? '↑' : `v${currentVersion}` }}<span v-if="candidate" class="update-dot" />
  </button>
  <n-modal v-model:show="show" preset="card" title="软件更新" class="update-dialog" style="width: min(480px, calc(100vw - 32px))">
    <n-space vertical :size="16">
      <div>当前版本 <strong class="mono">v{{ currentVersion }}</strong></div>
      <n-alert v-if="status && !status.supported" type="info" :show-icon="false">
        {{ status.unsupported_reason || '此安装方式不支持网页更新，请使用安装脚本升级。' }}
      </n-alert>
      <n-alert v-if="waitingTooLong" type="warning" :show-icon="false">
        <p>等待已超过 3 分钟，尚未确认更新结果，页面会继续查询。请先查看日志：</p>
        <code>journalctl --user -u model-bridge-update -n 100</code>
        <p>若日志显示任务已中断，可执行恢复命令：</p>
        <code>systemctl --user start model-bridge-update</code>
      </n-alert>
      <n-alert v-else-if="disconnected" :type="active ? 'info' : 'warning'" :show-icon="false">
        {{ active ? '服务正在重启，正在等待重新连接…' : '暂时无法连接服务，正在重试…' }}
      </n-alert>
      <n-alert v-if="status?.job" :type="jobType" :show-icon="false">
        <div>{{ phaseLabel }} · v{{ status.job.version }}</div>
        <div v-if="status.job.error" class="update-error">{{ status.job.error }}</div>
        <template v-if="needsRecovery">
          <p>请在安装服务的用户终端执行恢复命令：</p>
          <code>systemctl --user start model-bridge-update</code>
          <p>查看更新日志：</p>
          <code>journalctl --user -u model-bridge-update -n 100</code>
        </template>
      </n-alert>
      <template v-if="candidate">
        <div>可更新至 <strong class="mono">v{{ candidate.version }}</strong></div>
        <a :href="candidate.html_url" target="_blank" rel="noopener noreferrer">查看发布说明 ↗</a>
        <p class="update-note">更新会重启服务，最多等待现有请求 120 秒；超过 120 秒则取消更新，超长请求可能中断。请稍后再发起新请求。</p>
      </template>
      <div v-else-if="status?.check?.checked_at && !status.check.error && !status.checking">当前已是最新稳定版本</div>
      <div v-if="status?.check?.checked_at" class="update-note">上次检查：{{ new Date(status.check.checked_at * 1000).toLocaleString() }}</div>
      <n-alert v-if="requestError || status?.check?.error" type="error" :show-icon="false">
        {{ requestError || status?.check?.error }}
      </n-alert>
      <n-space justify="end">
        <n-button :loading="status?.checking || sending === 'check'" :disabled="busy" @click="send('check')">检查更新</n-button>
        <n-button v-if="candidate" type="primary" :loading="sending === 'apply' || (active && !waitingTooLong)" :disabled="busy || !status?.supported" @click="send('apply')">更新并重启</n-button>
      </n-space>
    </n-space>
  </n-modal>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { NAlert, NButton, NModal, NSpace } from 'naive-ui'
import { adminRequest, errorMessage } from '../api'

type ReleaseInfo = { version: string, html_url: string }
type Job = { id: string, version: string, old_version: string, phase: string, error: string | null }
type UpdateStatus = {
  current_version: string
  supported: boolean
  unsupported_reason: string | null
  checking: boolean
  check: { checked_at: number | null, candidate: ReleaseInfo | null, error: string | null } | null
  job: Job | null
}

const props = defineProps<{ version: string, collapsed: boolean }>()
const show = ref(false)
const status = ref<UpdateStatus | null>(null)
const disconnected = ref(false)
const waitingTooLong = ref(false)
let activeSince: number | undefined
const requestError = ref('')
const sending = ref('')
const requestedJob = ref<string | null>(null)
const terminal = new Set(['succeeded', 'rolled_back', 'failed', 'recovery_required'])
const active = computed(() => !!requestedJob.value || !!(status.value?.job && !terminal.has(status.value.job.phase)))
const busy = computed(() => active.value || !!sending.value || !!status.value?.checking || status.value?.job?.phase === 'recovery_required')
const candidate = computed(() => status.value?.check?.candidate)
const currentVersion = computed(() => status.value?.current_version || props.version)
const needsRecovery = computed(() => status.value?.job?.phase === 'recovery_required')
const jobType = computed(() => {
  const phase = status.value?.job?.phase
  return phase === 'succeeded' ? 'success' : ['failed', 'recovery_required', 'rolled_back'].includes(phase || '') ? 'warning' : 'info'
})
const phases: Record<string, string> = {
  downloading: '正在下载并校验', stopping: '正在等待现有请求结束', backed_up: '备份完成',
  validating: '正在验证新版本', committed: '新版本已提交，正在启动', succeeded: '更新成功',
  rolling_back: '正在回滚', rollback_validating: '正在验证回滚结果', rolled_back: '已恢复原版本',
  failed: '更新失败', recovery_required: '需要恢复更新任务', restarting_old: '恢复原服务中',
}
const phaseLabel = computed(() => phases[status.value?.job?.phase || ''] || '正在更新')
let timer: ReturnType<typeof setTimeout> | undefined
let polling = false
let disposed = false
let initialVersion: string | undefined
let reloadStarted = false

function schedule() {
  clearTimeout(timer)
  if (!disposed) timer = setTimeout(poll, show.value || active.value ? 2000 : 60000)
}

async function poll() {
  if (polling || disposed) return
  clearTimeout(timer)
  polling = true
  try {
    const response = await adminRequest('/update', { signal: AbortSignal.timeout(10000) })
    const next: UpdateStatus = await response.json()
    if (disposed) return
    initialVersion ||= next.current_version
    status.value = next
    disconnected.value = false
    if (requestedJob.value && next.job?.id === requestedJob.value && terminal.has(next.job.phase)) requestedJob.value = null
    // Only reload after the worker's success record; validation may briefly expose the new version.
    if (!reloadStarted && next.job?.phase === 'succeeded' && next.current_version !== initialVersion) {
      reloadStarted = true
      window.location.reload()
    }
  } catch {
    disconnected.value = true
  } finally {
    polling = false
    if (active.value) {
      activeSince ??= Date.now()
      waitingTooLong.value = Date.now() - activeSince >= 180000
    } else {
      activeSince = undefined
      waitingTooLong.value = false
    }
    schedule()
  }
}

async function send(action: 'check' | 'apply') {
  sending.value = action
  requestError.value = ''
  try {
    const response = await adminRequest(`/update/${action}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-Model-Bridge-Update': '1' },
      body: JSON.stringify(action === 'apply' ? { version: candidate.value?.version } : {}),
      signal: AbortSignal.timeout(10000),
    })
    if (action === 'apply') requestedJob.value = (await response.json()).job_id
    await poll()
  } catch (error) {
    requestError.value = errorMessage(error, action === 'apply' ? '启动更新失败' : '检查更新失败')
  } finally {
    sending.value = ''
    schedule()
  }
}

watch(show, () => { if (show.value) void poll(); else schedule() })
onMounted(poll)
onBeforeUnmount(() => { disposed = true; clearTimeout(timer) })
</script>

<style scoped>
.update-version { display: inline-flex; align-items: center; gap: 5px; padding: 3px 5px; border: 1px solid transparent; border-radius: 5px; background: transparent; color: var(--mb-text-3); cursor: pointer; font-size: 11px; }
.update-version:hover, .update-version.available { color: var(--mb-primary); background: var(--mb-tint-blue); }
.update-version:focus-visible { outline: 2px solid var(--mb-primary); outline-offset: 2px; }
.update-dot { width: 5px; height: 5px; border-radius: 50%; background: var(--mb-primary); }
.update-note { margin: 0; color: var(--mb-text-2); font-size: 12px; }
.update-error, code { overflow-wrap: anywhere; }
a { color: var(--mb-primary); }
</style>
