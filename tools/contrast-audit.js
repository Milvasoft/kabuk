// Contrast audit in real Chromium: every visible text element in every mock screen vs the real background behind it.
// All 7 looks x 2 modes x 8 accents (theme + 7 swatches) x every settings page, launcher open.
let chromium;
try { ({ chromium } = require("playwright")); } catch { ({ chromium } = require("/opt/npm-tools/node_modules/playwright")); }
const file = require("path").resolve(process.argv[2] || "docs/design/kabuk-tasarim.html");

function inPage(opts) {
  const parse = (s) => {
    s = String(s || "").trim();
    let m = s.match(/^rgba?\(([^)]+)\)$/);
    if (m) { const p = m[1].split(/[\s,/]+/).filter(Boolean).map(parseFloat); return { r: p[0], g: p[1], b: p[2], a: p.length > 3 ? p[3] : 1 }; }
    m = s.match(/^color\(srgb ([^)]+)\)$/);
    if (m) { const p = m[1].split(/[\s/]+/).filter(Boolean).map(parseFloat); return { r: p[0] * 255, g: p[1] * 255, b: p[2] * 255, a: p.length > 3 ? p[3] : 1 }; }
    return null;
  };
  const over = (t, u) => ({ r: t.r * t.a + u.r * (1 - t.a), g: t.g * t.a + u.g * (1 - t.a), b: t.b * t.a + u.b * (1 - t.a), a: 1 });
  const ch = (v) => { v /= 255; return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; };
  const lum = (c) => 0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b);
  const ratio = (x, y) => { const a = lum(x), b = lum(y); return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05); };
  const hex = (c) => "#" + [c.r, c.g, c.b].map((v) => Math.round(v).toString(16).padStart(2, "0")).join("");
  const click = (sel) => { const el = document.querySelector(sel); if (!el) throw new Error("missing " + sel); el.click(); };

  function bgOf(el) {
    const layers = [];
    for (let n = el; n && n.nodeType === 1; n = n.parentElement) {
      const cs = getComputedStyle(n);
      if (cs.backgroundImage && cs.backgroundImage !== "none" && !/^url/.test(cs.backgroundImage)) return null; // gradient behind text: can't measure
      const c = parse(cs.backgroundColor);
      if (c && c.a > 0) { layers.push(c); if (c.a >= 1) break; }
    }
    let c = { r: 0, g: 0, b: 0, a: 1 };
    for (let i = layers.length - 1; i >= 0; i--) c = over(layers[i], c);
    return c;
  }
  function opacityOf(el, root) {
    let op = 1;
    for (let n = el; n && n !== root.parentElement; n = n.parentElement) {
      const cs = getComputedStyle(n);
      if (cs.display === "none" || cs.visibility === "hidden") return 0;
      op *= parseFloat(cs.opacity);
    }
    return op;
  }
  const SKIP = ".tp, .sp, svg, [aria-hidden='true']";
  function audit(root, ctx, out) {
    for (const el of [root, ...root.querySelectorAll("*")]) {
      if (el.closest(SKIP)) continue;
      const own = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
      const field = (el.tagName === "INPUT" && el.type !== "range" && el.value) || el.tagName === "SELECT";
      if (!own && !field) continue;
      if (!el.getClientRects().length || parseFloat(getComputedStyle(el).fontSize) < 1) continue;
      const op = opacityOf(el, root);
      if (op === 0) continue;
      if (op < 0.9) { out.dimmed++; continue; }
      const cs = getComputedStyle(el);
      const fg0 = parse(cs.color), bg = bgOf(el);
      if (!fg0 || !bg) { out.unknown++; continue; }
      const fg = fg0.a < 1 ? over(fg0, bg) : fg0;
      const r = ratio(fg, bg);
      const size = parseFloat(cs.fontSize), bold = parseInt(cs.fontWeight, 10) >= 600;
      const need = size >= 24 || (bold && size >= 18.66) ? 3 : 4.5;
      out.checked++;
      if (r < need) {
        const text = (field ? el.value || el.tagName : [...el.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent).join("")).trim().slice(0, 26);
        const cls = (e) => e.tagName.toLowerCase() + (typeof e.className === "string" && e.className.trim() ? "." + e.className.trim().split(/\s+/).join(".") : "");
        out.fails.push({ ctx, sel: cls(el) + (el.parentElement ? " < " + cls(el.parentElement) : ""), text, fg: hex(fg), bg: hex(bg), r: +r.toFixed(2), need });
      }
    }
  }

  const out = { checked: 0, dimmed: 0, unknown: 0, fails: [] };
  const pages = [...document.querySelectorAll("#set-stage [data-page]")].map((b) => b.dataset.page);
  for (const mode of opts.modes) {
    click(`[data-pagemode="${mode}"]`);
    for (const look of opts.looks) {
      click(`#dir-${look}`);
      for (const acc of opts.accents) {
        click('#set-stage [data-page="look"]');
        click(`#set-stage [data-accent="${acc}"]`);
        if (!document.querySelector("#stage .launcher:not([hidden])")) click("#btn-launch");
        const ctx = `${mode}/${look}/${acc || "theme"}`;
        for (const sc of document.querySelectorAll("#stage .screen, #mon-1 .screen, #mon-2 .screen, .comp .screen")) audit(sc, ctx + " " + sc.closest("[id]").id, out);
        for (const p of acc === "" ? pages : ["look", "bar", "oled"]) {
          click(`#set-stage [data-page="${p}"]`);
          const c = document.querySelector('#set-stage [data-dim="custom"]'); if (c) c.click();
          audit(document.querySelector("#set-stage .screen"), ctx + " settings:" + p, out);
        }
        click("#btn-launch");
      }
    }
  }
  // wallpapers on: every bar must still read (see-through bars get a desk plate)
  for (const mode of opts.modes) {
    click(`[data-pagemode="${mode}"]`);
    for (const look of opts.looks) {
      click(`#dir-${look}`);
      click('#set-stage [data-page="wall"]');
      for (const wall of ["milva", "stars", "gradient"]) {
        click(`#set-stage [data-wall="${wall}"]`);
        for (const sc of document.querySelectorAll("#stage .screen, #mon-2 .screen")) audit(sc.querySelector(".bar"), `${mode}/${look}/wall:${wall} ${sc.closest("[id]").id}`, out);
      }
      click('#set-stage [data-wall="black"]');
    }
  }
  return out;
}

