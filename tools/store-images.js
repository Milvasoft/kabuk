// Makes the Microsoft Store's listing images from the logo's vector masters (DECISIONS
// D84): the app tile icon at the sizes the Store shows it, the 1:1 box art, the 9:16
// poster art and the 16:9 hero art. Each SVG is drawn by Chromium at the exact pixel
// size, so nothing is scaled up from a smaller PNG.
//
//   node tools/store-images.js
//
// The images land in design/logo/png/store/.
"use strict";
const fs = require("fs");
const path = require("path");
const { pathToFileURL } = require("url");
const { chromium } = require("playwright");

const LOGO = path.resolve(__dirname, "..", "design", "logo");
const OUT = path.join(LOGO, "png", "store");
// The dark tile's ground (design/logo/source/generate.py, TILE_D).
const GROUND = "#121317";
// How much of the image's width the mark or the lockup takes.
const BOX_MARK = 0.62;
const POSTER_LOCKUP = 0.66;
const HERO_MARK = 0.36;

const IMAGES = [
  // The app tile icon: the tile itself, its corners see-through.
  ...[300, 150, 71].map((s) => ({ name: `kabuk-store-tile-${s}.png`, w: s, h: s, svg: "kabuk-app-icon-dark.svg", share: 1, ground: null })),
  // 1:1 box art: the mark on the tile's ground, edge to edge.
  ...[1080, 2160].map((s) => ({ name: `kabuk-store-box-${s}.png`, w: s, h: s, svg: "kabuk-mark-on-dark.svg", share: BOX_MARK, ground: GROUND })),
  // 9:16 poster art: the vertical lockup (mark and name) on the tile's ground.
  ...[[720, 1080], [1440, 2160]].map(([w, h]) => ({ name: `kabuk-store-poster-${w}x${h}.png`, w, h, svg: "kabuk-lockup-vertical-on-dark.svg", share: POSTER_LOCKUP, ground: GROUND })),
  // 16:9 hero art: the mark alone on the tile's ground (the Store wants no name on it).
  ...[[1920, 1080], [3840, 2160]].map(([w, h]) => ({ name: `kabuk-store-hero-${w}x${h}.png`, w, h, svg: "kabuk-mark-on-dark.svg", share: HERO_MARK, ground: GROUND })),
];

function page(img) {
  const src = pathToFileURL(path.join(LOGO, "svg", img.svg)).href;
  const ground = img.ground || "transparent";
  return `<!doctype html><html><body style="margin:0;width:${img.w}px;height:${img.h}px;background:${ground};display:flex;align-items:center;justify-content:center">
<img src="${src}" style="width:${img.w * img.share}px;height:auto;display:block"></body></html>`;
}

async function main() {
  fs.mkdirSync(OUT, { recursive: true });
  const browser = await chromium.launch();
  for (const img of IMAGES) {
    const ctx = await browser.newContext({ viewport: { width: img.w, height: img.h }, deviceScaleFactor: 1 });
    const tab = await ctx.newPage();
    const html = path.join(OUT, ".page.html");
    fs.writeFileSync(html, page(img));
    await tab.goto(pathToFileURL(html).href);
    await tab.waitForLoadState("load");
    await tab.screenshot({ path: path.join(OUT, img.name), omitBackground: !img.ground });
    fs.unlinkSync(html);
    await ctx.close();
    console.log(`${img.name}  ${img.w}x${img.h}`);
  }
  await browser.close();
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
