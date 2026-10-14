// Screenshots the bar of the design page's main screen, for side-by-side comparison
// with Kabuk's own rendering (docs/TESTING.md "Visual and contrast").
//
//   node tools/design-shot.js <out.png> [--look a] [--mode dark] [--lang tr] [--scale 1] [--pos top] [--dock] [--launcher <query>] [--toast] [--osd]
//
// With --dock the clip is Rıhtım's dock plus its 8 px distance instead of the bar; with
// --launcher the launcher is opened, the query typed, and the clip is the launcher; with
// --toast or --osd the page's sample notice or volume OSD is shown and clipped.
//
// The main 1920×1080 screen is taken out of the page's scaled preview so the bar is
// rendered at its real size; the clip covers the bar plus its distance from the edge.
"use strict";
const path = require("path");
const { chromium } = require("playwright");

function arg(name, fallback) {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : fallback;
}

async function main() {
  const out = path.resolve(process.argv[2] || "design-bar.png");
  const look = arg("look", "a");
  const mode = arg("mode", "dark");
  const lang = arg("lang", "tr");
  const scale = Number(arg("scale", "1"));
  const pos = arg("pos", null);
  const dock = process.argv.includes("--dock");
  const launcher = arg("launcher", null);
  const toast = process.argv.includes("--toast");
  const osd = process.argv.includes("--osd");
  const page = path.resolve(__dirname, "..", "docs", "design", "kabuk-tasarim.html");

  const browser = await chromium.launch();
  const ctx = await browser.newContext({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: scale });
  const tab = await ctx.newPage();
  await tab.goto("file://" + page);
  await tab.evaluate(({ look, mode, lang, pos }) => {
    localStorage.setItem("kabuk-prefs", JSON.stringify({ dir: look, mode, lang, pos }));
  }, { look, mode, lang, pos });
  await tab.reload();
  await tab.evaluate(() => document.fonts.ready);
  if (launcher !== null) {
    await tab.click("#btn-launch");
    await tab.fill("#launch-q", launcher);
    await tab.waitForTimeout(100);
  }
  if (toast) await tab.click("#btn-toast");
  if (osd) await tab.click("#btn-osd");
  const part = launcher !== null ? ".launcher" : toast ? ".toast" : osd ? ".osd" : dock ? ".dock" : ".bar";
  const alone = launcher !== null || toast || osd;
  const box = await tab.evaluate(({ part }) => {
    const screen = document.querySelector("#stage .screen");
    // Keep the icon sprite (<symbol> definitions) next to the screen.
    const sprite = document.querySelector("svg symbol")?.closest("svg");
    document.body.replaceChildren(...[sprite, screen].filter(Boolean));
    document.body.style.margin = "0";
    screen.style.transform = "none";
    screen.style.position = "relative";
    const bar = screen.querySelector(part).getBoundingClientRect();
    const s = screen.getBoundingClientRect();
    return { x: s.x, y: s.y, w: s.width, h: s.height, bar: { x: bar.x, y: bar.y, w: bar.width, h: bar.height } };
  }, { part });
  await tab.waitForTimeout(300);
  // Bar plus its edge distance on every side, within the screen; the dock with 8 px around it.
  const DOCK_DISTANCE = 8;
  const m = box.bar.x - box.x;
  const clip = alone
    ? { x: box.bar.x, y: box.bar.y, width: box.bar.w, height: box.bar.h }
    : dock
    ? {
        x: box.bar.x - DOCK_DISTANCE,
        y: box.bar.y - DOCK_DISTANCE,
        width: box.bar.w + 2 * DOCK_DISTANCE,
        height: box.bar.h + 2 * DOCK_DISTANCE,
      }
    : {
        x: box.x,
        y: box.y,
        width: box.bar.w + 2 * m,
        height: box.bar.y - box.y + box.bar.h + m,
      };
  await tab.screenshot({ path: out, clip });
  await browser.close();
  console.log(`${out} (${clip.width}x${clip.height} CSS px at ${scale}x; bar ${box.bar.w}x${box.bar.h} at ${box.bar.x - box.x},${box.bar.y - box.y})`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