(async () => {
  const b = await chromium.launch(require("fs").existsSync("/opt/pw-browsers/chromium") ? { executablePath: "/opt/pw-browsers/chromium" } : {});
  const p = await b.newPage({ viewport: { width: 1400, height: 900 } });
  const errs = [];
  p.on("pageerror", (e) => errs.push(e.message));
  await p.goto(require("url").pathToFileURL(file).href);
  await p.evaluate(() => localStorage.clear());
  await p.reload();
  await p.waitForTimeout(400);
  await p.addStyleTag({ content: "*, *::before, *::after { transition: none !important; animation-duration: 0s !important; }" });
  const env = process.env;
  const out = await p.evaluate(inPage, {
    modes: (env.MODES || "dark,light").split(","),
    looks: (env.LOOKS || "abcdefg").split(""),
    accents: (env.ACC ?? ",#8fd3ff,#fbc419,#7fe0b5,#ff8f6b,#b9a7ff,#ff7eb6,#ecebe7").split(","),
  });
  const groups = new Map();
  for (const f of out.fails) { const k = `${f.sel} | ${f.fg} on ${f.bg}`; if (!groups.has(k)) groups.set(k, { f, ctx: [], min: 99 }); const g = groups.get(k); g.ctx.push(f.ctx); g.min = Math.min(g.min, f.r); }
  console.log(`checked ${out.checked} text elements: ${out.fails.length} below WCAG AA in ${groups.size} distinct cases (${out.dimmed} disabled/dimmed skipped, ${out.unknown} on gradients)`);
  [...groups.values()].sort((a, b) => a.min - b.min).slice(0, Number(env.LIM || 60)).forEach((g) => console.log(`${String(g.ctx.length).padStart(4)}x ${g.min.toFixed(2)}<${g.f.need}  "${g.f.text}"  ${g.f.sel}  ${g.f.fg} on ${g.f.bg}\n        ${[...new Set(g.ctx)].slice(0, 3).join(" ; ")}`));
  console.log(errs.length ? "PAGE ERRORS: " + errs.join(" | ") : "no page errors");
  await b.close();
})();
