<template>
  <n-tooltip placement="top">
    <template #trigger>
      <button
        type="button"
        role="switch"
        aria-label="Debug 日志"
        :aria-checked="enabled"
        :aria-busy="busy"
        :disabled="busy"
        :class="['debug-switch', { active: enabled }]"
        @click="toggle"
      >DBG</button>
    </template>
    <div>{{ ready ? `Debug 日志：${enabled ? '已开启' : '已关闭'}` : 'Debug 日志：点击重试读取状态' }}</div>
    <div>即时生效，服务重启后自动关闭。</div>
    <div>开启后保存完整请求内容，不含认证头。历史文件需手动清理。</div>
    <div v-if="directory">保存目录：{{ directory }}</div>
  </n-tooltip>
</template>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { NTooltip, useMessage } from 'naive-ui'
import { adminRequest, errorMessage } from '../api'

const message = useMessage()
const enabled = ref(false)
const ready = ref(false)
const busy = ref(false)
const directory = ref('')

async function load() {
  busy.value = true
  try {
    const response = await adminRequest('/settings')
    const settings = await response.json()
    enabled.value = settings.request_log_enabled
    directory.value = settings.request_log_dir
    ready.value = true
  } catch (error) {
    message.error(errorMessage(error, '读取 Debug 日志状态失败'))
  } finally {
    busy.value = false
  }
}

async function toggle() {
  if (!ready.value) return load()
  busy.value = true
  try {
    const response = await adminRequest('/settings', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ request_log_enabled: !enabled.value }),
    })
    const settings = await response.json()
    enabled.value = settings.request_log_enabled
    message.success(enabled.value ? 'Debug 日志已开启，服务重启后自动关闭' : 'Debug 日志已关闭')
  } catch (error) {
    message.error(errorMessage(error, '切换 Debug 日志失败'))
  } finally {
    busy.value = false
  }
}

onMounted(load)
</script>

<style scoped>
.debug-switch {
  border: 1px solid var(--mb-border);
  border-radius: 5px;
  padding: 3px 5px;
  background: transparent;
  color: var(--mb-text-3);
  font: 10px monospace;
  cursor: pointer;
}
.debug-switch.active { color: var(--mb-primary); background: var(--mb-tint-blue); border-color: var(--mb-primary); }
.debug-switch:disabled { cursor: wait; opacity: 0.5; }
.debug-switch:focus-visible { outline: 2px solid var(--mb-primary); outline-offset: 2px; }
</style>
