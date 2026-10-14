# Design page

`kabuk-tasarim.html` is a single-file, live design document. Open it in a browser (fonts are loaded from Google Fonts, an internet connection is required).

## What is on the page

- Seven looks (Ada, Çizgi, Devre, Ray, Konsol, Rıhtım, Etkinlik), each in dark and light mode.
- Live preview: launcher (real search, Turkish letter rules, calculator), workspace switching, notification, OSD, idle view, game mode, floating windows (drag, resize, double-click to re-tile), bar position (four edges), language (tr/en).
- Settings app: all 12 pages work (the Create flow on the Wallpaper page is simulated as an example).
- Two-monitor view, components, OLED rules, acceptance criteria, roadmap.

## How to carry it over to native code

| What | Where | How |
| --- | --- | --- |
| Color and size tokens | In `<style>`: `.screen { … }`, `.screen[data-dir="x"]`, `.screen[data-mode="light"][data-dir="x"]` | Extracted to `themes/*.toml` by the `tools/` script (M2) |
| Component styles | In the same stylesheet: `.screen[data-dir="x"] .bar`, `.ws`, `.grp`, `.launcher`, `.toast`, `.osd` … | The per-look drawing code is written by referring to these |
| Strings | `L.tr` and `L.en` in `<script>` | Extracted to `locales/tr.ftl`, `locales/en.ftl` |
| Icons | 23 `<symbol>`s inside the `<svg>` at the top of the page | SVG paths are extracted to `assets/icons/` |
| Contrast engine | `fitContrast`, `fillPair`, `adaptAccent` | Reference: `tools/contrast-reference.js`; expected results: `tests/fixtures/contrast.json` |
| Search | `fuzzy()`, `search()`, `calc()` | Ported to `kabuk-search`, tested with the same examples |

## Sample content

The contents of the windows (editor, terminal, browser, notes) are sample content; Kabuk does not draw them. The contrast engine still checks these sample colors too, because the same rule will later apply to plugins and backgrounds as well.

## Audit

```
npm install
npx playwright install chromium
npm run audit:contrast
```

When a color changes in the design, `tests/fixtures/contrast.json` is regenerated with `npm run fixture:contrast`, and the native tests must pass against it.
