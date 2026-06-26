# Theme Redesign — Design Spec

**Date:** 2026-06-26
**Scope:** Visual theme overhaul of the Model Bridge admin UI (Vue 3 + Naive UI SPA). No structural/layout changes, no new dependencies, no backend.

## 1. Goal

Replace the current "Soft Laboratory" theme (warm cream `#f5f1eb` + forest green `#2d6a4f` + Figtree/Newsreader/Source Code Pro) with a new direction validated through iterative browser mockups:

> **"Editorial Ink + Fresh Data"** — a paper-white / ink-black editorial chrome (serif titles, monospace everything else, hairline rules, soft buttons) with a fresh, brighter-green data layer. Brand has no hue (ink); color is reserved for data and semantic states.

## 2. Aesthetic Direction

- **Tone:** refined editorial / technical journal. Paper + ink, generous whitespace, strong typographic hierarchy, hairline rules, disciplined color.
- **Differentiator:** Fraunces serif headlines paired with IBM Plex Mono body — a "typewritten journal" feel — on warm paper, with a single fresh green carrying all data and "active" meaning.
- **Color discipline (decided with the user):**
  - Red is an alert color, never a highlight — so the primary stat is emphasized by weight, not red.
  - Brand/chrome has no hue (ink) — "简洁明快" via restraint, not austerity.
  - The data layer carries color (green primary + amber/teal accents) so dashboards don't read as all-black.
  - `墨黑品牌` (ink brand) was chosen over ultramarine/teal-as-brand; green is the data/active accent, not the brand.

## 3. Design Tokens

### 3.1 Palette

| Token | Value | Role |
|---|---|---|
| `--paper` | `#faf7f0` | app/content background, Naive `bodyColor`/`baseColor` |
| `--paper-2` | `#ffffff` | cards, table cells, inputs |
| `--desk` | `#e7e3d7` | page background outside the app wrapper (body) |
| `--side` | `#f4efe3` | sidebar background |
| `--ink` | `#17140f` | primary text, brand chrome, `primaryColor` |
| `--dim` | `#74695a` | secondary text |
| `--faint` | `#a89e8c` | tertiary text, placeholders |
| `--rule` | `#d9cfbf` | borders, dividers |
| `--rule-soft` | `#ece6da` | table row borders, light dividers |
| `--green` | `#2ea86a` | primary data color, success/active/online/positive |
| `--green-deep` | `#1d7a4c` | hover/pressed, active text, deep accent |
| `--green-soft` | `#bfe6cf` | green tint background |
| `--amber` | `#e3a217` | data accent (input-token stat) |
| `--amber-soft` | `#f6e6b6` | amber tint background |
| `--teal` | `#159b96` | data accent (output-token stat), `infoColor` |
| `--teal-soft` | `#c2e8e6` | teal tint background |
| `--warn` | `#b5842b` | paused/warning (ochre) |
| `--bad` | `#b3261e` | error/danger |

The fresh green `#2ea86a` is deliberately brighter than the old forest `#2d6a4f` so the theme does not read as a revert.

### 3.2 Typography

| Use | Family | Notes |
|---|---|---|
| Display / page titles / card titles / brand | `'Fraunces', serif` | variable opsz 9–144, weights 400–700; page title ~34px, card title ~16px, brand ~17px |
| Body / UI / numbers / labels / menu / charts | `'IBM Plex Mono', monospace` | weights 400–600; base 14px; italic for decks/subtitles |

IBM Plex Mono replaces **both** Figtree (body) and Source Code Pro (mono). Fraunces replaces Newsreader. Loaded via Google Fonts in `index.html`:
```
family=Fraunces:opsz,wght@9..144,400;9..144,500;9..144,600;9..144,700
family=IBM+Plex+Mono:ital,wght@0,400;0,500;0,600;1,400;1,500
```

### 3.3 Shape, shadow, motion

- **Radii:** common `10px`; Card `12px`; Button `8px` (small/medium), `10px` (large); Input `8px`; Modal `14px`; Tag/badge pill `999px`.
- **Shadows (ink-tinted, soft):** `boxShadow1 0 1px 2px rgba(23,20,15,.05)`; `boxShadow2 0 4px 16px rgba(23,20,15,.06)`; `boxShadow3 0 12px 40px rgba(23,20,15,.08)`; card hover `0 6px 22px rgba(23,20,15,.06)` + 1px lift.
- **Atmosphere:** clean — drop the old scanline texture on `body::before` (it read as somber). Optional: a very faint paper-grain noise on the app background; default off.
- **Motion (kept subtle):** page transition (opacity + translateY), green status-dot pulse, card/button hover lift. No new motion deps.

## 4. Semantic Color Mapping

