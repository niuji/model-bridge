# AI SaaS 主题重写 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `web/src/` 现有「Editorial Ink + Fresh Data」主题整体替换为 AI SaaS 浅底克制企业风（白/浅灰底 + 蓝→青→绿渐变 + Inter + JetBrains Mono + 卡片化大留白）。

**Architecture:** App.vue 的 Naive UI `themeOverrides` 是主杠杆——一处换肤覆盖全站 Naive 组件；字体经 `index.html` + 全局 `.serif/.mono` 重定义集中迁移；ECharts 经 Dashboard 顶部 `CHART` 常量集中调色；各 view scoped CSS 按「Editorial→AI SaaS」映射表机械替换。不改任何业务逻辑/接口/IA。

**Tech Stack:** Vue 3 + Naive UI 2 + ECharts 5 + Vite 6 + TypeScript；Rust（`include_dir!` 嵌入 `web/dist`）。

## Global Constraints

- **调色板（权威值）**：画布 `#F8FAFC` / 卡 `#FFFFFF` / 侧栏 `#FFFFFF` / 正文 `#0F172A` / 次 `#475569` / 弱 `#94A3B8` / 边框 `#E2E8F0` / 强边框 `#CBD5E1` / hover `#F1F5F9` / 蓝 `#3B82F6`(hover `#2563EB`, pressed `#1D4ED8`) / 青 `#06B6D4` / 绿 `#22C55E` / 深绿 `#16A34A` / 琥珀 `#F59E0B` / 错误 `#EF4444` / 渐变 `linear-gradient(135deg,#3B82F6,#06B6D4 50%,#22C55E)` / 渐变竖(柱) `linear-gradient(180deg,#3B82F6,#06B6D4 52%,#22C55E)` / tint：blue `#EFF6FF` cyan `#ECFEFF` green `#F0FDF4` amber `#FFFBEB` error `#FEF2F2` / 字体 Inter(400/500/600/700)+JetBrains Mono(400/500) / 卡片圆角 14px、按钮 8px、徽章 999px / 阴影 `0 1px 2px rgba(15,23,42,.04)`。
- **Editorial→AI SaaS 映射表（task 级权威，机械替换依据；同 spec §3）**：
  - `#17140f`→`#0F172A` · `#2a2620`→`#1E293B` · `#000000`→`#0F172A` · `#74695a`→`#475569` · `#a89e8c`/`#8c7f6c`→`#94A3B8` · `#5d564b`→`#475569` · `#4b443a`→`#334155`
  - `#d9cfbf`→`#E2E8F0` · `#c9c0b0`/`#bfb29a`→`#CBD5E1` · `#ece6da`→`#F1F5F9` · `#faf7f0`→`#F8FAFC`
  - `#f4efe3`→侧栏`#FFFFFF`/panel`#F1F5F9` · `#f9f5ec`/`#e5dccd`/`#e7e3d7`/`#ddd6c8`/`#d3ccc0`→`#F1F5F9`(面)或`#E2E8F0`(线,视是 bg/border)
  - `#fff7f5`/`#fcfaf6`/`#f7f2e8`/`#f1ece0`/`#eef7f0`/`#f4fbf6`→`#FFFFFF`/`#F8FAFC`(近白)
  - `#1d7a4c`→`#16A34A` · `#2ea86a`→`#22C55E` · `#bfe6cf`/`#cbe6d3`/`#dcefe1`→green-tint`#F0FDF4` · `#8dd8ae`/`#5bbf8a`→`#86EFAC`(图表浅绿端)
  - `#0d6e6b`/`#159b96`/`#1aab9f`→`#06B6D4` · `#c2e8e6`→cyan-tint`#ECFEFF`
  - `#9c6c00`/`#b5842b`→`#F59E0B` · `#f6e6b6`→amber-tint`#FFFBEB`
  - `#b3261e`/`#9f2019`/`#851a15`→`#EF4444`
  - **保留原值**：`#D97757`(Claude)/`#2A84EE`(Copilot)/`#000000`(Cursor) 在 Help.vue 的 brand SVG `fill`——身份色，不换。
