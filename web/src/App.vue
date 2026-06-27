<template>
  <n-config-provider :theme="null" :theme-overrides="themeOverrides" :locale="zhCN" :date-locale="dateZhCN">
    <n-message-provider>
      <div class="app-wrapper">
        <n-layout class="app-shell" has-sider>
          <n-layout-sider
            bordered
            :width="220"
            :collapsed-width="64"
            :native-scrollbar="false"
            class="sidebar"
            collapse-mode="width"
          >
            <div class="sidebar-brand">
              <div class="brand-icon">
                <svg width="28" height="28" viewBox="0 0 28 28" fill="none">
                  <rect x="3" y="3" width="22" height="22" rx="7" stroke="currentColor" stroke-width="1.5" />
                  <path d="M9 10h4M9 14h8M9 18h6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
                  <circle cx="20" cy="14" r="3.5" stroke="currentColor" stroke-width="1.2" />
                  <circle cx="20" cy="14" r="1.2" fill="currentColor" />
                </svg>
              </div>
              <span class="brand-text serif">Model Bridge</span>
            </div>
            <n-menu
              :options="menuOptions"
              :value="currentRoute"
              @update:value="navigate"
              class="sidebar-menu"
            />
            <div class="sidebar-footer">
              <span class="footer-version mono">v{{ appVersion }}</span>
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
import { computed, h } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import {
  NConfigProvider, NLayout, NLayoutSider, NLayoutContent, NMenu, NMessageProvider,
  zhCN, dateZhCN, NIcon,
} from 'naive-ui'
import type { GlobalThemeOverrides } from 'naive-ui'

// Build-time version (Vite `define`, sourced from Cargo.toml). Must be bound
// here in <script setup>, not referenced directly in the template — the SFC
// compiler rewrites bare template identifiers into render-context property
// accesses (e.g. h.__APP_VERSION__), which Vite's define cannot replace.
const appVersion = __APP_VERSION__

const router = useRouter(); const route = useRoute()
const currentRoute = computed(() => route.path)

