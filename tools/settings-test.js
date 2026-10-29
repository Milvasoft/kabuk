// The settings page end to end without WebView2 (docs/PLAN.md M8): the page runs in
// Playwright's Chromium, its messages go to the real Rust bridge
// (apps/kabuk-settings/examples/bridge_stdio.rs) and the settings land in a temporary
// kabuk.toml, which the checks read back.
//
//   node tools/settings-test.js [--shots <dir>]
//
// Needs `cargo build -p kabuk-settings --example bridge_stdio` first (tools/check.ps1
// does it). With --shots every page is also saved as <dir>/<page>.png.
//
// Checked: all 12 pages render without script errors; every choice, switch and chip on
// every page writes a value the bridge accepts; the main settings land in the file as
// TOML; the custom dimming delay refuses below 10 s, above 60 min and an empty value
// and saves a valid one; a shortcut is recorded from the keyboard; a window rule is
// added and removed; switching the language rewords the page; the Create tab makes a
// wallpaper with a local stand-in for Ollama (the first answer too bright and sent back,
// the second kept and applied), and an API key typed in is stored in Windows Credential
// Manager and in no file or log, then removed.
"use strict";
const fs = require("fs");
const os = require("os");
const path = require("path");
const readline = require("readline");
const { spawn } = require("child_process");
const http = require("http");
const { chromium } = require("playwright");

const root = path.resolve(__dirname, "..");
const exe = path.join(root, "target", "debug", "examples", "bridge_stdio.exe");
const ORIGIN = "https://kabuk.settings/";
const PAGES = ["look", "wall", "saver", "bar", "windows", "rules", "game", "launcher", "keys", "lang", "oled", "about"];
const TIMEOUT = 5000;
// Creating a wallpaper asks the model twice and checks each answer on the GPU.
const CREATE_TIMEOUT = 120000;
const SAMPLE_TOML = 'name = "Yavaş kurdeleler"\nauthor = "test"\nversion = 1\ntokens = ["accent"]\n';
const SAMPLE_HLSL = "float3 wall(float2 px)\n{\n    float2 uv = px / resolution.y;\n    float d = abs(uv.y - 0.5 - 0.18 * sin(uv.x * 1.3 + time * 0.2));\n    return tok_accent * (0.002 / (d + 0.002)) * 0.3;\n}\n";
const BRIGHT_HLSL = "float3 wall(float2 px)\n{\n    return float3(1, 1, 1) * step(0.7, frac(px.x / resolution.x + time * 0.05));\n}\n";
const KEY = "sk-ant-kabuk-settings-test-5d1f0c";

/** A stand-in for Ollama: its model list, and answers to /api/chat in turn. */
function fakeOllama(answers) {
  const seen = [];
  const server = http.createServer((req, res) => {
    let body = "";
    req.on("data", (c) => { body += c; });
    req.on("end", () => {
      seen.push({ url: req.url, body });
      res.setHeader("Content-Type", "application/json");
      if (req.url === "/api/tags") return res.end(JSON.stringify({ models: [{ name: "llama3:8b" }, { name: "test-coder:7b" }] }));
      const content = answers[Math.min(seen.filter((r) => r.url === "/api/chat").length - 1, answers.length - 1)];
      res.end(JSON.stringify({ message: { role: "assistant", content }, done: true }));
    });
  });
  return new Promise((ok) => server.listen(0, "127.0.0.1", () => ok({ server, seen, port: server.address().port })));
}

/** Every file under `dir` that holds `text`. */
function filesHolding(dir, text) {
  const found = [];
  const walk = (d) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const f = path.join(d, e.name);
      if (e.isDirectory()) walk(f);
      else if (fs.readFileSync(f).includes(text)) found.push(f);
    }
  };
  walk(dir);
  return found;
}

function arg(name) {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : null;
}

const MIME = { ".html": "text/html", ".css": "text/css", ".js": "text/javascript", ".svg": "image/svg+xml", ".ttf": "font/ttf" };
let failures = 0;
function check(ok, what) {
  if (ok) console.log(`ok    ${what}`);
  else { console.log(`FAIL  ${what}`); failures++; }
}