- **字体迁移集中化**：`index.html` 换 Google Fonts 为 Inter+JetBrains Mono；App.vue 全局 `<style>` 把 `.serif` 重定义为 `font-family:'Inter'...`、`.mono` 重定义为 `'JetBrains Mono'`——全站 `class="serif"/"mono"` 即刻生效，**不逐 view 改 class**。
- **验证方法**：前端无测试框架，reskin 无行为变更 → 每 task 用 `cd web && npm run build` 通过(vue-tsc+Vite)为 gate；末 task 加 `cargo build`+`cargo run` 视觉确认+`cargo test` 回归。
- **提交规则**（repo CLAUDE.md）：**未经用户明确要求不 commit/push**。每 task 用 `git add` 暂存待审；提交是末尾单独一步、用户许可后执行，且提交前先开分支（当前在 main）。
- **范围纪律**：只改视觉（CSS/theme/字体/色值/圆角/阴影/渐变）；不动 template 结构（除非 task 明确要求，如品牌 SVG 与统计图标 class）、不动 `<script>` 业务逻辑、不动 ECharts 的图表类型/数据/option 结构（只换色值与 fontFamily）。

## File Structure

| 文件 | 责任 | 改动类型 |
|---|---|---|
| `web/index.html` | 字体加载 | 换 `<link>` |
| `web/src/App.vue` | 全局 shell + Naive UI themeOverrides + 全局样式 + 品牌 SVG | 改 `<script>` themeOverrides + `<style>` + 品牌 SVG |
| `web/src/views/Dashboard.vue` | 仪表盘 + ECharts | 加 `CHART` 常量；改 ECharts option 色值；改 stat-icon class；scoped CSS |
| `web/src/views/Providers.vue` | 供应商卡片/弹窗 | scoped CSS 机械映射 |
| `web/src/views/Logs.vue` | 日志表 + 延迟条 | scoped CSS + 延迟 tier 语义色 |
| `web/src/views/ApiKeys.vue` | 密钥表 | scoped CSS 机械映射 |
| `web/src/views/Help.vue` | 接入指南 + 工具卡 | scoped CSS 映射（保留 brand SVG fill） |

---

### Task 1: Shell 换肤（index.html + App.vue themeOverrides + 全局样式 + 品牌标）

**Files:**
- Modify: `web/index.html`（字体 `<link>`）
- Modify: `web/src/App.vue`（`themeOverrides` 对象 + 全局 `<style>` + 品牌 SVG）

**Interfaces:**
- Produces: 全站 Naive UI 组件经 `themeOverrides` 即刻换肤（card/table/button/input/menu/modal…），全局 `.serif`→Inter、`.mono`→JetBrains Mono、背景/侧栏/品牌/状态点切 AI SaaS 调色板。后续各 view task 继承此基线，只补 scoped 残留色。

- [ ] **Step 1: 换 index.html 字体**

把 `<link href="...Fraunces...IBM+Plex+Mono...">` 替换为 Inter + JetBrains Mono。`<head>` 内该行改为：

```html
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet" />
```

（preconnect 两行保留不动。）

- [ ] **Step 2: 重写 App.vue `themeOverrides` 对象**

把 `<script setup>` 里的 `const themeOverrides: GlobalThemeOverrides = { ... }`（约 125–181 行）整体替换为：

```ts
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
```

- [ ] **Step 3: 品牌标换渐变方块 + 3-bar 图标**

template 里 `.sidebar-brand` 内的 `<div class="brand-icon">` 的 SVG 整体替换为 mockup 的 3-bar 标（白色描边，靠父 `color:#fff`）：

```html
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16M4 12h16M4 17h10"/></svg>
```

- [ ] **Step 4: 重写 App.vue 全局 `<style>`**

整体替换 `<style>`（约 184–232 行）为：

```css
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
.brand-icon { width: 34px; height: 34px; border-radius: 9px; display: flex; align-items: center; justify-content: center; flex-shrink: 0;
  background: linear-gradient(135deg, #3B82F6, #06B6D4 50%, #22C55E); color: #FFFFFF; }
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
```

> `.serif` 重定义为 Inter → 全站 `class="serif"`（各 view 的 page-title/card-name 等）即刻变 Inter，无需逐 view 改 class。

