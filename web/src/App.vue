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
              <span class="footer-version mono">v0.1.0</span>
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
function renderIcon(icon: () => any) { return () => h(NIcon, null, { default: icon }) }
const menuOptions = [
  { label: '仪表盘', key: '/', icon: renderIcon(DashboardIcon) },
  { label: '供应商', key: '/providers', icon: renderIcon(ProviderIcon) },
  { label: 'API 密钥', key: '/api-keys', icon: renderIcon(KeyIcon) },
  { label: '日志', key: '/logs', icon: renderIcon(LogIcon) },
]
function navigate(key: string) { router.push(key) }

// --- Soft Laboratory theme ---
const themeOverrides: GlobalThemeOverrides = {
  common: {
    fontFamily: "'Figtree', -apple-system, BlinkMacSystemFont, sans-serif",
    fontFamilyMono: "'Source Code Pro', 'Fira Code', monospace",
    fontSize: '14px',
    primaryColor: '#2d6a4f',
    primaryColorHover: '#40916c',
    primaryColorPressed: '#1b4332',
    primaryColorSuppl: '#2d6a4f',
    infoColor: '#52796f',
    infoColorHover: '#6b9080',
    successColor: '#2d6a4f',
    warningColor: '#e6b85c',
    errorColor: '#c4736e',
    bodyColor: '#f5f1eb',
    baseColor: '#f5f1eb',
    cardColor: '#ffffff',
    modalColor: '#ffffff',
    popoverColor: '#ffffff',
    tableColor: '#ffffff',
    borderColor: '#e0dcd5',
    dividerColor: '#e0dcd5',
    textColorBase: '#2d2d2a',
    textColor1: '#2d2d2a',
    textColor2: '#787870',
    textColor3: '#a0a098',
    inputColor: '#faf8f4',
    hoverColor: 'rgba(45, 106, 79, 0.04)',
    boxShadow1: '0 1px 3px rgba(0,0,0,0.04)',
    boxShadow2: '0 4px 20px rgba(0,0,0,0.06)',
    boxShadow3: '0 10px 40px rgba(0,0,0,0.08)',
    borderRadius: '12px',
    heightSmall: '30px',
    heightMedium: '34px',
    heightLarge: '40px',
  },
  Layout: { siderColor: '#f0ebe0', headerColor: '#f0ebe0', footerColor: '#f0ebe0' },
  Menu: {
    itemTextColor: '#787870', itemTextColorHover: '#2d6a4f', itemTextColorActive: '#2d6a4f',
    itemIconColor: '#a0a098', itemIconColorHover: '#2d6a4f', itemIconColorActive: '#2d6a4f',
    itemColorHover: 'rgba(45, 106, 79, 0.06)', itemColorActive: 'rgba(45, 106, 79, 0.08)', itemColorActiveHover: 'rgba(45, 106, 79, 0.1)',
    borderRadius: '10px',
  },
  Card: { borderRadius: '16px', borderColor: '#e0dcd5', paddingMedium: '24px', titleTextColor: '#2d2d2a', titleFontSize: '15px', titleFontWeight: '600' },
  Button: { borderRadiusSmall: '8px', borderRadiusMedium: '10px', borderRadiusLarge: '12px' },
  Input: { borderRadius: '10px', border: '1px solid #e0dcd5', borderFocus: '1px solid #2d6a4f', borderHover: '1px solid #c5c0b8', boxShadowFocus: '0 0 0 3px rgba(45, 106, 79, 0.1)' },
  Switch: { railColorActive: '#2d6a4f', railHeight: '22px', railWidth: '40px', buttonWidth: '18px', buttonHeight: '18px', borderRadius: '11px' },
  Table: { tdColor: '#ffffff', thColor: '#faf8f4', thTextColor: '#787870', tdTextColor: '#2d2d2a', borderColor: '#e0dcd5', thFontWeight: '600', thFontSize: '12px', fontSizeSmall: '13px', borderRadius: '12px' },
  DataTable: { tdColor: '#ffffff', thColor: '#faf8f4', thTextColor: '#787870', tdTextColor: '#2d2d2a', borderColor: '#e0dcd5' },
  Modal: { borderRadius: '16px', textColor: '#2d2d2a' },
  Tag: { borderRadius: '8px' },
  Pagination: { itemColor: '#ffffff', itemColorActive: 'rgba(45, 106, 79, 0.08)', itemTextColorActive: '#2d6a4f', itemBorder: '1px solid #e0dcd5', itemBorderActive: '1px solid rgba(45, 106, 79, 0.25)', itemBorderRadius: '8px' },
  Slider: { fillColor: '#2d6a4f', fillColorHover: '#40916c' },
  Spin: { color: '#2d6a4f' },
}
</script>

