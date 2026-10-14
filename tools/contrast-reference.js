// Reference implementation of Kabuk's contrast engine, extracted verbatim from docs/design/kabuk-tasarim.html.
// The Rust port in crates/kabuk-theme must produce byte-identical hex output for tests/fixtures/contrast.json.
// Notes for the port:
//  - HSL conversion, the 22-step binary search, the +0.12 headroom and the +0.02 "already passes" margin are part of the contract.
//  - toHex rounds each channel with Math.round (half away from zero for positives) after clamping to 0..255.
//  - surfacesOf() composites semi-transparent tokens over the desk (or over surf for surf-sel); desk itself over black.
//  - `state.accent` is the user-picked accent or null; `cs.getPropertyValue(name)` reads a theme token.
  /* ---------- contrast ----------
     A chosen accent is never applied as-is. For each screen it is measured against the surfaces it sits on
     (desk, panels, selected rows, windows) and nudged lighter or darker until text in it reaches 4.5:1.
     The filled variant (selected row, active workspace, buttons) is fitted against the text drawn on it.
     The native shell runs the same check when a theme or accent loads. */
  const MIN_TEXT = 4.5;
  function parseColor(str) {
    const s = String(str || "").trim();
    let m = s.match(/^#([0-9a-f]{3,8})$/i);
    if (m) {
      let h = m[1];
      if (h.length <= 4) h = [...h].map((c) => c + c).join("");
      return { r: parseInt(h.slice(0, 2), 16), g: parseInt(h.slice(2, 4), 16), b: parseInt(h.slice(4, 6), 16), a: h.length === 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1 };
    }
    m = s.match(/^rgba?\(([^)]+)\)$/i);
    if (m) {
      const p = m[1].split(/[\s,/]+/).filter(Boolean).map(parseFloat);
      return { r: p[0], g: p[1], b: p[2], a: p.length > 3 ? p[3] : 1 };
    }
    return null;
  }
  const over = (top, under) => ({ r: top.r * top.a + under.r * (1 - top.a), g: top.g * top.a + under.g * (1 - top.a), b: top.b * top.a + under.b * (1 - top.a), a: 1 });
  const chan = (v) => { v /= 255; return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; };
  const lum = (c) => 0.2126 * chan(c.r) + 0.7152 * chan(c.g) + 0.0722 * chan(c.b);
  const contrast = (x, y) => { const a = lum(x), b = lum(y); return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05); };
  const toHex = (c) => "#" + [c.r, c.g, c.b].map((v) => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, "0")).join("");
  function toHsl({ r, g, b }) {
    r /= 255; g /= 255; b /= 255;
    const max = Math.max(r, g, b), min = Math.min(r, g, b), l = (max + min) / 2, d = max - min;
    if (!d) return { h: 0, s: 0, l };
    const s = d / (1 - Math.abs(2 * l - 1));
    const h = max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
    return { h: h * 60, s, l };
  }
  function fromHsl({ h, s, l }) {
    const c = (1 - Math.abs(2 * l - 1)) * s, x = c * (1 - Math.abs(((h / 60) % 2) - 1)), m = l - c / 2;
    const [r, g, b] = h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x];
    return { r: (r + m) * 255, g: (g + m) * 255, b: (b + m) * 255, a: 1 };
  }
  /* Smallest lightness change (hue and saturation kept) that gives at least `min` against every background. */
  function fitContrast(color, bgs, min) {
    const worst = (c) => Math.min(...bgs.map((b) => contrast(c, b)));
    if (worst(color) >= min + 0.02) return color;
    min += 0.12; // headroom so rounding to a hex value never lands just under the line
    const darken = bgs.reduce((t, b) => t + lum(b), 0) / bgs.length > 0.18;
    const hsl = toHsl(color);
    let lo = darken ? 0 : hsl.l, hi = darken ? hsl.l : 1;
    for (let i = 0; i < 22; i++) {
      const mid = (lo + hi) / 2;
      const pass = worst(fromHsl({ ...hsl, l: mid })) >= min;
      if (darken) { if (pass) lo = mid; else hi = mid; } else if (pass) hi = mid; else lo = mid;
    }
    const fitted = fromHsl({ ...hsl, l: darken ? lo : hi });
    return worst(fitted) >= min ? fitted : (darken ? { r: 0, g: 0, b: 0, a: 1 } : { r: 255, g: 255, b: 255, a: 1 });
  }
  /* Opaque versions of the surfaces accent text can sit on, read from the screen's own tokens. */
  function surfacesOf(cs) {
    const tok = (n) => parseColor(cs.getPropertyValue(n));
    const desk = over(tok("--desk") || { r: 0, g: 0, b: 0, a: 1 }, { r: 0, g: 0, b: 0, a: 1 });
    const solid = (n, under) => { const c = tok(n); return c ? over(c, under) : under; };
    const surf = solid("--surf", desk);
    return { desk, surf, sel: solid("--surf-sel", surf), win: solid("--win-bg", desk), surfAlt: solid("--surf-alt", desk) };
  }
  /* Text roles and the backgrounds each must read on. Themes set these freely; this pass guarantees the minimum. */
  const TEXT_ROLES = [
    ["--fg-s", "ui"], ["--dim", "ui"],
    ["--app-fg", "app"], ["--app-strong", "app"], ["--app-dim", "chip"],
    ["--sx-k", "app"], ["--sx-ty", "app"], ["--sx-s", "app"], ["--sx-c", "app"], ["--sx-n", "app"], ["--sx-fn", "app"],
    ["--t-p", "app"], ["--t-ok", "app"], ["--t-d", "app"],
    ["--ok-fg", "--ok-bg"], ["--run-fg", "--run-bg"], ["--err-fg", "--err-bg"],
    ["--accent2", "ui"],
  ];
  const FITTED = ["--accent-s", "--accent-fill", "--on-accent", "--badge-fill", "--on-badge", ...TEXT_ROLES.map((r) => r[0])];
  const BLACK = { r: 0, g: 0, b: 0, a: 1 }, WHITE = { r: 255, g: 255, b: 255, a: 1 };
  /* A fill and the text drawn on it: keep the preferred text colour when it reads, else the other of black/white;
     only if neither reads is the fill itself shifted. */
  function fillPair(base, pref) {
    const other = lum(pref) > 0.5 ? BLACK : WHITE;
    const on = contrast(base, pref) >= MIN_TEXT ? pref : contrast(base, other) >= MIN_TEXT ? other : pref;
    return { fill: fitContrast(base, [on], MIN_TEXT), on };
  }
  function adaptAccent(sc) {
    FITTED.forEach((p) => sc.style.removeProperty(p));
    const cs = getComputedStyle(sc);
    const tok = (n) => parseColor(cs.getPropertyValue(n));
    const base = parseColor(state.accent || cs.getPropertyValue("--accent-s"));
    if (!base) return;
    const s = surfacesOf(cs);
    const ui = [s.desk, s.surf, s.sel, s.win, s.surfAlt];
    const chip = tok("--app-chip") ? over(tok("--app-chip"), s.win) : s.win;
    const groups = { ui, app: [s.win], chip: [s.win, chip] };
    const set = {};
    const text = fitContrast(base, ui, MIN_TEXT);
    const pref = tok("--on-accent") || BLACK;
    const acc = fillPair(base, pref);
    set["--accent-s"] = text; set["--accent-fill"] = acc.fill; set["--on-accent"] = acc.on;
    const badge = tok("--badge");
    if (badge) { const b = fillPair(badge, tok("--on-badge-pref") || pref); set["--badge-fill"] = b.fill; set["--on-badge"] = b.on; }
    for (const [role, against] of TEXT_ROLES) {
      const c = tok(role);
      if (!c) continue;
      const bgs = groups[against] || [tok(against) ? over(tok(against), s.win) : s.win];
      const fitted = fitContrast(over(c, bgs[0]), bgs, MIN_TEXT);
      if (toHex(fitted) !== toHex(over(c, bgs[0]))) set[role] = fitted;
    }
    Object.entries(set).forEach(([k, v]) => sc.style.setProperty(k, toHex(v)));
    sc.dataset.accentAdapted = toHex(text) !== toHex(base) || toHex(acc.fill) !== toHex(base) ? "1" : "";
  }