- [ ] **Step 5: 构建验证**

Run: `cd /home/niuji/projects/model-bridge/web && npm run build`
Expected: `vue-tsc` 无类型错误、Vite 构建成功、`dist/` 产出。若报错，多半是 SVG/template 语法——修正后重跑。

- [ ] **Step 6: 暂存待审（不提交）**

```bash
cd /home/niuji/projects/model-bridge && git add web/index.html web/src/App.vue
```

**视觉自检点**（起全服务后）：侧栏白底、品牌渐变方块、导航激活蓝、状态点绿、整体浅灰底白卡。Naive 组件（按钮/输入/表格）应已自动蓝调。

---

### Task 2: Dashboard.vue（ECharts 集中调色 + stat-icon 重排 + scoped CSS）

**Files:**
- Modify: `web/src/views/Dashboard.vue`（顶部加 `CHART` 常量；`hourlyOption`/`heatmapOption` 色值引用之；`modelColumns` render 内联色；stat-icon class 重排；scoped CSS 映射）

**Interfaces:**
- Consumes: Task 1 的 Naive UI 基线（表格/卡片已蓝调）。
- Produces: 仪表盘图表蓝→青→绿 ramp，统计图标 蓝(总请求)/青(输入)/绿(输出)/琥珀(延迟)。

- [ ] **Step 1: 在 `<script setup>` 顶部加 `CHART` 常量**

在 `import` 之后、`const API_BASE` 之前插入：

```ts
// AI SaaS 图表调色（集中定义，ECharts option 引用之）
const CHART = {
  ink: '#0F172A', text2: '#475569', text3: '#94A3B8',
  border: '#E2E8F0', divider: '#F1F5F9', canvas: '#F8FAFC',
  blue: '#3B82F6', cyan: '#06B6D4', green: '#22C55E', greenD: '#16A34A',
  ramp: ['#F1F5F9', '#BFDBFE', '#67E8F9', '#4ADE80', '#16A34A'],
  font: 'JetBrains Mono, monospace',
}
```

- [ ] **Step 2: 重写 `hourlyOption`（柱图）色值引用 CHART**

把 `const hourlyOption = ref({...})` 整体替换为（结构与原版一致，仅色值/fontFamily 换）：

```ts
const hourlyOption = ref({
  backgroundColor: 'transparent',
  tooltip: { trigger: 'axis' as const, backgroundColor: '#FFFFFF', borderColor: CHART.border, textStyle: { color: CHART.ink, fontFamily: CHART.font, fontSize: 12 }, extraCssText: 'border-radius: 8px; box-shadow: 0 1px 2px rgba(15,23,42,0.04);' },
  grid: { left: 85, right: 24, bottom: 90, top: 30, borderColor: CHART.border },
  xAxis: { type: 'category' as const, data: [] as string[], axisLabel: { rotate: 45, fontSize: 10, fontFamily: CHART.font, color: CHART.text2 }, axisLine: { lineStyle: { color: CHART.border } }, axisTick: { lineStyle: { color: CHART.border } } },
  yAxis: { type: 'value' as const, name: 'Token', nameLocation: 'middle' as const, nameGap: 40, nameTextStyle: { color: CHART.text2, fontFamily: CHART.font, fontSize: 11 }, axisLabel: { fontFamily: CHART.font, fontSize: 10, color: CHART.text2, formatter: (v: number) => formatNum(v) }, splitLine: { lineStyle: { color: CHART.divider, type: 'dashed' as const } } },
  dataZoom: [{ type: 'slider' as const, start: 0, end: 100, height: 20, bottom: 12, backgroundColor: CHART.canvas, borderColor: CHART.border, borderRadius: 8, dataBackground: { lineStyle: { color: CHART.blue, opacity: 0.15 }, areaStyle: { color: CHART.blue, opacity: 0.04 } }, selectedDataBackground: { lineStyle: { color: CHART.blue }, areaStyle: { color: CHART.blue, opacity: 0.08 } }, handleStyle: { color: CHART.blue, borderRadius: 4 }, textStyle: { color: CHART.text2, fontFamily: CHART.font, fontSize: 10 } }],
  series: [{ name: 'Token 用量', type: 'bar' as const, data: [] as number[], barWidth: '60%', itemStyle: { borderRadius: [4, 4, 0, 0], color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [{ offset: 0, color: CHART.blue }, { offset: 0.52, color: CHART.cyan }, { offset: 1, color: CHART.green }]) }, emphasis: { itemStyle: { color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [{ offset: 0, color: '#2563EB' }, { offset: 0.52, color: CHART.cyan }, { offset: 1, color: CHART.greenD }]) } } }],
})
```

