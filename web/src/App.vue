<template>
  <n-config-provider :theme="null" :theme-overrides="themeOverrides" :locale="zhCN" :date-locale="dateZhCN">
    <n-message-provider>
      <div class="app-wrapper">
        <n-layout class="app-shell" has-sider>
          <n-layout-sider
            bordered
            :width="220"
            :collapsed-width="64"
            :collapsed="collapsed"
            :native-scrollbar="false"
            class="sidebar"
            collapse-mode="width"
          >
            <div class="sidebar-brand">
              <div class="brand-icon">
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16M4 12h16M4 17h10"/></svg>
              </div>
              <span v-show="!collapsed" class="brand-text serif">Model Bridge</span>
            </div>
            <n-menu
              :options="menuOptions"
              :value="currentRoute"
              :collapsed="collapsed"
              @update:value="navigate"
              class="sidebar-menu"
            />
            <div class="sidebar-footer">
              <span v-show="!collapsed" class="footer-version mono">v{{ appVersion }}</span>
              <div class="status-indicator">
                <span class="status-dot" />
              </div>
            </div>
          </n-layout-sider>

          <n-layout class="main-area">
            <n-layout-content>
              <div class="content">
                <router-view v-slot="{ Component }">
                  <transition name="page" mode="out-in">
                    <component :is="Component" />
                  </transition>
                </router-view>
              </div>
            </n-layout-content>
          </n-layout>
        </n-layout>
      </div>
    </n-message-provider>
  </n-config-provider>
</template>

<script setup lang="ts">
import { computed, h, ref, onMounted, onBeforeUnmount } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import {
  NConfigProvider, NLayout, NLayoutSider, NLayoutContent, NMenu, NMessageProvider,
  zhCN, dateZhCN, NIcon,
} from 'naive-ui'
import type { GlobalThemeOverrides } from 'naive-ui'

// 版本号来自运行中的二进制（GET /api/admin/settings 的 version = CARGO_PKG_VERSION），
// 不再 build 时注入——嵌入的 dist 无需随 Cargo.toml 重 build 即可显示正确版本。
const appVersion = ref('…')
onMounted(async () => {
  try {
    const res = await fetch('/api/admin/settings')
    if (res.ok) {
      const data = await res.json()
      if (data.version) appVersion.value = data.version
    }
  } catch { /* 取不到则保留占位 */ }
})

// 窄屏（≤1024px）自动折叠侧边栏为 64px 图标栏，省出横向空间。
// 用 matchMedia 精确到 1024；NaiveUI 默认断点无此刻度，自定义全局 breakpoints 反而更重。
const collapsed = ref(false)
let collapseMql: MediaQueryList | null = null
const syncCollapse = (e: MediaQueryList | MediaQueryListEvent) => { collapsed.value = e.matches }
onMounted(() => {
  collapseMql = window.matchMedia('(max-width: 1024px)')
  syncCollapse(collapseMql)
  collapseMql.addEventListener('change', syncCollapse)
})
onBeforeUnmount(() => collapseMql?.removeEventListener('change', syncCollapse))

const router = useRouter(); const route = useRoute()
const currentRoute = computed(() => route.path)