| Meaning | Color | Where |
|---|---|---|
| Active / online / success / positive delta | green `#2ea86a` / deep `#1d7a4c` | status dots, "在线", active menu, switch-on, hit-rate, positive deltas |
| Paused / warning | ochre `#b5842b` | paused status, warning badges |
| Error / danger | red `#b3261e` | error badges, error text, danger actions |
| Brand / primary action / chrome | ink `#17140f` | primary buttons, focus default, titles, brand mark |
| Data magnitude (charts) | green scale + amber/teal accents | bars, heatmap, stat icons |

## 5. Naive UI `themeOverrides` (reference values)

Rewrite the `themeOverrides` object in `App.vue`. Key fields:

```ts
common: {
  fontFamily: "'IBM Plex Mono', monospace",
  fontFamilyMono: "'IBM Plex Mono', monospace",
  fontSize: '14px',
  primaryColor: '#17140f', primaryColorHover: '#2a2620',
  primaryColorPressed: '#000000', primaryColorSuppl: '#17140f',
  infoColor: '#159b96', successColor: '#2ea86a',
  warningColor: '#b5842b', errorColor: '#b3261e',
  bodyColor: '#faf7f0', baseColor: '#faf7f0',
  cardColor: '#ffffff', modalColor: '#ffffff', popoverColor: '#ffffff', tableColor: '#ffffff',
  borderColor: '#d9cfbf', dividerColor: '#ece6da',
  textColorBase: '#17140f', textColor1: '#17140f', textColor2: '#74695a', textColor3: '#a89e8c',
  inputColor: '#ffffff', hoverColor: 'rgba(23,20,15,0.04)',
  boxShadow1: '0 1px 2px rgba(23,20,15,0.05)',
  boxShadow2: '0 4px 16px rgba(23,20,15,0.06)',
  boxShadow3: '0 12px 40px rgba(23,20,15,0.08)',
  borderRadius: '10px', heightSmall: '30px', heightMedium: '34px', heightLarge: '40px',
},
Layout: { siderColor: '#f4efe3', headerColor: '#faf7f0', footerColor: '#f4efe3' },
Menu: {
  itemTextColor: '#74695a', itemTextColorHover: '#17140f', itemTextColorActive: '#1d7a4c',
  itemIconColor: '#a89e8c', itemIconColorHover: '#17140f', itemIconColorActive: '#1d7a4c',
  itemColorHover: 'rgba(23,20,15,0.04)',
  itemColorActive: 'rgba(46,168,106,0.10)', itemColorActiveHover: 'rgba(46,168,106,0.14)',
  borderRadius: '8px',
},
Card: { borderRadius: '12px', borderColor: '#d9cfbf', paddingMedium: '22px',
  titleTextColor: '#17140f', titleFontSize: '16px', titleFontWeight: '600',
  titleFontFamily: "'Fraunces', serif" },
Button: { borderRadiusSmall: '8px', borderRadiusMedium: '8px', borderRadiusLarge: '10px' },
Input: { borderRadius: '8px', border: '1px solid #d9cfbf',
  borderFocus: '1px solid #2ea86a', borderHover: '1px solid #bfb29a',
  boxShadowFocus: '0 0 0 3px rgba(46,168,106,0.12)' },
Switch: { railColorActive: '#2ea86a' },
Table: { tdColor: '#ffffff', thColor: '#f4efe3', thTextColor: '#74695a',
  tdTextColor: '#17140f', borderColor: '#ece6da', thFontWeight: '600', thFontSize: '11px' },
DataTable: { tdColor: '#ffffff', thColor: '#f4efe3', thTextColor: '#74695a',
  tdTextColor: '#17140f', borderColor: '#ece6da' },
Modal: { borderRadius: '14px', textColor: '#17140f' },
Tag: { borderRadius: '999px' },
Pagination: { itemColor: '#ffffff', itemColorActive: 'rgba(23,20,15,0.06)',
  itemTextColorActive: '#17140f', itemBorder: '1px solid #d9cfbf',
  itemBorderActive: '1px solid #17140f', itemBorderRadius: '8px' },
Slider: { fillColor: '#17140f', handleColor: '#17140f' },
Spin: { color: '#1d7a4c' },
```

Notes: `primaryColor = ink` (black primary buttons, brand chrome). Switch-on, active menu, focus ring, and success badges are overridden to **green** — green is the active/data accent, not the brand. Input focus ring is green (friendly, 明快).

## 6. Global Styles (`App.vue` `<style>`)