<style>
*,*::before,*::after { box-sizing: border-box; }

body {
  margin: 0; padding: 0;
  background: #ebe5d9;
  color: #2d2d2a;
  font-family: 'Figtree', -apple-system, BlinkMacSystemFont, sans-serif;
  -webkit-font-smoothing: antialiased; -moz-osx-font-smoothing: grayscale;
}

body::before {
  content: '';
  position: fixed; inset: 0;
  background: repeating-linear-gradient(0deg, transparent, transparent 2px, rgba(45, 106, 79, 0.006) 2px, rgba(45, 106, 79, 0.006) 4px);
  pointer-events: none; z-index: 0;
}

::-webkit-scrollbar { width: 6px; height: 6px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb { background: #d5d0c8; border-radius: 3px; }
::-webkit-scrollbar-thumb:hover { background: #2d6a4f; }

.app-wrapper {
  max-width: 1440px; margin: 0 auto;
  background: #f5f1eb; min-height: 100vh;
  position: relative; z-index: 1;
  box-shadow: 0 0 60px rgba(0,0,0,0.04);
}
.app-shell { height: 100vh; position: relative; z-index: 1; }
.main-area { background: transparent !important; overflow-y: auto; }

.sidebar { background: #f0ebe0 !important; border-right: 1px solid #e0dcd5 !important; position: relative; }
.sidebar-brand { display: flex; align-items: center; gap: 10px; padding: 22px 18px; border-bottom: 1px solid #e0dcd5; }
.brand-icon { color: #2d6a4f; display: flex; align-items: center; flex-shrink: 0; }
.brand-text { font-size: 18px; font-weight: 600; color: #2d2d2a; letter-spacing: -0.01em; white-space: nowrap; }
.sidebar-menu { padding: 8px 8px 48px 8px; background: transparent !important; }
.sidebar-menu .n-menu-item-content { font-family: 'Figtree', sans-serif; font-size: 13px; font-weight: 500; }
.sidebar-footer { position: absolute; bottom: 0; left: 0; right: 0; display: flex; justify-content: space-between; align-items: center; padding: 14px 18px; border-top: 1px solid #e0dcd5; background: #f0ebe0; }
.footer-version { font-size: 11px; color: #a0a098; letter-spacing: 0.02em; }
.status-indicator { display: flex; align-items: center; gap: 6px; }
.status-dot { width: 7px; height: 7px; border-radius: 50%; background: #2d6a4f; box-shadow: 0 0 6px rgba(45, 106, 79, 0.3); animation: status-pulse 3s ease-in-out infinite; }
@keyframes status-pulse { 0%, 100% { box-shadow: 0 0 6px rgba(45, 106, 79, 0.3); } 50% { box-shadow: 0 0 12px rgba(45, 106, 79, 0.5); } }
.status-label { font-size: 12px; color: #a0a098; letter-spacing: 0.02em; }

.content { padding: 28px 32px; max-width: 1280px; position: relative; z-index: 1; }

.page-enter-active { transition: opacity 0.25s ease, transform 0.25s ease; }
.page-leave-active { transition: opacity 0.15s ease, transform 0.15s ease; }
.page-enter-from { opacity: 0; transform: translateY(8px); }
.page-leave-to { opacity: 0; transform: translateY(-8px); }

.mono { font-family: 'Source Code Pro', monospace; }
.serif { font-family: 'Newsreader', 'Georgia', serif; }
.n-layout-sider-scroll-container {
  height: 100%;
}
</style>