const DashboardIcon = () => h('svg', { xmlns: 'http://www.w3.org/2000/svg', width: '18', height: '18', viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '1.5', 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [
  h('path', { d: 'M3 3v18h18' }), h('polyline', { points: '7,14 11,9 14,12 19,6' }),
])
const ProviderIcon = () => h('svg', { xmlns: 'http://www.w3.org/2000/svg', width: '18', height: '18', viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '1.5', 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [
  h('rect', { x: 3, y: 4, width: 18, height: 6, rx: 1.5 }), h('rect', { x: 3, y: 14, width: 18, height: 6, rx: 1.5 }),
  h('circle', { cx: 7, cy: 7, r: 1 }), h('circle', { cx: 7, cy: 17, r: 1 }),
])
const KeyIcon = () => h('svg', { xmlns: 'http://www.w3.org/2000/svg', width: '18', height: '18', viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '1.5', 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [
  h('path', { d: 'M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777zm0 0L15.5 7.5m0 0l3 3L22 7l-3-3m-3.5 3.5L19 4' }),
])
const LogIcon = () => h('svg', { xmlns: 'http://www.w3.org/2000/svg', width: '18', height: '18', viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '1.5', 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [
  h('path', { d: 'M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z' }),
  h('polyline', { points: '14,2 14,8 20,8' }), h('line', { x1: 16, y1: 13, x2: 8, y2: 13 }), h('line', { x1: 16, y1: 17, x2: 8, y2: 17 }),
])
const HelpIcon = () => h('svg', { xmlns: 'http://www.w3.org/2000/svg', width: '18', height: '18', viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '1.5', 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [
  h('circle', { cx: '12', cy: '12', r: '10' }),
  h('path', { d: 'M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3' }),
  h('line', { x1: '12', y1: '17', x2: '12.01', y2: '17' }),
])
function renderIcon(icon: () => any) { return () => h(NIcon, null, { default: icon }) }
const menuOptions = [
  { label: '仪表盘', key: '/', icon: renderIcon(DashboardIcon) },
  { label: '供应商', key: '/providers', icon: renderIcon(ProviderIcon) },
  { label: 'API 密钥', key: '/api-keys', icon: renderIcon(KeyIcon) },
  { label: '日志', key: '/logs', icon: renderIcon(LogIcon) },
  { label: '接入指南', key: '/help', icon: renderIcon(HelpIcon) },
]
function navigate(key: string) { router.push(key) }

// --- AI SaaS · 浅底克制企业风 ---
const themeOverrides: GlobalThemeOverrides = {
  common: {
    fontFamily: "'Inter', -apple-system, BlinkMacSystemFont, sans-serif",
    fontFamilyMono: "'JetBrains Mono', monospace",
    fontSize: '14px',
    primaryColor: '#3B82F6',
    primaryColorHover: '#2563EB',
    primaryColorPressed: '#1D4ED8',
    primaryColorSuppl: '#3B82F6',
    infoColor: '#06B6D4',
    infoColorHover: '#0891B2',
    successColor: '#22C55E',
    successColorHover: '#16A34A',
    warningColor: '#F59E0B',
    errorColor: '#EF4444',
    bodyColor: '#F8FAFC',
    baseColor: '#FFFFFF',
    cardColor: '#FFFFFF',
    modalColor: '#FFFFFF',
    popoverColor: '#FFFFFF',
    tableColor: '#FFFFFF',
    borderColor: '#E2E8F0',
    dividerColor: '#F1F5F9',
    textColorBase: '#0F172A',
    textColor1: '#0F172A',
    textColor2: '#475569',
    textColor3: '#94A3B8',
    inputColor: '#FFFFFF',
    hoverColor: 'rgba(59, 130, 246, 0.06)',
    boxShadow1: '0 1px 2px rgba(15,23,42,0.04)',
    boxShadow2: '0 4px 16px rgba(15,23,42,0.06)',
    boxShadow3: '0 12px 40px rgba(15,23,42,0.08)',
    borderRadius: '14px',
    heightSmall: '30px',
    heightMedium: '34px',
    heightLarge: '40px',
  },
  Layout: { siderColor: '#FFFFFF', headerColor: '#F8FAFC', footerColor: '#FFFFFF' },
  Menu: {
    itemTextColor: '#475569', itemTextColorHover: '#0F172A', itemTextColorActive: '#3B82F6',
    itemIconColor: '#94A3B8', itemIconColorHover: '#0F172A', itemIconColorActive: '#3B82F6',
    itemColorHover: 'rgba(15,23,42,0.04)',
    itemColorActive: 'rgba(59,130,246,0.08)', itemColorActiveHover: 'rgba(59,130,246,0.12)',
    borderRadius: '8px',
  },
  Card: { borderRadius: '14px', borderColor: '#E2E8F0', paddingMedium: '22px', titleTextColor: '#0F172A', titleFontSize: '16px', titleFontWeight: '600', titleFontFamily: "'Inter', sans-serif" },
  Button: { borderRadiusSmall: '8px', borderRadiusMedium: '8px', borderRadiusLarge: '10px' },
  Input: { borderRadius: '8px', border: '1px solid #E2E8F0', borderFocus: '1px solid #3B82F6', borderHover: '1px solid #CBD5E1', boxShadowFocus: '0 0 0 3px rgba(59,130,246,0.12)' },
  Switch: { railColorActive: '#22C55E' },
  Table: { tdColor: '#FFFFFF', thColor: '#F8FAFC', thTextColor: '#475569', tdTextColor: '#0F172A', borderColor: '#F1F5F9', thFontWeight: '600', thFontSize: '11px', fontSizeSmall: '13px', borderRadius: '14px' },
  DataTable: { tdColor: '#FFFFFF', thColor: '#F8FAFC', thTextColor: '#475569', tdTextColor: '#0F172A', borderColor: '#F1F5F9' },
  Modal: { borderRadius: '16px', textColor: '#0F172A' },
  Tag: { borderRadius: '999px' },
  Pagination: { itemColor: '#FFFFFF', itemColorActive: 'rgba(59,130,246,0.06)', itemTextColorActive: '#3B82F6', itemBorder: '1px solid #E2E8F0', itemBorderActive: '1px solid #3B82F6', itemBorderRadius: '8px' },
  Slider: { fillColor: '#3B82F6', fillColorHover: '#2563EB', handleColor: '#3B82F6' },
  Spin: { color: '#3B82F6' },
}
</script>

<style>
*,*::before,*::after { box-sizing: border-box; }

body {
  margin: 0; padding: 0;
  background: #F8FAFC;
  color: #0F172A;
  font-family: 'Inter', -apple-system, BlinkMacSystemFont, sans-serif;
  -webkit-font-smoothing: antialiased; -moz-osx-font-smoothing: grayscale;
}

::-webkit-scrollbar { width: 9px; height: 9px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb { background: #CBD5E1; border-radius: 99px; border: 2px solid #F8FAFC; }
::-webkit-scrollbar-thumb:hover { background: #3B82F6; }

.app-wrapper {
  max-width: 1440px; margin: 0 auto;
  background: #F8FAFC; min-height: 100vh;
  position: relative; z-index: 1;
  box-shadow: 0 0 60px rgba(15,23,42,0.04);
}
.app-shell { height: 100vh; position: relative; z-index: 1; }
.main-area { background: transparent !important; overflow-y: auto; }

.sidebar { background: #FFFFFF !important; border-right: 1px solid #E2E8F0 !important; position: relative; }
.sidebar-brand { display: flex; align-items: center; gap: 10px; padding: 22px 18px; border-bottom: 1px solid #E2E8F0; }
.brand-icon { width: 34px; height: 34px; border-radius: 9px; display: flex; align-items: center; justify-content: center; flex-shrink: 0; background: linear-gradient(135deg, #3B82F6, #06B6D4 50%, #22C55E); color: #FFFFFF; }
.brand-text { font-size: 18px; font-weight: 600; color: #0F172A; letter-spacing: -0.01em; white-space: nowrap; }
.sidebar-menu { padding: 8px 8px 48px 8px; background: transparent !important; }
.sidebar-menu .n-menu-item-content { font-family: 'Inter', sans-serif; font-size: 13px; font-weight: 500; }
.sidebar-footer { position: absolute; bottom: 0; left: 0; right: 0; display: flex; justify-content: space-between; align-items: center; padding: 14px 18px; border-top: 1px solid #E2E8F0; background: #FFFFFF; }
.footer-version { font-size: 11px; color: #94A3B8; letter-spacing: 0.02em; }
.status-indicator { display: flex; align-items: center; gap: 6px; }
.status-dot { width: 7px; height: 7px; border-radius: 50%; background: #22C55E; box-shadow: 0 0 6px rgba(34,197,94,0.4); animation: status-pulse 3s ease-in-out infinite; }
@keyframes status-pulse { 0%, 100% { box-shadow: 0 0 6px rgba(34,197,94,0.4); } 50% { box-shadow: 0 0 12px rgba(34,197,94,0.6); } }
.status-label { font-size: 12px; color: #94A3B8; letter-spacing: 0.02em; }

.content { padding: 28px 32px; max-width: 1280px; position: relative; z-index: 1; }

.page-enter-active { transition: opacity 0.25s ease, transform 0.25s ease; }
.page-leave-active { transition: opacity 0.15s ease, transform 0.15s ease; }
.page-enter-from { opacity: 0; transform: translateY(8px); }
.page-leave-to { opacity: 0; transform: translateY(-8px); }

.mono { font-family: 'JetBrains Mono', monospace; }
.serif { font-family: 'Inter', -apple-system, BlinkMacSystemFont, sans-serif; }
.n-layout-sider-scroll-container { height: 100%; }
</style>