- `body` background `--desk #e7e3d7`; text `--ink`; font IBM Plex Mono. Remove the `body::before` scanline.
- `.app-wrapper` background `--paper #faf7f0`; keep max-width 1440 centered + box-shadow.
- Sidebar `--side #f4efe3` + right hairline `--rule`; brand icon ink; brand text Fraunces; menu items IBM Plex Mono; active item green left-border + green-tint bg (via Menu overrides + a sidebar style for the left border accent); footer version (mono, faint) + green pulsing online dot.
- Scrollbar thumb `#c9c0b0`, hover `#1d7a4c`.
- `.serif` utility → Fraunces; `.mono` utility → IBM Plex Mono.
- Keep page transition; keep status-dot pulse (recolored green).

## 7. Data Layer (charts & stat accents)

- **Stat-card icons:** tinted soft squares — requests `green-soft`/`green-deep`, input `amber-soft`/`#9c6c00`, output `teal-soft`/`#0d6e6b`, latency `green-soft`/`green-deep`. Values stay ink.
- **Bar chart (ECharts):** gradient `#2ea86a → #8dd8ae`, peak/emphasis `#1d7a4c → #2ea86a`, top radius 4. Axis labels IBM Plex Mono `#74695a`; axis/tick `#d9cfbf`; splitLine `#ece6da` dashed. Tooltip white, border `#d9cfbf`, text `#17140f`, mono.
- **Heatmap (ECharts):** `visualMap.inRange` green scale `['#ece6da','#bfe6cf','#5bbf8a','#2ea86a','#1d7a4c']`; empty cells `#faf7f0`; cell border `#ffffff`.
- **DataZoom:** bg `#f4efe3`, border `#d9cfbf`, handle/data `#2ea86a`, text `#74695a` mono.
- **Hit-rate cell:** `#1d7a4c` when >0 else `#a89e8c`.

## 8. Per-Page Application

All scoped `<style>` blocks replace the old palette (`#2d6a4f` family, `#2d2d2a/#787870/#a0a098`, `#e0dcd5/#f0ece5`, `#c4736e`, `#c9a84c`) and old fonts (Figtree/Newsreader/Source Code Pro) with the new tokens. Approx. ref counts: App 55, Logs 27, Dashboard 25, Help 24, Providers 19, ApiKeys 11.

- **Dashboard.vue** — scoped styles + the two ECharts option objects (`hourlyOption`, `heatmapOption`) + the `modelColumns` hit-rate render. See §7. Card titles → Fraunces; stat icons tinted; hairline rules.
- **Providers.vue** — provider cards (border `--rule`, hover border green-tint + soft shadow), channel tags, config/sync modals, diff labels: `added=green`, `removed=red`, `renamed=ochre` (`#b5842b`) or neutral. Replace `.section-label`/`.diff-group-label` Figtree → IBM Plex Mono.
- **ApiKeys.vue** — table container, key-preview chip, copy button green, create modal. Replace greens.
- **Logs.vue** — token-in = amber, token-out = teal (matching the Dashboard input/output stat accents); cache read = teal, cache write = ochre; `status-badge.success` green / `.error` red; latency ochre. This file has the most semantic colors — map each to the §4 discipline.
- **Help.vue** — tool cards, detail modal, code blocks, chips, steps. **Keep the brand-logo SVG fills `#D97757` (Claude) and `#000000` (Cursor)** — these are official brand icons (commit `4db0750`), not theme colors. Replace only theme greens/fonts.
- **App.vue** — §5 + §6.

## 9. Non-Goals

- No layout/structure changes (page composition, sidebar width, routes unchanged).
- No new JS/TS dependencies (Naive UI, ECharts, Vue Router stay).
- No backend / API changes.
- No rebranding of provider logos in Help.
- Does not touch the proxy/admin server or Rust code.

## 10. Verification

1. `cd web && npm run build` succeeds (then rebuild Rust binary to embed, per CLAUDE.md).
2. `npm run dev` — visually check all 5 pages (Dashboard, Providers, ApiKeys, Logs, Help) + at least one modal (provider config, key create, help detail).
3. ECharts render with the new green palette and IBM Plex Mono axis fonts.
4. Grep confirms no stale theme values remain:
   ```
   grep -rnE "#2d6a4f|#40916c|#1b4332|#c4736e|#2d2d2a|#787870|#a0a098|#e0dcd5|#f0ece5|#f5f1eb|#f0ebe0|Figtree|Newsreader|Source Code Pro" web/src
   ```
   Expected: empty (the Help brand-logo fills `#D97757`/`#000000` are intentional and not matched by this pattern).
5. Semantic spot-checks: active/online = green; a paused/disabled provider = ochre; an error log row = red; primary stat emphasized by weight, not red.

## 11. Reference Mockups

Validated mockups live in `.superpowers/brainstorm/1756522-1782468848/content/`:
`directions.html` (3 directions), `directions-v2.html`–`v4` (synthesis), `dashboard-final.html` (all-ink, rejected as too somber), `dashboard-rich.html` (**approved final**).
