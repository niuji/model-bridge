# AI SaaS 主题重写 spec

日期 2026-07-23 · 目标：把 `web/src/` 现有「Editorial Ink + Fresh Data」主题（暖奶油 + Fraunces 衬线 + 鼠尾草绿）整体替换为已选定的 **AI SaaS 浅底克制企业风**（白/浅灰底 + 蓝→青→绿克制渐变 + Inter + JetBrains Mono + 卡片化大留白）。

参考真值：`design-mockups/ai-saas.html`（已与用户确认的浅底克制版）。

## 1. 目标与非目标

- **目标**：全站 5 个 view + App.vue shell 视觉一致地切到 AI SaaS 调色板/字体/圆角；Naive UI 组件（card/table/button/input/menu/modal…）经 `themeOverrides` 一次性换肤；ECharts 图表换蓝→青→绿 ramp。
- **非目标**：不改信息架构、不改交互逻辑、不动数据流、不引入主题切换器、不做深色模式。纯视觉 reskin。
- **不动**：供应商品牌身份——Help.vue 里 Claude/Cursor/Copilot 的 SVG `fill`（`#D97757`/`#000000`/`#2A84EE`）原样保留；Providers 的 `<img src="/icons/...">` 图标不涉及 CSS 色。

## 2. Token 集合

| 角色 | 值 |
|---|---|
| 画布 canvas | `#F8FAFC` |
| 卡片/表层 card | `#FFFFFF` |
| 侧栏 sidebar | `#FFFFFF` |
| 正文 ink | `#0F172A` |
| 次文字 text2 | `#475569` |
| 弱文字 text3 | `#94A3B8` |
| 边框 border | `#E2E8F0` |
| 强边框 border2 | `#CBD5E1` |
| hover 底 | `#F1F5F9` |
| 蓝 primary | `#3B82F6`（主操作/激活态） |
| 蓝悬停 | `#2563EB` |
| 青 cyan | `#06B6D4` |
| 绿 success/online | `#22C55E` |
| 深绿（命中率等） | `#16A34A` |
| 琥珀 warning/drift/延迟 | `#F59E0B` |
| 错误 error | `#EF4444` |
| 渐变（品牌/标） | `linear-gradient(135deg,#3B82F6,#06B6D4 50%,#22C55E)` |
| 渐变竖（柱图） | `linear-gradient(180deg,#3B82F6,#06B6D4 52%,#22C55E)` |
| 浅 tint | blue `#EFF6FF` / cyan `#ECFEFF` / green `#F0FDF4` / amber `#FFFBEB` / error `#FEF2F2` |
| 字体 UI/正文 | Inter (400/500/600/700) |
| 字体 数据 | JetBrains Mono (400/500) |
| 圆角 | 卡片/输入 14px、按钮 8px、徽章 999px |
| 阴影 | `0 1px 2px rgba(15,23,42,.04)` |

## 3. Editorial → AI SaaS 映射表（~371 处色值的机械替换依据）

| Editorial | 角色 | → AI SaaS |
|---|---|---|
| `#17140f` | 正文 ink | `#0F172A` |
| `#2a2620` | ink hover | `#1E293B` |
| `#000000` | pressed | `#0F172A` |
| `#74695a` | text2 | `#475569` |
| `#a89e8c` `#8c7f6c` | text3 | `#94A3B8` |
| `#5d564b` | text 变体 | `#475569` |
| `#4b443a` | text 深 | `#334155` |
| `#d9cfbf` | border | `#E2E8F0` |
| `#c9c0b0` `#bfb29a` | border 强/滚动条 | `#CBD5E1` |
| `#ece6da` | divider | `#F1F5F9` |
| `#faf7f0` | body bg | `#F8FAFC` |
| `#f4efe3` | 暖侧栏/面 | sidebar→`#FFFFFF` / panel→`#F1F5F9` |
| `#f9f5ec` `#e5dccd` `#e7e3d7` `#ddd6c8` `#d3ccc0` | 暖面 | `#F1F5F9`/`#E2E8F0` |
| `#fff7f5` `#fcfaf6` `#f7f2e8` `#f1ece0` `#eef7f0` `#f4fbf6` | 近白暖 | `#FFFFFF`/`#F8FAFC` |
| `#1d7a4c` | 绿（命中率/复制/激活） | `#16A34A` |
| `#2ea86a` | 绿 | `#22C55E` |
| `#bfe6cf` `#cbe6d3` `#dcefe1` | 绿 tint 背景（图标芯片） | green-tint `#F0FDF4` |
| `#8dd8ae` `#5bbf8a` | 图表渐变浅绿端 | `#86EFAC` |
| `#0d6e6b` `#159b96` `#1aab9f` | teal/info | `#06B6D4` |
| `#c2e8e6` | teal tint | cyan-tint `#ECFEFF` |
| `#9c6c00` `#b5842b` | 琥珀 | `#F59E0B` |
| `#f6e6b6` | 琥珀 tint | amber-tint `#FFFBEB` |
| `#b3261e` `#9f2019` `#851a15` | error | `#EF4444` |
| `#D97757` `#2A84EE` `#000000`(brand fill) | 供应商品牌 | **保留原值** |