- [ ] **Step 3: 重写 `heatmapOption`（热力图）色值引用 CHART**

整体替换：

```ts
const heatmapOption = ref({
  backgroundColor: 'transparent',
  tooltip: { formatter: (p: any) => `${p.value[0]}<br/>Token: <b>${formatNum(p.value[1] as number)}</b>`, backgroundColor: '#FFFFFF', borderColor: CHART.border, textStyle: { color: CHART.ink, fontFamily: CHART.font, fontSize: 12 }, extraCssText: 'border-radius: 8px; box-shadow: 0 1px 2px rgba(15,23,42,0.04);' },
  visualMap: { min: 0, max: 1, type: 'continuous' as const, orient: 'vertical' as const, right: 10, top: 'middle', itemWidth: 10, itemHeight: 90, inRange: { color: CHART.ramp }, textStyle: { color: CHART.text2, fontFamily: CHART.font, fontSize: 9 }, formatter: (v: number) => formatNum(v) },
  calendar: { range: ['2026-01-01', '2026-01-31'], cellSize: [13, 13], left: 36, right: 56, top: 24, bottom: 8, orient: 'horizontal', itemStyle: { borderWidth: 2, borderColor: '#fff', color: '#FFFFFF' }, yearLabel: { show: false }, monthLabel: { nameMap: 'cn', color: CHART.text2, fontFamily: CHART.font, fontSize: 11, margin: 8 }, dayLabel: { firstDay: 1, nameMap: 'cn', color: CHART.text3, fontFamily: CHART.font, fontSize: 10 }, splitLine: { show: false } },
  series: [{ type: 'heatmap' as const, coordinateSystem: 'calendar', data: [] as any[] }],
})
```

- [ ] **Step 4: `modelColumns` 命中率 render 内联色**

把命中率列的 render 颜色 `#1d7a4c`→`CHART.greenD`、`#a89e8c`→`CHART.text3`：

```ts
  { title: '命中率', key: 'cache_hit_rate', width: 90, align: 'right', render: (row) => h('span', { class: 'mono', style: { color: row.cache_hit_rate > 0 ? CHART.greenD : CHART.text3, fontWeight: row.cache_hit_rate > 0 ? '600' : '400' } }, `${row.cache_hit_rate}%`) },
```

- [ ] **Step 5: 统计图标 class 重排（蓝/青/绿/琥珀）**

template 的 4 个 `.stat-icon` class 改为：
- 总请求数：`ico-g` → `ico-b`
- 输入 Token：`ico-a` → `ico-c`
- 输出 Token：`ico-t` → `ico-g`
- 平均延迟：`ico-g` → `ico-a`

（即 `.stat-icon.ico-g/.ico-a/.ico-t/.ico-l` 改名为 `.ico-b/.ico-c/.ico-g/.ico-a`，模板与下方 CSS 同步。）

- [ ] **Step 6: Dashboard scoped CSS 映射**

按 Global Constraints 映射表替换该文件 `<style scoped>` 内所有 editorial 色值。关键规则改后：