async function main() {
  const shots = arg("shots");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "kabuk-settings-"));
  const config = path.join(dir, "kabuk.toml");
  fs.writeFileSync(config, "schema = 1\n# the user's own comment\n[language]\nlanguage = \"tr\"\n");
  const res = path.join(dir, "res");
  const bridge = spawn(exe, ["--config", config, "--state-dir", dir, "--resources", res], { stdio: ["pipe", "pipe", "inherit"] });
  const lines = readline.createInterface({ input: bridge.stdout });
  // Wait for the resources to be written: the first answer.
  const first = new Promise((ok) => lines.once("line", ok));
  bridge.stdin.write('{"op":"info"}\n');
  await first;

  const browser = await chromium.launch();
  const ctx = await browser.newContext({ viewport: { width: 1200, height: 740 } });
  const page = await ctx.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
  await page.route(`${ORIGIN}**`, (route) => {
    const file = path.join(res, decodeURIComponent(new URL(route.request().url()).pathname));
    if (!fs.existsSync(file)) return route.fulfill({ status: 404, body: "" });
    route.fulfill({ status: 200, contentType: MIME[path.extname(file)] || "application/octet-stream", body: fs.readFileSync(file) });
  });
  // The bridge answers in order; the page sees each answer as WebView2's message event.
  // The Create tab's steps, the update check's result and the gallery's previews come from
  // another thread and answer nothing.
  const UNASKED = ["gen-step", "gen-done", "gen-failed", "gallery-preview"];
  const unasked = (m) => UNASKED.includes(m.op) || (m.op === "update" && m.state !== "checking");
  let sent = 0, answered = 0, refused = 0;
  const pending = [];
  lines.on("line", async (line) => {
    const m = line ? JSON.parse(line) : {};
    const op = m.op ?? null;
    if (!unasked(m)) answered++;
    if (!line) return;
    if (op === "refused") refused++;
    pending.push(page.evaluate((text) => window.__fromRust(text), line));
  });
  await page.exposeFunction("__toRust", (text) => {
    sent++;
    bridge.stdin.write(text.replace(/\n/g, " ") + "\n");
  });
  await page.addInitScript(() => {
    const listeners = [];
    window.chrome = { webview: { postMessage: (m) => window.__toRust(m), addEventListener: (_, f) => listeners.push(f) } };
    window.__fromRust = (text) => listeners.forEach((f) => f({ data: text }));
  });
  const settled = async () => {
    const until = Date.now() + TIMEOUT;
    while ((answered < sent || pending.length) && Date.now() < until) {
      await Promise.all(pending.splice(0));
      await new Promise((r) => setTimeout(r, 20));
    }
    await page.waitForTimeout(30);
  };
  const toml = () => fs.readFileSync(config, "utf8");
  // For answers that take longer than a round trip: waits until `f()` holds.
  const until = async (f, ms = TIMEOUT) => {
    const end = Date.now() + ms;
    while (!(await f()) && Date.now() < end) await page.waitForTimeout(50);
    await settled();
    return f();
  };
  const go = async (p) => { await page.click(`[data-page="${p}"]`); await settled(); };

  await page.goto(`${ORIGIN}index.html`);
  await page.waitForSelector(".set-nav");
  await settled();
  check((await page.locator(".set-nav").count()) === 12, "12 pages in the navigation");
  check((await page.locator(".set-nav").first().innerText()).includes("Görünüm"), "Turkish words from the language files");

  // A short window: the menu scrolls, so its last page can be reached (found by the user:
  // the last pages were cut off).
  await page.setViewportSize({ width: 900, height: 420 });
  const side = await page.$eval(".set-side", (s) => ({ scroll: s.scrollHeight, client: s.clientHeight, overflow: getComputedStyle(s).overflowY }));
  check(side.overflow === "auto" && side.scroll > side.client, `the short window's menu scrolls (${side.scroll} in ${side.client})`);
  await page.locator(`[data-page="about"]`).scrollIntoViewIfNeeded();
  const last = await page.$eval(`[data-page="about"]`, (b) => { const r = b.getBoundingClientRect(); return r.bottom <= window.innerHeight && r.top >= 0; });
  check(last, "the menu's last page scrolled into view");
  await page.click(`[data-page="about"]`);
  await settled();
  check((await page.locator(`[data-page="about"].on`).count()) === 1, "the menu's last page opens in a short window");
  await page.setViewportSize({ width: 1200, height: 740 });
  await page.$eval(".set-side", (s) => { s.scrollTop = 0; });

  // Every page draws; every choice, switch and chip on it writes an accepted value.
  for (const p of PAGES) {
    await go(p);
    check((await page.locator(".set-main h3").count()) >= 1, `${p}: drawn`);
    if (shots) {
      fs.mkdirSync(shots, { recursive: true });
      await page.screenshot({ path: path.join(shots, `${p}.png`) });
    }
    const controls = await page.locator("[data-set], [data-tgl], [data-wall]:not([data-wall=image])").count();
    const before = refused;
    for (let i = 0; i < controls; i++) {
      const c = page.locator("[data-set], [data-tgl], [data-wall]:not([data-wall=image])").nth(i);
      if (!(await c.count()) || !(await c.isEnabled())) continue;
      await c.click();
      await settled();
    }
    check(refused === before, `${p}: ${controls} controls written, none refused`);
  }
  check(toml().includes("# the user's own comment"), "the user's comment kept");

  // Specific values land in the file as TOML.
  const restart = async () => { fs.writeFileSync(config, "schema = 1\n[language]\nlanguage = \"tr\"\n"); await page.evaluate(() => window.chrome.webview.postMessage('{"op":"load"}')); await settled(); };
  await restart();
  await go("bar");
  await page.click(`[data-set="bar.background"][data-val='"blur"']`); await settled();
  check(/background = "blur"/.test(toml()), "bar background: blur");
  await page.click(`[data-tgl="bar.autohide"]`); await settled();
  check(/autohide = true/.test(toml()), "bar auto-hide on");
  await page.click(`[data-tgl="bar.modules.taskbar"]`); await settled();
  check(/taskbar = false/.test(toml()), "apps module off");
  // DECISIONS D56: the app grid's button, named in the page's language.
  const gridChip = await page.$eval(`[data-tgl="bar.modules.app_grid"]`, (el) => el.textContent);
  check(gridChip.includes("Tüm uygulamalar"), `app grid button chip named: ${gridChip}`);
  await page.click(`[data-tgl="bar.modules.app_grid"]`); await settled();
  check(/app_grid = false/.test(toml()), "app grid button off");
  // DECISIONS D57: the bar's colour from a swatch; a light one warns about the text; the
  // look's own takes it away again.
  await page.click(`[data-set="bar.color"][data-val='"#1E1433"']`); await settled();
  check(/color = "#1E1433"/.test(toml()) && (await page.locator(".sw-warn").count()) === 0, "bar colour: a dark swatch, no warning");
  await page.click(`[data-set="bar.color"][data-val='"#E9E6E1"']`); await settled();
  check(/color = "#E9E6E1"/.test(toml()) && (await page.locator(".sw-warn").count()) === 1, "bar colour: a light swatch warns about the text");
  await page.$eval("#bar-color", (el) => { el.value = "#204060"; el.dispatchEvent(new Event("change", { bubbles: true })); });
  await settled();
  check(/color = "#204060"/.test(toml()), "bar colour: any colour from the picker");
  await page.click(`[data-set="bar.color"][data-val='""']`); await settled();
  check(!/color = "#/.test(toml()), "bar colour: the look's own again");
  await page.$eval("#bar-height", (el) => { el.value = "48"; el.dispatchEvent(new Event("input", { bubbles: true })); el.dispatchEvent(new Event("change", { bubbles: true })); });
  await settled();
  check(/height = 48/.test(toml()), "bar height 48");
  await page.click(`[data-reset="bar.height"]`); await settled();
  check(!/height =/.test(toml()), "bar height back to the theme's");
  await page.$eval("#tray-shown", (el) => { el.value = "5"; el.dispatchEvent(new Event("change", { bubbles: true })); });
  await settled();
  check(/tray_shown = 5/.test(toml()), "tray icons on the bar: 5");

  await go("wall");
  await page.click(`[data-wall="gradient"]`); await settled();
  check(/kind = "gradient"/.test(toml()), "wallpaper: night gradient");
  await page.click(`[data-wall="windows"]`); await settled();
  check(/kind = "windows"/.test(toml()), "wallpaper: Windows' own");
  // DECISIONS D58: the gallery's wallpapers, live and still, each with its preview as the
  // bridge draws it; one chosen.
  check((await page.locator(`[data-wall="gallery"]`).count()) >= 20, "gallery: twenty wallpapers or more");
  check((await page.locator(".set-main").innerText()).includes("Galeri · canlı duvar kâğıtları"), "gallery: its sections named");
  await page.click(`[data-gal="wall-live/aurora-shore"]`); await settled();
  check(/kind = "gallery"/.test(toml()) && /gallery = "wall-live\/aurora-shore"/.test(toml()), "gallery: a live wallpaper chosen");
  await page.click(`[data-gal="wall-still/agate-slice"]`); await settled();
  check(/gallery = "wall-still\/agate-slice"/.test(toml()), "gallery: a still wallpaper chosen");
  await page.waitForFunction(() => document.querySelectorAll("img[data-gprev][src^='data:image/jpeg']").length >= 20, null, { timeout: 120000 }).catch(() => {});
  check((await page.locator("img[data-gprev][src^='data:image/jpeg']").count()) >= 20, "gallery: the previews came");
  await page.click(`[data-wall="windows"]`); await settled();
  await go("saver");
  check((await page.locator(`[data-saver-gal]`).count()) >= 20, "gallery: twenty screen savers or more");
  await page.click(`[data-saver-gal="saver-still/ember-enso"]`); await settled();
  check(/\[screensaver\][^[]*kind = "gallery"/.test(toml()) && /gallery = "saver-still\/ember-enso"/.test(toml()), "gallery: a still screen saver chosen");
  // The live wallpapers play as screen savers too, saying they are not for OLED screens.
  await page.click(`[data-saver-gal="wall-live/aurora-shore"]`); await settled();
  check(/\[screensaver\][^[]*gallery = "wall-live\/aurora-shore"/.test(toml()), "gallery: a live wallpaper chosen as the screen saver");
  check((await page.locator(`[data-saver-gal="wall-live/aurora-shore"]`).innerText()).includes("OLED"), "gallery: a live wallpaper as a saver warns about OLED");
  await page.click(`[data-set="screensaver.kind"][data-val='"circuit"']`); await settled();

  // The drop-down terminal (DECISIONS D62): its program and height written.
  await go("windows");
  await page.fill(`[data-text="dropdown.command"]`, "pwsh -NoLogo");
  await page.press(`[data-text="dropdown.command"]`, "Tab"); await settled();
  check(/\[dropdown\][^[]*command = "pwsh -NoLogo"/.test(toml()), "dropdown: its program written");
  await page.$eval(`[data-range="dropdown.height"]`, (el) => { el.value = "60"; el.dispatchEvent(new Event("change", { bubbles: true })); }); await settled();
  check(/\[dropdown\][^[]*height = 60/.test(toml()), "dropdown: its height written");
  check((await page.locator(`[data-chord="shortcuts.dropdown"]`).first().innerText()).includes("Enter"), "dropdown: its shortcut beside it");

  // Hot corners (asked for by the user): none by default, each corner's choice written.
  await go("windows");
  check((await page.locator(`[data-select^="hot_corners."]`).count()) === 4, "hot corners: four corners");
  check(!/\[hot_corners\]/.test(toml()), "hot corners: none by default");
  await page.selectOption(`[data-select="hot_corners.top_left"]`, "app_grid"); await settled();
  check(/\[hot_corners\][^[]*top_left = "app_grid"/.test(toml()), "hot corners: top left opens every app");
  await page.selectOption(`[data-select="hot_corners.top_left"]`, "none"); await settled();
  check(/top_left = "none"/.test(toml()), "hot corners: back to nothing");

  // Touchpad swipes (DECISIONS D68): off by default, the ways shown once on, the fingers
  // and each way's choice written; Windows' touchpad page a button away (not pressed here).
  check((await page.locator(`[data-tgl="gestures.enabled"]`).getAttribute("aria-checked")) === "false", "gestures: off by default");
  check((await page.locator(`[data-select^="gestures."]`).count()) === 0, "gestures: no ways while off");
  check((await page.locator(`[data-touchpad-settings]`).count()) === 1, "gestures: Windows' touchpad page a button away");
  await page.click(`[data-tgl="gestures.enabled"]`); await settled();
  check(/\[gestures\][^[]*enabled = true/.test(toml()), "gestures: turned on");
  check((await page.locator(`[data-select^="gestures."]`).count()) === 4, "gestures: four ways");
  await page.click(`[data-set="gestures.fingers"][data-val='4']`); await settled();
  check(/fingers = 4/.test(toml()), "gestures: four fingers");
  await page.selectOption(`[data-select="gestures.swipe_up"]`, "launcher"); await settled();
  check(/swipe_up = "launcher"/.test(toml()), "gestures: up opens the launcher");
  if (shots) {
    await page.locator("[data-touchpad-settings]").scrollIntoViewIfNeeded();
    await page.screenshot({ path: path.join(shots, "windows-gestures.png") });
  }
  await page.click(`[data-tgl="gestures.enabled"]`); await settled();
  check(/enabled = false/.test(toml()), "gestures: off again");

  // The all-apps grid (asked for by the user): an app hidden on the grid is listed and
  // shown again, the order goes back to alphabetical.
  const gridFile = path.join(dir, "app-grid.json");
  fs.writeFileSync(gridFile, JSON.stringify({ order: ["b", "a"], hidden: [{ open: "shell:AppsFolder\\X", name: "Hidden app" }] }));
  await go("launcher");
  const chip = page.locator(`[data-tgl="launcher.sources.recent"]`);
  await chip.click(); await settled(); await chip.click(); await settled();
  check((await page.locator(".set-main").innerText()).includes("Hidden app"), "app grid: a hidden app listed");
  check((await page.locator(".set-part h4").innerText()).length > 0, "app grid: a part of its own, titled");
  await page.locator(`[data-grid-show]`).first().click(); await settled();
  check(JSON.parse(fs.readFileSync(gridFile, "utf8")).hidden.length === 0, "app grid: the hidden app shown again");
  check(!(await page.locator(".set-main").innerText()).includes("Hidden app"), "app grid: no longer listed");
  check(await page.locator(`[data-grid-reset]`).isEnabled(), "app grid: an order to reset");
  await page.click(`[data-grid-reset]`); await settled();
  check(JSON.parse(fs.readFileSync(gridFile, "utf8")).order.length === 0, "app grid: back to alphabetical order");
  check(!(await page.locator(`[data-grid-reset]`).isEnabled()), "app grid: nothing more to reset");

  await go("look");
  await page.click(`[data-set="appearance.look"][data-val='"cizgi"']`); await settled();
  check(/look = "cizgi"/.test(toml()) && (await page.getAttribute(".screen", "data-dir")) === "b", "look Çizgi, page restyled");
  // Ten looks (DECISIONS D67): the design's but Piksel.
  check((await page.locator(`[data-set="appearance.look"]`).count()) === 10 && (await page.locator(`[data-val='"piksel"']`).count()) === 0, "looks: ten cards, no Piksel");
  for (const [name, dir] of [["pano", "h"], ["izgara", "i"], ["defter", "j"]]) {
    await page.click(`[data-set="appearance.look"][data-val='"${name}"']`); await settled();
    check(new RegExp(`look = "${name}"`).test(toml()) && (await page.getAttribute(".screen", "data-dir")) === dir, `look ${name}, page restyled`);
  }
  await page.click(`[data-set="appearance.look"][data-val='"cizgi"']`); await settled();
  await page.click(`[data-set="appearance.accent"][data-val='"#fbc419"']`); await settled();
  check(/accent = "#fbc419"/.test(toml()), "accent Milva yellow");
  await page.click(`[data-set="appearance.animations"][data-val='"reduced"']`); await settled();
  check(/animations = "reduced"/.test(toml()), "animations reduced");
  // Cursors picked apart from the look (DECISIONS D41): the pickers show only for
  // "custom", each card previewed; the size shows with any of Kabuk's sets.
  check((await page.$$(".cursor-card")).length === 0 && (await page.$$(`[data-set="appearance.cursor_size"]`)).length === 0, "cursors: no pickers with the user's own");
  await page.click(`[data-set="appearance.cursors"][data-val='"kabuk"']`); await settled();
  check(/cursors = "kabuk"/.test(toml()) && (await page.$$(`[data-set="appearance.cursor_size"]`)).length === 3 && (await page.$$(".cursor-card")).length === 0, "cursors: the look's set, size shown");
  await page.click(`[data-set="appearance.cursors"][data-val='"custom"']`); await settled();
  const cards = await page.$$eval(".cursor-card", (els) => els.map((e) => e.querySelectorAll(".cp-i svg").length));
  check(cards.length === 10 && cards.every((n) => n === 3), `cursors: 7 styles and 3 colours, each previewed (${cards})`);
  await page.click(`[data-set="appearance.cursor_style"][data-val='"cartoon"']`); await settled();
  check(/cursor_style = "cartoon"/.test(toml()), "cursors: the cartoon set picked");
  await page.click(`[data-set="appearance.cursor_style"][data-val='"block"']`); await settled();
  await page.click(`[data-set="appearance.cursor_color"][data-val='"accent"']`); await settled();
  await page.click(`[data-set="appearance.cursor_size"][data-val='"huge"']`); await settled();
  check(/cursor_style = "block"/.test(toml()) && /cursor_color = "accent"/.test(toml()) && /cursor_size = "huge"/.test(toml()), "cursors: style, colour and size saved");
  check(await page.$eval(`[data-set="appearance.cursor_style"][data-val='"block"']`, (e) => e.classList.contains("on")), "cursors: the picked style is marked");
  if (shots) await page.screenshot({ path: path.join(shots, "look-cursors.png"), fullPage: true });
  await page.click(`[data-set="appearance.cursors"][data-val='"windows"']`); await settled();
  check(/cursors = "windows"/.test(toml()), "cursors: the user's own again");

  // The system theme's colour layer (DECISIONS D42): off by default, a switch on the look page.
  await go("look");
  check(await page.$eval(`[data-tgl="system_theme.colors"]`, (e) => e.getAttribute("aria-checked") === "false"), "system colours: off by default");
  await page.click(`[data-tgl="system_theme.colors"]`); await settled();
  check(/\[system_theme\]\s*colors = true/.test(toml()), "system colours: on");
  await page.click(`[data-tgl="system_theme.colors"]`); await settled();
  check(/colors = false/.test(toml()) || !/colors = true/.test(toml()), "system colours: off again");

  // The minimise effect: the genie by default, Windows' own when chosen.
  await go("windows");
  check(await page.$eval(`[data-set="windows.minimize_effect"][data-val='"genie"']`, (e) => e.getAttribute("aria-pressed") === "true" || e.classList.contains("on")), "minimise: the genie by default");
  await page.click(`[data-set="windows.minimize_effect"][data-val='"windows"']`); await settled();
  check(/minimize_effect = "windows"/.test(toml()), "minimise: Windows' own");
  await page.click(`[data-set="windows.minimize_effect"][data-val='"genie"']`); await settled();
  check(/minimize_effect = "genie"/.test(toml()), "minimise: the genie again");

  // PLAN M19 U11: the window manager's settings that were in kabuk.toml alone: how many
  // workspaces, how many on the bar, the tiling layout and the title's double click.
  const slide = async (id, value) => {
    await page.$eval(`#${id}`, (el, v) => { el.value = v; el.dispatchEvent(new Event("input", { bubbles: true })); el.dispatchEvent(new Event("change", { bubbles: true })); }, value);
    await settled();
  };
  await slide("ws-count", "6");
  check(/\[windows\][^[]*\nworkspaces = 6/.test(toml()) && (await page.locator(`[data-v="ws-count"]`).innerText()) === "6", "workspaces: six");
  await slide("ws-shown", "5");
  check(/shown_workspaces = 5/.test(toml()), "workspaces on the bar: five");
  check(await page.$eval(`[data-set="windows.tiling_layout"][data-val='"dwindle"']`, (e) => e.classList.contains("on")), "tiling layout: dwindle by default");
  await page.click(`[data-set="windows.tiling_layout"][data-val='"master"']`); await settled();
  check(/tiling_layout = "master"/.test(toml()), "tiling layout: master");
  check(await page.$eval(`[data-tgl="windows.double_click_to_tile"]`, (e) => e.getAttribute("aria-checked") === "true"), "double click: on by default");
  await page.click(`[data-tgl="windows.double_click_to_tile"]`); await settled();
  check(/double_click_to_tile = false/.test(toml()), "double click: off");
  for (const p of ["windows.workspaces", "windows.shown_workspaces", "windows.tiling_layout", "windows.double_click_to_tile"]) {
    await page.click(`[data-reset="${p}"]`); await settled();
  }
  check(!/workspaces =|tiling_layout|double_click_to_tile/.test(toml()), "window manager settings back to their defaults");

  // The mode's hint says what the chosen mode does.
  await go("look");
  const modeHint = () => page.$eval(`[data-set="appearance.mode"]`, (e) => e.closest(".set-row").querySelector(".h").textContent);
  await page.click(`[data-set="appearance.mode"][data-val='"dark"']`); await settled();
  const darkHint = await modeHint();
  await page.click(`[data-set="appearance.mode"][data-val='"time"']`); await settled();
  const timeHint = await modeHint();
  check(darkHint.includes("koyu") && timeHint.includes("07:00") && darkHint !== timeHint, `mode: the hint follows the choice (${darkHint} / ${timeHint})`);

  // A still picture's drift (DECISIONS D47): on by default, a switch on the OLED page.
  await go("oled");
  check(await page.$eval(`[data-tgl="wallpaper.drift"]`, (e) => e.getAttribute("aria-checked") === "true"), "drift: on by default");
  await page.click(`[data-tgl="wallpaper.drift"]`); await settled();
  check(/drift = false/.test(toml()), "drift: off");
  await page.click(`[data-tgl="wallpaper.drift"]`); await settled();

  // Dimming delay: 10 s to 60 min, whole numbers.
  await go("oled");
  await page.click(`[data-dim="custom"]`); await settled();
  // Value and unit typed together, as one entry.
  const dim = async (value, unit) => {
    await page.$eval("#dim-unit", (el, u) => { el.value = u; }, unit);
    await page.$eval("#dim-value", (el, v) => { el.value = v; el.dispatchEvent(new Event("change", { bubbles: true })); }, value);
    await settled();
    return page.$eval("#dim-hint", (el) => el.classList.contains("err"));
  };
  const saved = () => (toml().match(/dim_after_secs = (\d+)/) || [])[1];
  check((await dim("5", "s")) && saved() === undefined, "dimming: 5 s refused");
  check((await dim("61", "m")) && saved() === undefined, "dimming: 61 min refused");
  check((await dim("", "s")) && saved() === undefined, "dimming: empty refused");
  check((await dim("2.5", "m")) && saved() === undefined, "dimming: 2.5 min refused");
  check(!(await dim("2", "m")) && saved() === "120", "dimming: 2 min saved as 120 s");
  check(!(await dim("45", "s")) && saved() === "45", "dimming: 45 s saved");

  // A shortcut from the keyboard.
  await go("keys");
  const press = async (keys) => {
    await page.click(`[data-chord="shortcuts.launcher"][data-index="0"]`); await settled();
    await page.keyboard.press(keys); await settled();
  };
  // Ctrl+Alt is AltGr on a Turkish keyboard: refused, said so, the file unchanged.
  await press("Control+Alt+K");
  check(!/launcher =/.test(toml()) && (await page.locator("[role=alert]").count()) === 1, "Ctrl+Alt+K refused and said so");
  // PLAN M19 U2: under its row, and why.
  check((await page.locator("[data-refusal]").count()) === 1 && (await page.locator("[data-refusal]").innerText()).includes("AltGr"), "the refusal says why, under the shortcut");
  await press("Control+Shift+K");
  check(/launcher = \["Ctrl\+Shift\+K"\]/.test(toml()) && (await page.locator("[role=alert]").count()) === 0, "launcher shortcut recorded as Ctrl+Shift+K");
  // A Win chord never reaches the page: Rust takes it while the recorder waits and sends
  // it as {"op":"chord"} (D40); Esc that way cancels.
  const fromRust = async (m) => { await page.evaluate((t) => window.__fromRust(t), JSON.stringify(m)); await settled(); };
  await page.click(`[data-chord="shortcuts.launcher"][data-index="0"]`); await settled();
  await fromRust({ op: "chord", chord: "Win+Shift+R" });
  check(/launcher = \["Win\+Shift\+R"\]/.test(toml()), "launcher shortcut recorded as Win+Shift+R from Rust");
  await page.click(`[data-chord="shortcuts.launcher"][data-index="0"]`); await settled();
  await fromRust({ op: "chord", chord: "Esc" });
  check(/launcher = \["Win\+Shift\+R"\]/.test(toml()) && (await page.locator(".keys > kbd").count()) === 0, "Esc from Rust cancels the recorder");
  await fromRust({ op: "chord", chord: "Win+Q" });
  check(!/Win\+Q/.test(toml()), "a chord with no recorder waiting changes nothing");
  // DECISIONS D56: the app grid's shortcut is on the page, with its default, and records.
  check((await page.locator(".set-main").innerText()).includes("Tüm uygulamaları göster"), "app grid shortcut listed");
  await page.click(`[data-chord="shortcuts.app_grid"][data-index="0"]`); await settled();
  await fromRust({ op: "chord", chord: "Win+Shift+G" });
  check(/app_grid = \["Win\+Shift\+G"\]/.test(toml()), "app grid shortcut recorded as Win+Shift+G");
  await go("keys");
  await page.click(`[data-reset="shortcuts.app_grid"]`); await settled();
  check(!/app_grid = \[/.test(toml()), "app grid shortcut back to its default");

  // Back to the default (asked for by the user): a changed shortcut has its way back, and
  // the keys page resets them all; an unchanged setting shows none.
  await go("keys");
  check((await page.$$(`[data-reset="shortcuts.launcher"]`)).length === 1 && (await page.$$("[data-reset-keys]")).length === 1, "reset: offered for a changed shortcut");
  check((await page.$$(`[data-reset="shortcuts.close_window"]`)).length === 0, "reset: none for an unchanged one");
  await page.click(`[data-reset="shortcuts.launcher"]`); await settled();
  check(!/launcher =/.test(toml()) && (await page.$$("[data-reset-keys]")).length === 0, "reset: the launcher back to its default");
  await go("windows");
  await page.click(`[data-set="windows.minimize_effect"][data-val='"windows"']`); await settled();
  await page.click(`[data-reset="windows.minimize_effect"]`); await settled();
  check(!/minimize_effect/.test(toml()), "reset: a choice back to its default");

  // PLAN M19 U11 (SPEC §2.3): the default rules can be turned off; the table then lists
  // none of them.
  await go("rules");
  const defaultRows = () => page.locator(".rules-t .src:not(.mine):not(.h)").count();
  check((await defaultRows()) > 0, "default rules: listed while on");
  await page.click(`[data-tgl="rules.defaults"]`); await settled();
  check(/\[rules\][^[]*defaults = false/.test(toml()) && (await defaultRows()) === 0, "default rules: off, none listed");
  await page.click(`[data-tgl="rules.defaults"]`); await settled();
  check(/defaults = true/.test(toml()) && (await defaultRows()) > 0, "default rules: on again");

  // A window rule added and removed.
  await go("rules");
  await page.click(`[data-rule-new]`); await settled();
  await page.fill("#rule-exe", "Discord.exe");
  await page.selectOption("#rule-what", "ws5");
  await page.click(`[data-rule-save]`); await settled();
  check(/\[\[rules\.rule\]\]\s*exe = "Discord\.exe"\s*workspace = 5/.test(toml()), "rule: Discord.exe on workspace 5");
  await page.click(`[data-rule-drop="0"]`); await settled();
  check(!/rules\.rule/.test(toml()), "rule removed");

  // Check for updates: Kabuk's latest GitHub release is asked for and what it said shown
  // (up to date, a newer version, or GitHub unreachable when offline).
  await go("about");
  await page.click("[data-update-check]");
  const said = async () => page.locator("[data-update-said]").getAttribute("data-update-said").catch(() => null);
  check(await until(async () => ["latest", "newer", "failed"].includes(await said()), 15000), "update: checked and answered");
  check((await page.locator("[data-update-check][disabled]").count()) === 0, "update: can be checked again");

  // Create: a wallpaper from a stand-in Ollama; its first answer is too bright and goes
  // back to the model with the reason, the second passes, is previewed and applied.
  const reply = (hlsl) => "```toml\n" + SAMPLE_TOML + "```\n```hlsl\n" + hlsl + "```\n";
  const ollama = await fakeOllama([reply(BRIGHT_HLSL), reply(SAMPLE_HLSL)]);
  fs.writeFileSync(config, `schema = 1\n[language]\nlanguage = "tr"\n[create]\nprovider = "ollama"\nollama_url = "http://127.0.0.1:${ollama.port}"\n`);
  await page.evaluate(() => window.chrome.webview.postMessage('{"op":"load"}')); await settled();
  await go("wall");
  await page.click(`[data-walltab="gen"]`); await settled();
  check(await page.locator("[data-genrun]").isDisabled(), "create: waits for a connection");
  await page.click("[data-genconnect]"); await settled();
  check((await page.locator(".conn .st").innerText()).includes("test-coder:7b"), "create: Ollama's coding model found");
  // A CI runner has no graphics card: every wallpaper takes too long a frame there and
  // is refused, as it would be on such a machine. The creation runs on the host and in
  // the test VM (docs/TESTING.md).
  if (process.env.CI) {
    console.log("skip  create: no graphics card on a CI runner");
  } else {
    await page.fill("#gen-prompt", "yavaş kurdeleler");
    await page.click("[data-genrun]");
    await page.waitForSelector("[data-genapply]", { timeout: CREATE_TIMEOUT });
    await settled();
    const chats = ollama.seen.filter((r) => r.url === "/api/chat").map((r) => JSON.parse(r.body));
    check(chats.length === 2 && chats[0].model === "test-coder:7b", `create: the model asked twice (${chats.length})`);
    const fix = chats[1] ? chats[1].messages[chats[1].messages.length - 1].content : "";
    check(/average brightness/.test(fix), "create: the refusal's reason went back to the model");
    check((await page.locator(".gen-steps li.ok").count()) === 4, "create: every step passed");
    check((await page.locator("[data-gentries]").innerText()).includes("2/3"), "create: the second try shown");
    check((await page.locator(".gen-res img").getAttribute("src") || "").startsWith("data:image/bmp;base64,"), "create: previewed");
    if (shots) await page.screenshot({ path: path.join(shots, "wall-create.png") });
    await page.click("[data-genapply]");
    check(await until(() => /kind = "generated"/.test(toml()) && /generated = "gen-\d+"/.test(toml())), `create: applied as the wallpaper${errors.length ? " " + errors.join(" | ") : ""}`);
    const kept = fs.readdirSync(path.join(dir, "walls"));
    check(kept.length === 1 && fs.readFileSync(path.join(dir, "walls", kept[0], "wall.hlsl"), "utf8") === SAMPLE_HLSL, "create: kept in the state folder");
    const card = page.locator(`[data-gen="${kept[0]}"]`);
    check((await card.count()) === 1 && (await card.getAttribute("aria-pressed")) === "true", "create: chosen in the list");
  }
  ollama.server.close();

  // A key typed in for Claude lands in Credential Manager only, and goes on Disconnect.
  await page.click(`[data-walltab="gen"]`); await settled();
  await page.click(`[data-set="create.provider"][data-val='"claude"']`); await settled();
  await page.click("[data-genkey]"); await settled();
  await page.fill("#gen-key", KEY);
  await page.click("[data-genkeysave]"); await settled();
  check((await page.locator("[data-genforget]").count()) === 1, "key: stored, shown as connected");
  check(filesHolding(dir, KEY).length === 0, `key: in no file or log (${filesHolding(dir, KEY).join(", ")})`);
  check(!(await page.content()).includes(KEY), "key: not left in the page");
  await page.click("[data-genforget]"); await settled();
  check((await page.locator("[data-genkey]").count()) === 1, "key: removed on Disconnect");

  // The language switches the words.
  await go("lang");
  await page.click(`[data-set="language.language"][data-val='"en"']`); await settled();
  check((await page.locator(".set-nav").first().innerText()).includes("Appearance"), "English after switching");

  // PLAN M19 U1: a mistake in kabuk.toml is shown, with where, and a way to the file.
  const good = toml();
  fs.writeFileSync(config, good + "\nbroken = [\n");
  await page.reload(); await settled();
  const banner = await page.locator(".set-main .note[role=alert]").first().innerText().catch(() => "");
  check(banner.includes("kabuk.toml") && (await page.locator("[data-open-config]").count()) === 1, `the file's mistake is shown: ${banner}`);
  fs.writeFileSync(config, good);

  check(errors.length === 0, `no script errors${errors.length ? ": " + errors.join(" | ") : ""}`);
  await browser.close();
  bridge.stdin.end();
  fs.rmSync(dir, { recursive: true, force: true });
  if (failures) { console.log(`${failures} check(s) failed`); process.exit(1); }
  console.log("settings page checks passed");
}

main().catch((e) => { console.error(e); process.exit(1); });