## 4. 策略（最小且自洽）

1. **主杠杆 = App.vue `themeOverrides`**：Naive UI 全部组件经这一处换肤，覆盖 card/table/button/input/menu/modal/tag/pagination 等，多数 view 无需动 Naive 组件本身。
2. **字体集中迁移**：`index.html` 换 Google Fonts（Fraunces+IBM Plex Mono → Inter+JetBrains Mono）；App.vue 全局 `<style>` 把 `.serif` 重定义为 Inter、`.mono` 重定义为 JetBrains Mono——全站 `class="serif"`/`"mono"` 即刻生效，无需逐 view 改 class。`themeOverrides.common.fontFamily`/`fontFamilyMono` 同步。
3. **scoped CSS 直替换**：各 view 的 scoped 样式按映射表直接换 hex（不引入逐处 `var()`，保持最小 diff、最低风险）。
4. **ECharts 集中调色**：Dashboard.vue 顶部新增 `const CHART = { ink, text2, text3, border, canvas, blue, cyan, green, greenD, ramp:[...] }`，`hourlyOption`/`heatmapOption` 的 ~20 处内联色值改为引用 `CHART.*`，柱图渐变改 `CHART` 蓝青绿、热力图 `visualMap.inRange` 用 `CHART.ramp`。一次定义、集中可调。
5. **`.serif` 残留**：card-title 等 `font-family:'Fraunces'` 内联值改 Inter 600（随 themeOverrides Card.titleFontFamily 一并处理，或在 scoped 里改）。

## 5. 文件清单与改动

| 文件 | 色值数 | 改动 |
|---|---|---|
| `web/index.html` | — | 换字体 link：Inter + JetBrains Mono |
| `src/App.vue` | 85 | 重写 `themeOverrides`（common+Layout/Menu/Card/Button/Input/Switch/Table/DataTable/Modal/Tag/Pagination/Slider/Spin）；重写全局 `<style>`（body `.app-wrapper` `.sidebar*` `.brand*` `.sidebar-menu/footer` `.status-dot` 滚动条 `.content` 过渡 `.mono/.serif`）；`fontFamily` 全换 |
| `src/views/Dashboard.vue` | 63 | 新增 `CHART` 常量；`hourlyOption`/`heatmapOption` 全色值引用之（tooltip/grid/axis/dataZoom/series 渐变/visualMap.inRange/calendar）；`modelColumns` render 内联色（命中率绿）；scoped CSS（page-title/stat-icon ico-g/a/t/stat-*/card-*/dashboard-table/--n-*/`:deep`） |
| `src/views/Providers.vue` | 95 | scoped CSS 全色值映射（page-header/card-hero/card-icon-well/fallback/card-name/card-meta/status-dot/drift-badge/card-channels/modal…）；品牌字母 fallback `#74695a`→`#475569`；图标 `<img>` 不动 |
| `src/views/Logs.vue` | 79 | scoped + `:deep` 色值映射（page-header/count/logs-table `--n-*`/empty/time/caller）；`latency-fill tier-*` 语义色→新绿/琥珀/红 |
| `src/views/Help.vue` | 33 | scoped 色值映射（tool-card/card-name/page-header/code 块）；**保留** Claude/Cursor/Copilot SVG fill |
| `src/views/ApiKeys.vue` | 16 | scoped + `:deep`（table-container/keys-table `--n-*`/key-preview/copy-btn 绿/time-cell/create-modal） |

## 6. 验证

1. `cd web && npm run build` 通过（TypeScript + Vite）。
2. `cargo build` 通过（`include_dir!` 嵌入新 `web/dist` 编译成功）。
3. 视觉确认：`cargo run` 起全服务（admin :10020 + proxy :10010），浏览器看真实数据下的 5 个页面是否一致；与 `design-mockups/ai-saas.html` 对照。
4. 不改任何业务逻辑/接口，`cargo test` 应仍全绿（无逻辑变更）。

## 7. 风险与回滚

- 风险：371 处色值散落，逐文件替换易漏/错配。缓解：按映射表机械替换 + App.vue 主杠杆先改、各 view 后改、每改一个 `npm run build` 验证。
- 回滚：纯样式改动，`git checkout -- web/src web/index.html` 即回退；mockup 与 spec 留作对照。