```css
.page-title { font-size: 28px; font-weight: 600; color: #0F172A; margin: 0; letter-spacing: -0.02em; }
.page-subtitle { margin: 6px 0 0; color: #94A3B8; font-size: 13px; }

.stat-card { background: #FFFFFF; border: 1px solid #E2E8F0; border-radius: 14px; padding: 22px; transition: box-shadow 0.2s, transform 0.2s; }
.stat-card:hover { box-shadow: 0 4px 16px rgba(15,23,42,0.06); transform: translateY(-1px); }
.stat-icon.ico-b { background: #EFF6FF; color: #3B82F6; }
.stat-icon.ico-c { background: #ECFEFF; color: #06B6D4; }
.stat-icon.ico-g { background: #F0FDF4; color: #16A34A; }
.stat-icon.ico-a { background: #FFFBEB; color: #F59E0B; }
.stat-label { font-size: 12px; font-weight: 500; color: #94A3B8; text-transform: uppercase; letter-spacing: 0.05em; margin-bottom: 6px; }
.stat-value { font-size: 28px; font-weight: 600; color: #0F172A; line-height: 1; }
.stat-unit { font-size: 16px; color: #94A3B8; font-weight: 400; margin-left: 3px; }

.card { background: #FFFFFF; border: 1px solid #E2E8F0; border-radius: 14px; overflow: hidden; }
.card-header { display: flex; justify-content: space-between; align-items: center; padding: 18px 24px; border-bottom: 1px solid #F1F5F9; }
.card-title { margin: 0; font-family: 'Inter', sans-serif; font-size: 17px; font-weight: 600; color: #0F172A; letter-spacing: -0.01em; }
.card-badge { font-size: 11px; color: #475569; padding: 4px 10px; background: #F8FAFC; border: 1px solid #E2E8F0; border-radius: 999px; }
.card-body { padding: 24px; }

.dashboard-table { --n-td-color: #FFFFFF; --n-th-color: #F8FAFC; }
.dashboard :deep(.model-id-cell) { font-size: 12px; color: #475569; }
.dashboard :deep(.token-cell) { color: #16A34A; font-weight: 500; }
```

（其余 editorial 色值按映射表机械替换；**删除原 `.ico-t`**——输出已改用 `.ico-g` 绿，`.ico-t` 不再引用。）

- [ ] **Step 7: 构建验证**

Run: `cd /home/niuji/projects/model-bridge/web && npm run build`
Expected: 通过。注意 `CHART` 引用的字段名与 option 内一致；`echarts.graphic.LinearGradient` 已 import（原文件 `import * as echarts from 'echarts/core'`）。

- [ ] **Step 8: 暂存待审**

```bash
cd /home/niuji/projects/model-bridge && git add web/src/views/Dashboard.vue
```

---

### Task 3: Providers.vue（scoped CSS 机械映射）

**Files:**
- Modify: `web/src/views/Providers.vue`（仅 `<style scoped>`，~95 处色值）

**Interfaces:**
- Consumes: Task 1 基线。供应商图标走 `<img src="/icons/...">`，与 CSS 色无关，不动。

- [ ] **Step 1: 通读并按映射表替换该文件 `<style scoped>` 全部 editorial 色值**

读取 `web/src/views/Providers.vue` 的 `<style scoped>` 全段，按 Global Constraints 映射表对每个 editorial hex 机械替换。要点：
- `.card-icon-fallback { color: #74695a }` → `#475569`
- `serif` 类（card-name 等）：无需改 class（Task 1 已重定义 `.serif`→Inter）；但若有内联 `font-family:'Fraunces'` → 改 `'Inter', sans-serif`。
- 卡片/弹窗背景 `#ffffff`→保留 `#FFFFFF`；边框 `#d9cfbf`→`#E2E8F0`；分隔 `#ece6da`→`#F1F5F9`。
- 状态点 `#2ea86a`→`#22C55E`；drift 徽章琥珀系 `#9c6c00`/`#b5842b`→`#F59E0B`（底用 amber-tint `#FFFBEB`）。
- `#b3261e` 错误 → `#EF4444`。
- 文字色 `#17140f`→`#0F172A`、`#74695a`→`#475569`、`#a89e8c`→`#94A3B8`、`#5d564b`→`#475569`。
- 暖面 `#f4efe3`/`#f9f5ec`/`#e5dccd` 等 → `#F1F5F9`（bg）或 `#E2E8F0`（border）视上下文。

逐段用 Edit 替换；`#ffffff`→`#FFFFFF` 仅大小写统一，可选（不影响渲染）。

- [ ] **Step 2: 构建验证**

Run: `cd /home/niuji/projects/model-bridge/web && npm run build`
Expected: 通过。

- [ ] **Step 3: 暂存待审**

```bash
cd /home/niuji/projects/model-bridge && git add web/src/views/Providers.vue
```

