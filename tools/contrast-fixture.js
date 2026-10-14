// Generates tests/fixtures/contrast.json from the design page: for every look x mode x accent, the raw theme
// tokens (inputs) and the values the page's contrast engine produced (expected outputs). The native kabuk-theme
// engine must reproduce every output exactly.
// Usage: node tools/contrast-fixture.js docs/design/kabuk-tasarim.html tests/fixtures/contrast.json
// Needs Playwright with Chromium (npm i -D playwright; npx playwright install chromium).
const path = require("path");
const fs = require("fs");
let chromium;
try { ({ chromium } = require("playwright")); } catch { ({ chromium } = require("/opt/npm-tools/node_modules/playwright")); }

const file = path.resolve(process.argv[2] || "docs/design/kabuk-tasarim.html");
const out = path.resolve(process.argv[3] || "tests/fixtures/contrast.json");

(async () => {
  const launch = fs.existsSync("/opt/pw-browsers/chromium") ? { executablePath: "/opt/pw-browsers/chromium" } : {};
  const browser = await chromium.launch(launch);
  const page = await browser.newPage();
  await page.goto(require("url").pathToFileURL(file).href);
  await page.evaluate(() => localStorage.clear());
  await page.reload();
  await page.waitForTimeout(400);
  const cases = await page.evaluate(() => {
    const FITTED = ["--accent-s", "--accent-fill", "--on-accent", "--badge-fill", "--on-badge",
      "--fg-s", "--dim", "--app-fg", "--app-strong", "--app-dim", "--sx-k", "--sx-ty", "--sx-s", "--sx-c", "--sx-n", "--sx-fn",
      "--t-p", "--t-ok", "--t-d", "--ok-fg", "--run-fg", "--err-fg", "--accent2"];
    const INPUTS = ["--desk", "--surf", "--surf-sel", "--win-bg", "--surf-alt", "--app-chip", "--accent-s", "--on-accent", "--badge", "--on-badge-pref",
      "--fg-s", "--dim", "--app-fg", "--app-strong", "--app-dim", "--sx-k", "--sx-ty", "--sx-s", "--sx-c", "--sx-n", "--sx-fn",
      "--t-p", "--t-ok", "--t-d", "--ok-fg", "--run-fg", "--err-fg", "--ok-bg", "--run-bg", "--err-bg", "--accent2"];
    const click = (s) => document.querySelector(s).click();
    const accents = ["", "#8fd3ff", "#fbc419", "#7fe0b5", "#ff8f6b", "#b9a7ff", "#ff7eb6", "#ecebe7"];
    const res = [];
    for (const mode of ["dark", "light"]) {
      click(`[data-pagemode="${mode}"]`);
      for (const look of "abcdefg") {
        click(`#dir-${look}`);
        for (const acc of accents) {
          click('#set-stage [data-page="look"]');
          click(`#set-stage [data-accent="${acc}"]`);
          const sc = document.querySelector("#stage .screen");
          const expected = {};
          for (const p of FITTED) { const v = sc.style.getPropertyValue(p); if (v) expected[p.slice(2)] = v.trim(); }
          const saved = FITTED.map((p) => [p, sc.style.getPropertyValue(p)]);
          FITTED.forEach((p) => sc.style.removeProperty(p));
          const cs = getComputedStyle(sc);
          const input = {};
          for (const p of INPUTS) { const v = cs.getPropertyValue(p).trim(); if (v) input[p.slice(2)] = v; }
          saved.forEach(([p, v]) => v && sc.style.setProperty(p, v));
          res.push({ look, mode, accent: acc || null, input, expected });
        }
      }
    }
    return res;
  });
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, JSON.stringify({
    about: "Contrast engine fixture generated from docs/design/kabuk-tasarim.html. 'accent' null means the theme's own accent (input['accent-s']); otherwise the user-picked accent replaces it. 'expected' lists only tokens the engine changed or always sets (accent-s, accent-fill, on-accent). Minimum text contrast 4.5:1, target 4.62 to absorb hex rounding.",
    minText: 4.5,
    cases,
  }, null, 1));
  console.log(`${cases.length} cases -> ${out}`);
  await browser.close();
})();
