import { ref, computed, watchEffect } from 'vue'

// 主题模式：light / dark / auto（auto 跟随系统 prefers-color-scheme）。
// 持久化在 localStorage，默认 auto。html.dark 类由本模块的 effect 同步设置，
// 早于 Vue 首次渲染，避免暗色用户的亮色闪烁（FOUC）。

export type ThemeMode = 'light' | 'dark' | 'auto'

const STORAGE_KEY = 'mb-theme'
const MEDIA_QUERY = '(prefers-color-scheme: dark)'

function readStoredMode(): ThemeMode {
  if (typeof localStorage === 'undefined') return 'auto'
  const v = localStorage.getItem(STORAGE_KEY)
  return v === 'light' || v === 'dark' || v === 'auto' ? v : 'auto'
}

function readSystemDark(): boolean {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  return window.matchMedia(MEDIA_QUERY).matches
}

export const mode = ref<ThemeMode>(readStoredMode())
const systemDark = ref<boolean>(readSystemDark())

// 监听系统配色变化（仅 auto 模式下影响 isDark）。
if (typeof window !== 'undefined' && window.matchMedia) {
  const mql = window.matchMedia(MEDIA_QUERY)
  const onChange = (e: MediaQueryListEvent) => { systemDark.value = e.matches }
  mql.addEventListener('change', onChange)
}

// 实际生效的主题：显式选择优先，否则跟随系统。
export const isDark = computed<boolean>(() =>
  mode.value === 'dark' || (mode.value === 'auto' && systemDark.value)
)

export function setMode(m: ThemeMode): void {
  mode.value = m
  try { localStorage.setItem(STORAGE_KEY, m) } catch { /* 无痕模式等忽略 */ }
}

// 折叠态单按钮循环：light → dark → auto → light。
export function cycleMode(): void {
  const order: ThemeMode[] = ['light', 'dark', 'auto']
  const next = order[(order.indexOf(mode.value) + 1) % order.length]
  setMode(next)
}

// 同步 html.dark + color-scheme，让 CSS 变量与原生表单控件一起翻转。
watchEffect(() => {
  if (typeof document === 'undefined') return
  const el = document.documentElement
  el.classList.toggle('dark', isDark.value)
  el.style.colorScheme = isDark.value ? 'dark' : 'light'
})