---

### Task 4: Logs.vue（scoped CSS + 延迟 tier 语义色）

**Files:**
- Modify: `web/src/views/Logs.vue`（`<style scoped>` + 延迟条 `tier-*` 语义色 + `:deep` 样式）

**Interfaces:**
- Consumes: Task 1 基线。

- [ ] **Step 1: 按映射表替换 `<style scoped>` editorial 色值**

读 `Logs.vue` 的 `<style>`，按映射表机械替换。要点：
- `.logs-table { --n-td-color: #ffffff; --n-th-color: #f4efe3; --n-td-color-hover: #f9f5ec }` → `#FFFFFF` / `#F8FAFC` / `#F1F5F9`
- empty 状态：`.empty-icon { background:#f4efe3; border:1px solid #ece6da; color:#c9c0b0 }` → bg `#F1F5F9`、border `#E2E8F0`、color `#94A3B8`
- `:deep(.time-date)` `#a89e8c`→`#94A3B8`；`:deep(.time-time)` `#5d564b`→`#475569`；`:deep(.caller-client-icon)` `#b3a98f`→`#94A3B8`；`:deep(.caller-client-product)` `#5d564b`→`#475569`
- page-title/subtitle/count：`#17140f`→`#0F172A`、`#a89e8c`→`#94A3B8`

- [ ] **Step 2: 延迟条 tier 语义色**

找到 `.latency-fill.tier-*`（fast/mid/slow 之类）规则，按语义映射：
- fast(绿) `#2ea86a`/`#1d7a4c` → `#22C55E`（底可用 green-tint `#F0FDF4`）
- mid(琥珀) `#b5842b`/`#9c6c00` → `#F59E0B`（底 amber-tint `#FFFBEB`）
- slow(红) `#b3261e` → `#EF4444`（底 error-tint `#FEF2F2`）

具体 tier 名以文件内实际为准（`.tier-1/.tier-2/.tier-3` 或 fast/medium/slow），保持 tier→语义映射一致即可。

- [ ] **Step 3: 构建验证**

Run: `cd /home/niuji/projects/model-bridge/web && npm run build`
Expected: 通过。

- [ ] **Step 4: 暂存待审**

```bash
cd /home/niuji/projects/model-bridge && git add web/src/views/Logs.vue
```

---

### Task 5: ApiKeys.vue（scoped CSS 机械映射）

**Files:**
- Modify: `web/src/views/ApiKeys.vue`（`<style scoped>` + `:deep`，~16 处）

**Interfaces:**
- Consumes: Task 1 基线。

- [ ] **Step 1: 按映射表替换全部 editorial 色值**

读 ApiKeys.vue `<style>`，按映射表替换。要点：
- `.table-container { background:#ffffff; border:1px solid #d9cfbf; border-radius:12px }` → bg `#FFFFFF`、border `#E2E8F0`、radius `14px`
- `.keys-table { --n-td-color:#ffffff; --n-th-color:#f4efe3 }` → `#FFFFFF` / `#F8FAFC`
- `:deep(.key-preview) { color:#74695a; background:#f4efe3; border:1px solid #d9cfbf }` → color `#475569`、bg `#F1F5F9`、border `#E2E8F0`
- `:deep(.copy-btn) { color:#1d7a4c }` → `#16A34A`
- `:deep(.time-cell)` `#74695a`→`#475569`；`:deep(.name-cell .key-name)` `#17140f`→`#0F172A`；`:deep(.rename-icon-btn)` `#74695a`→`#475569`
- `.create-modal { --n-title-text-color:#17140f }` → `#0F172A`
- empty 文字 `#74695a`/`#a89e8c`→`#475569`/`#94A3B8`

- [ ] **Step 2: 构建验证**

Run: `cd /home/niuji/projects/model-bridge/web && npm run build`
Expected: 通过。

- [ ] **Step 3: 暂存待审**

```bash
cd /home/niuji/projects/model-bridge && git add web/src/views/ApiKeys.vue
```

---

### Task 6: Help.vue（scoped CSS + 保留 brand SVG）

**Files:**
- Modify: `web/src/views/Help.vue`（`<style scoped>`，~33 处；**不改** brand SVG 的 `fill`）