const DashboardIcon = () => h('svg', { xmlns: 'http://www.w3.org/2000/svg', width: '18', height: '18', viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '1.5', 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [
  h('rect', { x: 3, y: 3, width: 7, height: 7, rx: 1.5 }), h('rect', { x: 14, y: 3, width: 7, height: 7, rx: 1.5 }),
  h('rect', { x: 3, y: 14, width: 7, height: 7, rx: 1.5 }), h('rect', { x: 14, y: 14, width: 7, height: 7, rx: 1.5 }),
])
const ProviderIcon = () => h('svg', { xmlns: 'http://www.w3.org/2000/svg', width: '18', height: '18', viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': '1.5', 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }, [
  h('path', { d: 'M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h6v6h-6z' }),
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

// --- Editorial Ink + Fresh Data theme ---
const themeOverrides: GlobalThemeOverrides = {
  common: {
    fontFamily: "'IBM Plex Mono', monospace",
    fontFamilyMono: "'IBM Plex Mono', monospace",
    fontSize: '14px',
    primaryColor: '#17140f',
    primaryColorHover: '#2a2620',
    primaryColorPressed: '#000000',
    primaryColorSuppl: '#17140f',
    infoColor: '#159b96',
    infoColorHover: '#1aab9f',
    successColor: '#2ea86a',
    successColorHover: '#36bb78',
    warningColor: '#b5842b',
    errorColor: '#b3261e',
    bodyColor: '#faf7f0',
    baseColor: '#faf7f0',
    cardColor: '#ffffff',
    modalColor: '#ffffff',
    popoverColor: '#ffffff',
    tableColor: '#ffffff',
    borderColor: '#d9cfbf',
    dividerColor: '#ece6da',
    textColorBase: '#17140f',
    textColor1: '#17140f',
    textColor2: '#74695a',
    textColor3: '#a89e8c',
    inputColor: '#ffffff',
    hoverColor: 'rgba(23, 20, 15, 0.04)',
    boxShadow1: '0 1px 2px rgba(23,20,15,0.05)',
    boxShadow2: '0 4px 16px rgba(23,20,15,0.06)',
    boxShadow3: '0 12px 40px rgba(23,20,15,0.08)',
    borderRadius: '10px',
    heightSmall: '30px',
    heightMedium: '34px',
    heightLarge: '40px',
  },
  Layout: { siderColor: '#f4efe3', headerColor: '#faf7f0', footerColor: '#f4efe3' },
  Menu: {
    itemTextColor: '#74695a', itemTextColorHover: '#17140f', itemTextColorActive: '#1d7a4c',
    itemIconColor: '#a89e8c', itemIconColorHover: '#17140f', itemIconColorActive: '#1d7a4c',
    itemColorHover: 'rgba(23,20,15,0.04)',
    itemColorActive: 'rgba(46,168,106,0.10)', itemColorActiveHover: 'rgba(46,168,106,0.14)',
    borderRadius: '8px',
  },
  Card: { borderRadius: '12px', borderColor: '#d9cfbf', paddingMedium: '22px', titleTextColor: '#17140f', titleFontSize: '16px', titleFontWeight: '600', titleFontFamily: "'Fraunces', serif" },
  Button: { borderRadiusSmall: '8px', borderRadiusMedium: '8px', borderRadiusLarge: '10px' },
  Input: { borderRadius: '8px', border: '1px solid #d9cfbf', borderFocus: '1px solid #2ea86a', borderHover: '1px solid #bfb29a', boxShadowFocus: '0 0 0 3px rgba(46,168,106,0.12)' },
  Switch: { railColorActive: '#2ea86a' },
  Table: { tdColor: '#ffffff', thColor: '#f4efe3', thTextColor: '#74695a', tdTextColor: '#17140f', borderColor: '#ece6da', thFontWeight: '600', thFontSize: '11px', fontSizeSmall: '13px', borderRadius: '12px' },
  DataTable: { tdColor: '#ffffff', thColor: '#f4efe3', thTextColor: '#74695a', tdTextColor: '#17140f', borderColor: '#ece6da' },
  Modal: { borderRadius: '14px', textColor: '#17140f' },
  Tag: { borderRadius: '999px' },
  Pagination: { itemColor: '#ffffff', itemColorActive: 'rgba(23,20,15,0.06)', itemTextColorActive: '#17140f', itemBorder: '1px solid #d9cfbf', itemBorderActive: '1px solid #17140f', itemBorderRadius: '8px' },
  Slider: { fillColor: '#17140f', fillColorHover: '#2a2620', handleColor: '#17140f' },
  Spin: { color: '#1d7a4c' },
}
</script>

<style>
*,*::before,*::after { box-sizing: border-box; }

body {
  margin: 0; padding: 0;
  background: #e7e3d7;
  color: #17140f;
  font-family: 'IBM Plex Mono', -apple-system, BlinkMacSystemFont, monospace;
  -webkit-font-smoothing: antialiased; -moz-osx-font-smoothing: grayscale;
}

::-webkit-scrollbar { width: 6px; height: 6px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb { background: #c9c0b0; border-radius: 3px; }
::-webkit-scrollbar-thumb:hover { background: #1d7a4c; }

.app-wrapper {
  max-width: 1440px; margin: 0 auto;
  background: #faf7f0; min-height: 100vh;
  position: relative; z-index: 1;
  box-shadow: 0 0 60px rgba(23,20,15,0.06);
}
.app-shell { height: 100vh; position: relative; z-index: 1; }
.main-area { background: transparent !important; overflow-y: auto; }

.sidebar { background: #f4efe3 !important; border-right: 1px solid #d9cfbf !important; position: relative; }
.sidebar-brand { display: flex; align-items: center; gap: 10px; padding: 22px 18px; border-bottom: 1px solid #d9cfbf; }
.brand-icon { color: #17140f; display: flex; align-items: center; flex-shrink: 0; }
.brand-text { font-size: 18px; font-weight: 600; color: #17140f; letter-spacing: -0.01em; white-space: nowrap; }
.sidebar-menu { padding: 8px 8px 48px 8px; background: transparent !important; }
.sidebar-menu .n-menu-item-content { font-family: 'IBM Plex Mono', monospace; font-size: 13px; font-weight: 500; }
.sidebar-footer { position: absolute; bottom: 0; left: 0; right: 0; display: flex; justify-content: space-between; align-items: center; padding: 14px 18px; border-top: 1px solid #d9cfbf; background: #f4efe3; }
.footer-version { font-size: 11px; color: #a89e8c; letter-spacing: 0.02em; }
.status-indicator { display: flex; align-items: center; gap: 6px; }
.status-dot { width: 7px; height: 7px; border-radius: 50%; background: #2ea86a; box-shadow: 0 0 6px rgba(46,168,106,0.4); animation: status-pulse 3s ease-in-out infinite; }
@keyframes status-pulse { 0%, 100% { box-shadow: 0 0 6px rgba(46,168,106,0.4); } 50% { box-shadow: 0 0 12px rgba(46,168,106,0.6); } }
.status-label { font-size: 12px; color: #a89e8c; letter-spacing: 0.02em; }

.content { padding: 28px 32px; max-width: 1280px; position: relative; z-index: 1; }

.page-enter-active { transition: opacity 0.25s ease, transform 0.25s ease; }
.page-leave-active { transition: opacity 0.15s ease, transform 0.15s ease; }
.page-enter-from { opacity: 0; transform: translateY(8px); }
.page-leave-to { opacity: 0; transform: translateY(-8px); }

.mono { font-family: 'IBM Plex Mono', monospace; }
.serif { font-family: 'Fraunces', 'Georgia', serif; }
.n-layout-sider-scroll-container { height: 100%; }
</style>