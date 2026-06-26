# Theme Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the "Soft Laboratory" theme with "Editorial Ink + Fresh Data" across the Vue 3 admin UI — paper/ink editorial chrome, fresh-green data layer, Fraunces + IBM Plex Mono.

**Architecture:** A pure CSS/value migration. No layout, template logic, or dependency changes. The Naive UI `themeOverrides` object in `App.vue` is the single source of component-level theming; each view's scoped `<style>` block and the Dashboard ECharts option objects are recolored in place. Old hardcoded hex values and font names are swapped for the new palette (same hardcoded pattern the codebase already uses — no CSS-variable refactor, per YAGNI/surgical-edit).

**Tech Stack:** Vue 3, Naive UI 2, ECharts 5 (vue-echarts), Vite 6. No new deps.

**Verification approach:** This project has no test suite (per `CLAUDE.md`). Each task verifies by (a) grepping the edited file to confirm no old-palette values remain, (b) a final `npm run build` (Vite compile) + full grep gate + visual smoke via `npm run dev`.

**Stale-color gate pattern** (used in every task's grep step — call it `OLD`):
```
'#2d6a4f|#40916c|#1b4332|#95d5b2|#52b788|#52796f|#6b9080|#c4736e|#c9a84c|#e6b85c|#2d2d2a|#787870|#a0a098|#e0dcd5|#f0ece5|#f5f1eb|#f0ebe0|#faf8f4|#ebe5d9|#d5d0c8|#c5c0b8|#b08968|#5a7a68|#5a5a52|#b8a89a|#b8b3a8|Figtree|Newsreader|Source Code Pro'
```
Note: the Help brand-logo fills `#D97757` (Claude) and `#000000` (Cursor) are **intentional** and are NOT matched by this pattern — leave them.

**New palette reference** (the values substituted in below):
```
paper #faf7f0 · paper-2 #ffffff · desk #e7e3d7 · side #f4efe3
ink #17140f · dim #74695a · faint #a89e8c · rule #d9cfbf · rule-soft #ece6da
green #2ea86a · green-deep #1d7a4c · green-soft #bfe6cf
amber #e3a217 · amber-soft #f6e6b6 · amber-text #9c6c00
teal #159b96 · teal-soft #c2e8e6 · teal-text #0d6e6b
warn(ochre) #b5842b · bad(red) #b3261e
```

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `web/index.html` | Google Fonts link | Swap font families loaded |
| `web/src/App.vue` | Naive UI `themeOverrides` + global shell `<style>` | Rewrite both blocks |
| `web/src/views/Dashboard.vue` | Scoped styles + 2 ECharts option objects + hit-rate column | Recolor all; add stat-icon tint classes |
| `web/src/views/Providers.vue` | Scoped styles | Replace `<style>` block |
| `web/src/views/ApiKeys.vue` | Scoped styles | Replace `<style>` block |
| `web/src/views/Logs.vue` | Scoped styles (most semantic colors) | Replace `<style>` block |
| `web/src/views/Help.vue` | Scoped styles (keep brand-logo fills in template) | Replace `<style>` block only |

---

### Task 1: Fonts (`web/index.html`)

**Files:**
- Modify: `web/index.html` (the `<link href="...fonts.googleapis...">` line)

- [ ] **Step 1: Replace the Google Fonts link**

In `web/index.html`, replace the `<link href="https://fonts.googleapis.com/css2?...">` line with:

```html
    <link href="https://fonts.googleapis.com/css2?family=Fraunces:opsz,wght@9..144,400;9..144,500;9..144,600;9..144,700&family=IBM+Plex+Mono:ital,wght@0,400;0,500;0,600;1,400;1,500&display=swap" rel="stylesheet" />
```

This drops `Newsreader`, `Figtree`, and `Source Code Pro`; loads `Fraunces` (serif titles) and `IBM Plex Mono` (body/mono, with italic).

- [ ] **Step 2: Verify no old font names remain**

Run:
```bash
cd /home/niuji/projects/model-bridge/web && grep -nE "Newsreader|Figtree|Source Code Pro" index.html
```
Expected: no output.

- [ ] **Step 3: Commit**
```bash
cd /home/niuji/projects/model-bridge && git add web/index.html && git commit -m "feat(theme): load Fraunces + IBM Plex Mono fonts"
```

---

### Task 2: App.vue — themeOverrides + global shell styles

**Files:**
- Modify: `web/src/App.vue` (the `themeOverrides` object ~lines 98-152, and the `<style>` block ~lines 155-212)

- [ ] **Step 1: Replace the `themeOverrides` object**

In `web/src/App.vue`, replace the entire `const themeOverrides: GlobalThemeOverrides = { ... }` block with:

```ts
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
```

Key decisions: `primaryColor = ink` (black primary buttons, brand chrome). Switch-on, active menu, input focus ring, and success badges are overridden to **green** — green is the active/data accent, not the brand.

- [ ] **Step 2: Replace the global `<style>` block**

In `web/src/App.vue`, replace the entire `<style> ... </style>` block with:

```css
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
```

The `body::before` scanline texture is **removed** (it read as somber). Body background is now the desk color `#e7e3d7`; the app wrapper sits on `#faf7f0`. Status dot is green. Scrollbar hover is green-deep.

- [ ] **Step 3: Verify no stale values in App.vue**

Run:
```bash
cd /home/niuji/projects/model-bridge/web && grep -nE '#2d6a4f|#40916c|#1b4332|#95d5b2|#52b788|#52796f|#6b9080|#c4736e|#c9a84c|#e6b85c|#2d2d2a|#787870|#a0a098|#e0dcd5|#f0ece5|#f5f1eb|#f0ebe0|#faf8f4|#ebe5d9|#d5d0c8|#c5c0b8|Figtree|Newsreader|Source Code Pro' src/App.vue
```
Expected: no output.

- [ ] **Step 4: Commit**
```bash
cd /home/niuji/projects/model-bridge && git add web/src/App.vue && git commit -m "feat(theme): Editorial Ink themeOverrides + shell styles"
```

---

### Task 3: Dashboard.vue — styles, ECharts, stat-icon tints

**Files:**
- Modify: `web/src/views/Dashboard.vue` (template stat-icon divs ~lines 10/17/24/31, `hourlyOption` ~lines 82-90, `heatmapOption` ~lines 92-98, hit-rate column ~line 109, `<style>` ~lines 142-171)

- [ ] **Step 1: Add tint classes to the four stat-icon divs**

In `web/src/views/Dashboard.vue` template, the four `<div class="stat-icon">` elements (inside each `.stat-card`) become tinted. Change each opening tag:

- 总请求数 card: `<div class="stat-icon">` → `<div class="stat-icon ico-g">`
- 输入 Token card: `<div class="stat-icon">` → `<div class="stat-icon ico-a">`
- 输出 Token card: `<div class="stat-icon">` → `<div class="stat-icon ico-t">`
- 平均延迟 card: `<div class="stat-icon">` → `<div class="stat-icon ico-g">`

- [ ] **Step 2: Replace the two ECharts option objects**

Replace the `const hourlyOption = ref({ ... })` block with:

```ts
const hourlyOption = ref({
  backgroundColor: 'transparent',
  tooltip: { trigger: 'axis' as const, backgroundColor: '#ffffff', borderColor: '#d9cfbf', textStyle: { color: '#17140f', fontFamily: 'IBM Plex Mono, monospace', fontSize: 12 }, extraCssText: 'border-radius: 8px; box-shadow: 0 4px 16px rgba(23,20,15,0.06);' },
  grid: { left: 85, right: 24, bottom: 90, top: 30, borderColor: '#d9cfbf' },
  xAxis: { type: 'category' as const, data: [] as string[], axisLabel: { rotate: 45, fontSize: 10, fontFamily: 'IBM Plex Mono, monospace', color: '#74695a' }, axisLine: { lineStyle: { color: '#d9cfbf' } }, axisTick: { lineStyle: { color: '#d9cfbf' } } },
  yAxis: { type: 'value' as const, name: 'Token', nameLocation: 'middle' as const, nameGap: 40, nameTextStyle: { color: '#74695a', fontFamily: 'IBM Plex Mono, monospace', fontSize: 11 }, axisLabel: { fontFamily: 'IBM Plex Mono, monospace', fontSize: 10, color: '#74695a', formatter: (v: number) => formatNum(v) }, splitLine: { lineStyle: { color: '#ece6da', type: 'dashed' as const } } },
  dataZoom: [{ type: 'slider' as const, start: 0, end: 100, height: 20, bottom: 12, backgroundColor: '#f4efe3', borderColor: '#d9cfbf', borderRadius: 8, dataBackground: { lineStyle: { color: '#2ea86a', opacity: 0.15 }, areaStyle: { color: '#2ea86a', opacity: 0.04 } }, selectedDataBackground: { lineStyle: { color: '#2ea86a' }, areaStyle: { color: '#2ea86a', opacity: 0.08 } }, handleStyle: { color: '#2ea86a', borderRadius: 4 }, textStyle: { color: '#74695a', fontFamily: 'IBM Plex Mono, monospace', fontSize: 10 } }],
  series: [{ name: 'Token 用量', type: 'bar' as const, data: [] as number[], barWidth: '60%', itemStyle: { borderRadius: [4, 4, 0, 0], color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [{ offset: 0, color: '#2ea86a' }, { offset: 1, color: '#8dd8ae' }]) }, emphasis: { itemStyle: { color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [{ offset: 0, color: '#1d7a4c' }, { offset: 1, color: '#2ea86a' }]) } } }],
})
```

Replace the `const heatmapOption = ref({ ... })` block with:

```ts
const heatmapOption = ref({
  backgroundColor: 'transparent',
  tooltip: { formatter: (p: any) => `${p.value[0]}<br/>Token: <b>${formatNum(p.value[1] as number)}</b>`, backgroundColor: '#ffffff', borderColor: '#d9cfbf', textStyle: { color: '#17140f', fontFamily: 'IBM Plex Mono, monospace', fontSize: 12 }, extraCssText: 'border-radius: 8px; box-shadow: 0 4px 16px rgba(23,20,15,0.06);' },
  visualMap: { min: 0, max: 1, type: 'continuous' as const, orient: 'vertical' as const, right: 10, top: 'middle', itemWidth: 10, itemHeight: 90, inRange: { color: ['#ece6da', '#bfe6cf', '#5bbf8a', '#2ea86a', '#1d7a4c'] }, textStyle: { color: '#74695a', fontFamily: 'IBM Plex Mono, monospace', fontSize: 9 } },
  calendar: { range: ['2026-01-01', '2026-01-31'], cellSize: [13, 13], left: 36, right: 56, top: 24, bottom: 8, orient: 'horizontal', itemStyle: { borderWidth: 2, borderColor: '#fff', color: '#faf7f0' }, yearLabel: { show: false }, monthLabel: { nameMap: 'cn', color: '#74695a', fontFamily: 'IBM Plex Mono, monospace', fontSize: 11, margin: 8 }, dayLabel: { firstDay: 1, nameMap: 'cn', color: '#a89e8c', fontFamily: 'IBM Plex Mono, monospace', fontSize: 10 }, splitLine: { show: false } },
  series: [{ type: 'heatmap' as const, coordinateSystem: 'calendar', data: [] as any[] }],
})
```

- [ ] **Step 3: Recolor the hit-rate column render**

In the `modelColumns` array, replace the 命中率 column line with:

```ts
  { title: '命中率', key: 'cache_hit_rate', width: 90, align: 'right', render: (row) => h('span', { class: 'mono', style: { color: row.cache_hit_rate > 0 ? '#1d7a4c' : '#a89e8c', fontWeight: row.cache_hit_rate > 0 ? '600' : '400' } }, `${row.cache_hit_rate}%`) },
```

- [ ] **Step 4: Replace the `<style scoped>` block**

Replace the entire `<style scoped> ... </style>` block with:

```css
<style scoped>
.dashboard { display: flex; flex-direction: column; gap: 20px; }
.page-header { margin-bottom: 4px; }
.page-title { font-size: 28px; font-weight: 600; color: #17140f; margin: 0; letter-spacing: -0.02em; }
.page-subtitle { margin: 6px 0 0; color: #a89e8c; font-size: 13px; }

.stat-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 16px; }
.stat-card { background: #ffffff; border: 1px solid #d9cfbf; border-radius: 12px; padding: 22px; transition: box-shadow 0.2s, transform 0.2s; }
.stat-card:hover { box-shadow: 0 6px 22px rgba(23,20,15,0.06); transform: translateY(-1px); }
.stat-icon { width: 36px; height: 36px; border-radius: 9px; display: flex; align-items: center; justify-content: center; margin-bottom: 12px; }
.stat-icon svg { width: 20px; height: 20px; }
.stat-icon.ico-g { background: #bfe6cf; color: #1d7a4c; }
.stat-icon.ico-a { background: #f6e6b6; color: #9c6c00; }
.stat-icon.ico-t { background: #c2e8e6; color: #0d6e6b; }
.stat-label { font-size: 12px; font-weight: 500; color: #a89e8c; text-transform: uppercase; letter-spacing: 0.05em; margin-bottom: 6px; }
.stat-value { font-size: 28px; font-weight: 600; color: #17140f; line-height: 1; }
.stat-unit { font-size: 16px; color: #a89e8c; font-weight: 400; margin-left: 3px; }

.card { background: #ffffff; border: 1px solid #d9cfbf; border-radius: 12px; overflow: hidden; }
.card-header { display: flex; justify-content: space-between; align-items: center; padding: 18px 24px; border-bottom: 1px solid #ece6da; }
.chart-header-right { display: flex; align-items: center; gap: 12px; }
.card-title { margin: 0; font-family: 'Fraunces', 'Georgia', serif; font-size: 17px; font-weight: 600; color: #17140f; letter-spacing: -0.01em; }
.card-badge { font-size: 11px; color: #74695a; padding: 4px 10px; background: #f4efe3; border: 1px solid #d9cfbf; border-radius: 999px; }
.card-body { padding: 24px; }

.chart { height: 320px; }
.chart-bar { height: 380px; }
.chart-heatmap { height: 170px; }
.dashboard-table { --n-td-color: #ffffff; --n-th-color: #f4efe3; }
.model-id-cell { font-size: 12px; color: #74695a; }
.token-cell { color: #1d7a4c; font-weight: 500; }

@media (max-width: 900px) { .stat-grid { grid-template-columns: repeat(2, 1fr); } }
@media (max-width: 600px) { .stat-grid { grid-template-columns: 1fr; } .content { padding: 16px; } }
</style>
```

- [ ] **Step 5: Verify no stale values in Dashboard.vue**

Run:
```bash
cd /home/niuji/projects/model-bridge/web && grep -nE '#2d6a4f|#40916c|#1b4332|#95d5b2|#52b788|#52796f|#6b9080|#c4736e|#c9a84c|#e6b85c|#2d2d2a|#787870|#a0a098|#e0dcd5|#f0ece5|#f5f1eb|#f0ebe0|#faf8f4|#ebe5d9|#d5d0c8|#c5c0b8|Figtree|Newsreader|Source Code Pro' src/views/Dashboard.vue
```
Expected: no output.

- [ ] **Step 6: Commit**
```bash
cd /home/niuji/projects/model-bridge && git add web/src/views/Dashboard.vue && git commit -m "feat(theme): recolor Dashboard styles + ECharts + stat tints"
```

---

### Task 4: Providers.vue

**Files:**
- Modify: `web/src/views/Providers.vue` (`<style scoped>` ~lines 105-151)

- [ ] **Step 1: Replace the `<style scoped>` block**

Replace the entire `<style scoped> ... </style>` block with:

```css
<style scoped>
.page-header { margin-bottom: 24px; }
.page-title { font-size: 28px; font-weight: 600; color: #17140f; margin: 0; letter-spacing: -0.02em; }
.page-subtitle { margin: 6px 0 0; color: #a89e8c; font-size: 13px; }

.provider-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 16px; }
.provider-card { background: #ffffff; border: 1px solid #d9cfbf; border-radius: 12px; padding: 22px; cursor: pointer; transition: box-shadow 0.2s, border-color 0.2s, transform: 0.15s; }
.provider-card:hover { border-color: rgba(46, 168, 106, 0.3); box-shadow: 0 6px 22px rgba(23,20,15,0.06); transform: translateY(-1px); }
.provider-card.disabled { opacity: 0.55; }
.provider-card.disabled:hover { border-color: rgba(179, 38, 30, 0.25); box-shadow: 0 4px 24px rgba(179, 38, 30, 0.06); }
.card-top { display: flex; justify-content: space-between; align-items: center; margin-bottom: 14px; }
.card-name { display: flex; align-items: center; gap: 10px; }
.card-icon { width: 24px; height: 24px; object-fit: contain; flex-shrink: 0; }
.card-name-text { font-size: 17px; font-weight: 600; color: #17140f; letter-spacing: -0.01em; }
.card-channels { display: flex; flex-wrap: wrap; gap: 6px; margin-bottom: 16px; }
.channel-tag { display: flex; align-items: center; gap: 5px; font-size: 11px; padding: 4px 10px; background: #f4efe3; border: 1px solid #d9cfbf; border-radius: 8px; color: #74695a; }
.channel-tag.off { text-decoration: line-through; opacity: 0.5; }
.card-footer { display: flex; justify-content: space-between; align-items: center; }
.model-count { font-size: 12px; color: #a89e8c; }
.card-arrow { font-size: 14px; color: #c9c0b0; transition: color 0.2s, transform 0.2s; }
.provider-card:hover .card-arrow { color: #1d7a4c; transform: translateX(3px); }

.config-modal { --n-title-text-color: #17140f; }
.form-section { margin-top: 20px; }
.section-label { display: flex; align-items: center; gap: 8px; font-family: 'IBM Plex Mono', monospace; font-size: 12px; font-weight: 600; color: #a89e8c; text-transform: uppercase; letter-spacing: 0.06em; margin-bottom: 10px; }
.sync-btn { font-size: 11px; color: #1d7a4c; }
.sync-btn:focus:not(:focus-visible) { background-color: transparent; }
.channel-row { display: flex; align-items: center; gap: 8px; margin-bottom: 8px; }
.channel-type-label { font-size: 12px; width: 120px; flex-shrink: 0; color: #74695a; }
.channel-url-input { flex: 1; }
.model-list { max-height: 240px; overflow-y: auto; }
.model-row { display: flex; align-items: center; gap: 6px; margin-bottom: 6px; }
.model-id-input { width: 140px; flex-shrink: 0; }
.model-name-input { flex: 1; }
.remove-btn { opacity: 0.3; transition: opacity 0.15s; }
.remove-btn:hover { opacity: 1; }
.add-model-btn { width: 100%; font-size: 12px; color: #a89e8c; border-color: #d9cfbf; border-radius: 8px; margin-top: 4px; }

.sync-modal { --n-title-text-color: #17140f; }
.diff-section { margin-bottom: 16px; }
.diff-section:last-child { margin-bottom: 0; }
.diff-group-label { display: flex; align-items: center; gap: 6px; font-family: 'IBM Plex Mono', monospace; font-size: 12px; font-weight: 600; margin-bottom: 8px; }
.diff-group-label.added { color: #1d7a4c; }
.diff-group-label.removed { color: #b3261e; }
.diff-group-label.renamed { color: #b5842b; }
.diff-row { display: flex; align-items: center; gap: 8px; padding: 3px 0; font-size: 13px; }
</style>
```

Diff-label semantics: added=green, removed=red, renamed=ochre. Figtree → IBM Plex Mono on `.section-label` and `.diff-group-label`.

- [ ] **Step 2: Verify no stale values**

Run:
```bash
cd /home/niuji/projects/model-bridge/web && grep -nE '#2d6a4f|#40916c|#1b4332|#52796f|#6b9080|#c4736e|#2d2d2a|#787870|#a0a098|#e0dcd5|#f0ece5|#f0ebe0|#faf8f4|#d5d0c8|#c5c0b8|#b08968|Figtree|Newsreader|Source Code Pro' src/views/Providers.vue
```
Expected: no output.

- [ ] **Step 3: Commit**
```bash
cd /home/niuji/projects/model-bridge && git add web/src/views/Providers.vue && git commit -m "feat(theme): recolor Providers styles"
```

---

### Task 5: ApiKeys.vue

**Files:**
- Modify: `web/src/views/ApiKeys.vue` (`<style scoped>` ~lines 44-63)

- [ ] **Step 1: Replace the `<style scoped>` block**

Replace the entire `<style scoped> ... </style>` block with:

```css
<style scoped>
.page-header { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 24px; }
.page-title { font-size: 28px; font-weight: 600; color: #17140f; margin: 0; letter-spacing: -0.02em; }
.page-subtitle { margin: 6px 0 0; color: #a89e8c; font-size: 13px; }
.create-btn { font-weight: 600; }
.table-container { background: #ffffff; border: 1px solid #d9cfbf; border-radius: 12px; overflow: hidden; }
.keys-table { --n-td-color: #ffffff; --n-th-color: #f4efe3; }
.key-name { font-weight: 500; color: #17140f; }
.key-preview-cell { display: flex; align-items: center; gap: 8px; }
.key-preview { font-size: 12px; color: #74695a; background: #f4efe3; padding: 3px 10px; border: 1px solid #d9cfbf; border-radius: 8px; }
.copy-btn { opacity: 0.3; transition: opacity 0.15s; color: #1d7a4c; }
.copy-btn:hover { opacity: 1; }
.time-cell { font-size: 12px; color: #74695a; }
.delete-btn { opacity: 0.4; transition: opacity 0.15s; }
.delete-btn:hover { opacity: 1; }
.empty-state { display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 60px 20px; text-align: center; }
.empty-text { margin: 0; font-size: 16px; font-weight: 500; color: #74695a; }
.empty-hint { margin: 6px 0 0; font-size: 13px; color: #a89e8c; }
.create-modal { --n-title-text-color: #17140f; }
</style>
```

- [ ] **Step 2: Verify no stale values**

Run:
```bash
cd /home/niuji/projects/model-bridge/web && grep -nE '#2d6a4f|#2d2d2a|#787870|#a0a098|#e0dcd5|#faf8f4' src/views/ApiKeys.vue
```
Expected: no output.

- [ ] **Step 3: Commit**
```bash
cd /home/niuji/projects/model-bridge && git add web/src/views/ApiKeys.vue && git commit -m "feat(theme): recolor ApiKeys styles"
```

---

### Task 6: Logs.vue

**Files:**
- Modify: `web/src/views/Logs.vue` (`<style scoped>` ~lines 57-98)

- [ ] **Step 1: Replace the `<style scoped>` block**

Replace the entire `<style scoped> ... </style>` block with:

```css
<style scoped>
.page-header { display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 24px; }
.page-title { font-size: 28px; font-weight: 600; color: #17140f; margin: 0; letter-spacing: -0.02em; }
.page-subtitle { margin: 6px 0 0; color: #a89e8c; font-size: 13px; }
.record-count { font-size: 12px; color: #a89e8c; padding: 5px 12px; background: #ffffff; border: 1px solid #d9cfbf; border-radius: 999px; }

.table-container { background: #ffffff; border: 1px solid #d9cfbf; border-radius: 12px; overflow: hidden; }
.logs-table { --n-td-color: #ffffff; --n-th-color: #f4efe3; }

.time-cell { font-size: 12px; color: #74695a; }
.model-cell { font-size: 12px; color: #74695a; }
.provider-cell { font-size: 12px; color: #0d6e6b; }
.apikey-cell { font-size: 12px; color: #74695a; }
.apikey-deleted { font-size: 12px; color: #a89e8c; }
.token-info { font-size: 12px; display: inline-flex; flex-direction: column; align-items: flex-end; gap: 2px; }
.token-main { line-height: 1.4; }
.token-in { color: #9c6c00; }
.token-sep { color: #c9c0b0; }
.token-out { color: #0d6e6b; }
.token-cache { font-size: 11px; line-height: 1.3; white-space: nowrap; }
.cache-label { color: #a89e8c; margin-right: 5px; }
.cache-read { color: #0d6e6b; }
.cache-sep { color: #c9c0b0; margin: 0 5px; }
.cache-write { color: #b5842b; }
.latency-cell { font-size: 12px; color: #b5842b; }

.status-badge { display: inline-flex; align-items: center; gap: 5px; padding: 3px 10px; border-radius: 999px; }
.status-badge.success { background: rgba(46, 168, 106, 0.08); border: 1px solid rgba(46, 168, 106, 0.2); }
.status-badge.error { background: rgba(179, 38, 30, 0.08); border: 1px solid rgba(179, 38, 30, 0.2); }
.status-dot-sm { width: 5px; height: 5px; border-radius: 50%; }
.status-badge.success .status-dot-sm { background: #2ea86a; }
.status-badge.error .status-dot-sm { background: #b3261e; }
.status-text { font-size: 10px; font-weight: 600; letter-spacing: 0.05em; }
.status-badge.success .status-text { color: #1d7a4c; }
.status-badge.error .status-text { color: #b3261e; }
.error-cell { font-size: 12px; color: #b3261e; }

.pagination-bar { display: flex; justify-content: flex-end; margin-top: 16px; }
.empty-state { display: flex; flex-direction: column; align-items: center; justify-content: center; padding: 60px 20px; text-align: center; }
.empty-text { margin: 0; font-size: 16px; font-weight: 500; color: #74695a; }
.empty-hint { margin: 6px 0 0; font-size: 13px; color: #a89e8c; }
</style>
```

Semantic mapping here: token-in=amber `#9c6c00`, token-out=teal `#0d6e6b`, cache-read=teal, cache-write=ochre, latency=ochre, success=green, error=red.

- [ ] **Step 2: Verify no stale values**

Run:
```bash
cd /home/niuji/projects/model-bridge/web && grep -nE '#2d6a4f|#40916c|#52796f|#6b9080|#c4736e|#c9a84c|#2d2d2a|#787870|#a0a098|#e0dcd5|#f0ece5|#faf8f4|#d5d0c8|#b8a89a|#b8b3a8|#5a5a52' src/views/Logs.vue
```
Expected: no output.

- [ ] **Step 3: Commit**
```bash
cd /home/niuji/projects/model-bridge && git add web/src/views/Logs.vue && git commit -m "feat(theme): recolor Logs styles + semantic status"
```

---

### Task 7: Help.vue

**Files:**
- Modify: `web/src/views/Help.vue` (`<style scoped>` ~lines 138-178). **Do not touch the `<template>`** — the brand-logo SVG fills `#D97757` (Claude) and `#000000` (Cursor) are official brand icons and stay.

- [ ] **Step 1: Replace the `<style scoped>` block**

Replace the entire `<style scoped> ... </style>` block with:

```css
<style scoped>
.page-header { margin-bottom: 24px; }
.page-title { font-size: 28px; font-weight: 600; color: #17140f; margin: 0; letter-spacing: -0.02em; }
.page-subtitle { margin: 6px 0 0; color: #a89e8c; font-size: 13px; }

.tool-cards { display: flex; gap: 12px; }
.tool-card { display: flex; align-items: center; gap: 12px; padding: 14px 18px; background: #ffffff; border: 1px solid #d9cfbf; border-radius: 12px; cursor: pointer; transition: border-color 0.15s, background 0.15s, transform 0.1s; min-width: 200px; }
.tool-card:hover { border-color: #c9c0b0; background: #f4efe3; }
.tool-card:active { transform: translateY(1px); }
.tool-card .card-icon { display: flex; align-items: center; }
.card-meta { display: flex; flex-direction: column; gap: 2px; }
.card-name { font-size: 14px; font-weight: 600; color: #17140f; transition: color 0.15s; }
.tool-card:hover .card-name { color: #1d7a4c; }
.card-tag { font-size: 11px; color: #a89e8c; }

.detail-card { position: relative; background: #ffffff; border: 1px solid #d9cfbf; border-radius: 14px; padding: 20px 24px 24px; width: min(680px, 92vw); max-height: 85vh; overflow-y: auto; }
.detail-title { display: flex; align-items: baseline; gap: 12px; margin-bottom: 16px; padding: 0 36px 14px 0; border-bottom: 1px solid #d9cfbf; }
.detail-name { font-size: 17px; font-weight: 600; color: #17140f; }
.detail-tag { font-size: 12px; color: #74695a; }

.close-btn { position: absolute; top: 14px; right: 14px; color: #a89e8c; opacity: 0.6; transition: opacity 0.15s, color 0.15s; }
.close-btn:hover { opacity: 1; color: #17140f; }

.tool-section { padding-top: 4px; }
.tool-desc { color: #74695a; font-size: 14px; line-height: 1.7; margin: 0 0 16px; }

.code-block { position: relative; background: #f4efe3; border: 1px solid #d9cfbf; border-radius: 12px; margin: 10px 0 14px; }
.code-block pre { margin: 0; padding: 16px 56px 16px 16px; overflow-x: auto; font-size: 13px; line-height: 1.6; color: #17140f; white-space: pre; }
.code-block .copy-btn { position: absolute; top: 8px; right: 8px; color: #1d7a4c; opacity: 0.45; transition: opacity 0.15s; font-size: 12px; }
.code-block .copy-btn:hover { opacity: 1; }

.chip { font-size: 12px; color: #1d7a4c; background: #f4efe3; padding: 1px 7px; border: 1px solid #d9cfbf; border-radius: 6px; }

.notes { color: #74695a; font-size: 13px; line-height: 1.85; padding-left: 18px; margin: 12px 0 0; }
.notes li { margin: 3px 0; }
.notes li::marker { color: #a89e8c; }

.steps { color: #17140f; font-size: 14px; line-height: 1.9; padding-left: 20px; margin: 8px 0 0; }
.steps li { margin: 8px 0; }
.steps li::marker { color: #1d7a4c; font-weight: 600; }
</style>
```

- [ ] **Step 2: Confirm brand-logo fills are untouched**

Run:
```bash
cd /home/niuji/projects/model-bridge/web && grep -nE '#D97757|#000000' src/views/Help.vue
```
Expected: two matches (the Claude and Cursor SVG `fill="..."` attributes in `<template>`).

- [ ] **Step 3: Verify no stale theme values in the style block**

Run:
```bash
cd /home/niuji/projects/model-bridge/web && grep -nE '#2d6a4f|#2d2d2a|#787870|#a0a098|#e0dcd5|#f0ebe0|#faf8f4|#c5c0b8|#5a7a68' src/views/Help.vue
```
Expected: no output.

- [ ] **Step 4: Commit**
```bash
cd /home/niuji/projects/model-bridge && git add web/src/views/Help.vue && git commit -m "feat(theme): recolor Help styles (keep brand-logo fills)"
```

---

### Task 8: Build, full grep gate, visual smoke

**Files:** none (verification only)

- [ ] **Step 1: Production build compiles**

Run:
```bash
cd /home/niuji/projects/model-bridge/web && npm run build
```
Expected: `vite build` completes with no errors; `dist/` written. (If it fails, fix the syntax error in the flagged file before proceeding.)

- [ ] **Step 2: Full stale-color gate across the frontend**

Run:
```bash
cd /home/niuji/projects/model-bridge/web && grep -rnE '#2d6a4f|#40916c|#1b4332|#95d5b2|#52b788|#52796f|#6b9080|#c4736e|#c9a84c|#e6b85c|#2d2d2a|#787870|#a0a098|#e0dcd5|#f0ece5|#f5f1eb|#f0ebe0|#faf8f4|#ebe5d9|#d5d0c8|#c5c0b8|#b08968|#5a7a68|#5a5a52|#b8a89a|#b8b3a8|Figtree|Newsreader|Source Code Pro' src
```
Expected: no output. (The Help brand fills `#D97757`/`#000000` are not matched by this pattern; confirm they remain with `grep -nE '#D97757|#000000' src/views/Help.vue` → two matches.)

- [ ] **Step 3: Visual smoke test**

Run:
```bash
cd /home/niuji/projects/model-bridge/web && npm run dev
```
Open the printed Vite URL (default http://localhost:5173). Walk all five routes — `/` (Dashboard), `/providers`, `/api-keys`, `/logs`, `/help` — and open at least one modal on each of Providers (provider config), ApiKeys (生成密钥), and Help (click a tool card).

Expected: paper background, ink text, Fraunces headings, IBM Plex Mono body/numbers, green active nav + online dot + charts, soft black primary buttons, green switch. No forest-green `#2d6a4f`, no cream `#f5f1eb`, no scanline texture. Help brand logos render in their original orange/black.

(For live data, optionally run the Rust server in another shell — `cargo run` — so the dev proxy has a backend; empty states are fine for a visual check.)

- [ ] **Step 4: Rebuild Rust binary to embed the new frontend (if shipping)**

Per `CLAUDE.md`, the admin server embeds `web/dist/` at compile time. After the build in Step 1:
```bash
cd /home/niuji/projects/model-bridge && cargo build
```
Expected: compiles; the admin server at `:10020` now serves the new theme.

- [ ] **Step 5: Commit the build** (only if `web/dist/` is tracked — it is embedded via `include_dir!`, check first)
```bash
cd /home/niuji/projects/model-bridge && git status --short web/dist
```
If `web/dist/` is git-tracked, `git add web/dist && git commit -m "build: rebuild embedded frontend with new theme"`; otherwise skip — nothing to commit.

---

## Self-Review (completed)

**1. Spec coverage:** §1 goal ✓ (all tasks recolor). §3 tokens ✓ (App themeOverrides + per-file hex). §3.2 fonts ✓ (Task 1). §3.3 radii/shadow ✓ (themeOverrides + card 12px in views). §4 semantic mapping ✓ (Logs status, Providers diff labels, Dashboard hit-rate). §5 themeOverrides ✓ (Task 2). §6 global styles ✓ (Task 2 — scanline removed, desk/paper/sidebar, status dot green). §7 data layer ✓ (Task 3 — bar/heatmap/dataZoom/tooltip + stat tints). §8 per-page ✓ (Tasks 3-7; Help brand fills preserved, Task 7 Step 2). §9 non-goals respected (no layout/deps/backend changes). §10 verification ✓ (Task 8 — build, full grep, visual).

**2. Placeholder scan:** No TBD/TODO/"add error handling"/"similar to Task N". Every code step shows complete replacement blocks. ✓

**3. Type consistency:** `themeOverrides` keys match Naive UI's `GlobalThemeOverrides` shape used in the existing file. ECharts option objects keep the same structure (only color/font values change), so `loadData`'s spread-merge still type-checks. Hit-rate render keeps `(row)` signature and `h('span', {...})` shape. Stat-icon classes `ico-g/ico-a/ico-t` defined in Task 3 Step 4 CSS match the classes added in Step 1. ✓

No issues found — plan is complete.