**Interfaces:**
- Consumes: Task 1 基线。

- [ ] **Step 1: 按映射表替换 scoped editorial 色值**

读 Help.vue `<style>`，按映射表替换。要点：
- `.tool-card { background:#ffffff; border:1px solid #d9cfbf; border-radius:12px }` → bg `#FFFFFF`、border `#E2E8F0`、radius `14px`
- `.tool-card:hover { border-color:#c9c0b0; background:#f4efe3 }` → `#CBD5E1` / `#F1F5F9`
- `.card-name { color:#17140f }` → `#0F172A`
- page-title/subtitle：`#17140f`→`#0F172A`、`#a89e8c`→`#94A3B8`
- 代码块（若有 `background:#f4efe3`/`#f9f5ec`、`color:#17140f`、border `#d9cfbf`）→ `#F1F5F9`/`#0F172A`/`#E2E8F0`

- [ ] **Step 2: 确认 brand SVG fill 未被动**

Run: `cd /home/niuji/projects/model-bridge && grep -nE 'fill="#(D97757|2A84EE|000000)"' web/src/views/Help.vue`
Expected: 三处仍在（Claude `#D97757`、Copilot `#2A84EE`、Cursor `#000000`）。若缺，回退该处。

- [ ] **Step 3: 构建验证**

Run: `cd /home/niuji/projects/model-bridge/web && npm run build`
Expected: 通过。

- [ ] **Step 4: 暂存待审**

```bash
cd /home/niuji/projects/model-bridge && git add web/src/views/Help.vue
```

---

### Task 7: 集成验证（构建 + 嵌入编译 + 全服务视觉 + 回归）

**Files:**
- 无修改（验证 only）

**Interfaces:**
- Consumes: Task 1–6 全部产出。

- [ ] **Step 1: 前端构建最终确认**

Run: `cd /home/niuji/projects/model-bridge/web && npm run build`
Expected: 通过，`web/dist/` 产出。

- [ ] **Step 2: Rust 嵌入编译确认**

Run: `cd /home/niuji/projects/model-bridge && cargo build`
Expected: 编译通过（`include_dir!` 嵌入新 `web/dist` 成功；无 Rust 改动，应直接过）。

- [ ] **Step 3: 行为回归**

Run: `cd /home/niuji/projects/model-bridge && cargo test`
Expected: 全绿（`proxy.rs`/`crypto` 纯函数测试；本计划未动 Rust 逻辑）。

- [ ] **Step 4: 全服务视觉确认**

Run: `cd /home/niuji/projects/model-bridge && cargo run`（需 DB 与 `model-bridge.toml`；admin :10020）
打开 `http://127.0.0.1:10020/`，逐页核对 vs `design-mockups/ai-saas.html`：
- 侧栏白底 + 渐变品牌 + 蓝激活 + 绿状态点
- 仪表盘：4 统计卡(蓝/青/绿/琥珀) + 蓝青绿柱图 + 蓝→青→绿热力图 + 表格蓝调
- 供应商/日志/密钥/接入指南：白卡灰边、Inter 字、无暖色残留
- 接入指南三工具 SVG 品牌色保留

发现残留 editorial 暖色 → 回对应 task 修，再 `npm run build`。

- [ ] **Step 5: 提交（用户许可后）**

当前在 main；按 CLAUDE.md 先开分支再提交。等用户明确说「提交」后执行：

```bash
cd /home/niuji/projects/model-bridge
git checkout -b feat/ai-saas-theme
git add web/index.html web/src/App.vue web/src/views/*.vue
git commit -m "feat(web): reskin admin UI to AI SaaS light theme

Replace Editorial Ink + Fresh Data (cream/Fraunces/sage) with AI SaaS
light enterprise (slate canvas, blue→cyan→green gradient, Inter +
JetBrains Mono). Naive UI themeOverrides as main lever; ECharts palette
centralized via CHART const; scoped CSS remapped via editorial→AI SaaS
table. Provider brand SVG fills preserved.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

（mockup 与 spec 文件 `design-mockups/`、`docs/superpowers/` 是否纳入版本控制由用户定；默认不随此 commit。）
