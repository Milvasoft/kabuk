// The settings screen: the design page's settings panel (docs/design/kabuk-tasarim.html,
// `settingsPanel`) with its controls bound to kabuk.toml through the Rust bridge
// (apps/kabuk-settings/src/bridge.rs). Markup and class names follow the design page so
// its stylesheet (design.css) styles it; its drawings come from design-art.js. Every word
// comes from the language files, which the bridge sends with the state.
"use strict";
(function () {
  const webview = window.chrome.webview;
  const post = (m) => webview.postMessage(JSON.stringify(m));

  /* ---------- state ---------- */
  let S = null; // the last state the bridge sent
  let A = null; // the design's drawings
  const ui = {
    page: "look",
    wallMon: "all",
    wallTab: "pick",
    dimCustom: null, // null: follow the value; true/false: chosen
    ruleForm: null, // the rule being added (index null) or edited, as its fields read
    rulePicking: false, // the open windows listed to fill the rule from
    ruleWindows: null, // null: asked, not answered; else [{ exe, class, title }]
    capture: null, // { path, index, mods } while a shortcut is being pressed
    refused: null,
    pending: null, // what a picked file or folder is for
    // The Create tab: the key being typed, Ollama's answer, the request and the run.
    keyEntry: false,
    ollama: null, // null: not asked; { ok, model }
    prompt: "",
    gen: { phase: "idle", step: 0, attempt: 1, ms: null, average: null, reason: "", name: "", preview: null },
    update: null, // null: not asked; { state, version, page } from the bridge
    previews: {}, // the gallery's previews by design key (DECISIONS D58), as they come
    previewsAsked: false,
    previewsDone: false, // every preview sent: a card without one stops waiting for it
    fileMsg: null, // what the settings file's export, import or reset said
    confirmReset: false, // "Reset all" asks before it does
    search: "", // what is typed in the menu's search field (PLAN M19 U20)
    found: null, // the setting a search led to, { page, n }, marked until the next click
  };

  // The design page's keys are camelCase; the language files use kebab-case
  // (tools/extract-locales.js), so both spellings work here.
  const kebab = (k) => String(k).replace(/^#/, "").replace(/([a-z0-9])([A-Z])/g, "$1-$2").toLowerCase().replace(/[^a-z0-9-]/g, "-");
  const t = (k) => (S && S.strings[kebab(k)]) ?? kebab(k);
  const tv = (k, v) => t(k).split(S.mark).join(String(v));
  const escapeHtml = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  const icon = (id, cls = "i") => `<svg class="${cls}" aria-hidden="true"><use href="#i-${id}"></use></svg>`;

  // The design's looks Kabuk has (not Piksel, which the user did not want).
  const LOOK_NAMES = { a: "ada", b: "cizgi", c: "devre", d: "ray", e: "konsol", f: "rihtim", g: "etkinlik", h: "pano", i: "izgara", j: "defter" };
  const DIM_MIN = 10, DIM_MAX = 3600, DIM_PRESETS = [30, 60, 120];
  const SAVER_WAITS = [3, 5, 10, 15];
  const RANGES = { height: [24, 72], tray: [0, 12], gap: [0, 32], bright: [10, 100, 5], dropdown: [20, 90, 5], workspaces: [1, 9], drift: [1, 120], minWidth: [1, 16384] };

  const cfg = () => S.config;
  const at = (root, path) => path.split(".").reduce((o, k) => (o == null ? undefined : o[k]), root);
  const get = (path) => at(cfg(), path);
  /** Whether a setting differs from Kabuk's default (asked for by the user: a way back to it). */
  const changed = (path) => JSON.stringify(get(path) ?? null) !== JSON.stringify(at(S.defaults, path) ?? null);
  const set = (path, value) => post({ op: "set", path, value });
  const code = () => S.look;

  /* ---------- building blocks (the design page's helpers) ---------- */
  const head = (title, desc) => `<div><h3>${title}</h3><p>${desc}</p></div>`;
  const row = (label, hint, control, extra = "") => `<div class="set-row${extra}"><span><b>${label}</b><span class="h">${hint}</span></span>${control}</div>`;
  const tgl = (path, label, hint) => row(label, hint + resetIfChanged(path) + refusalFor(path),`<button class="tgl" type="button" role="switch" aria-checked="${!!get(path)}" aria-label="${escapeHtml(label)}" data-tgl="${path}"></button>`);
  const seg = (aria, buttons) => `<span class="seg" role="group" aria-label="${escapeHtml(aria)}">${buttons}</span>`;
  const choice = (path, now, options) => options.map(([v, n]) => {
    const on = now === v;
    return `<button type="button" data-set="${path}" data-val='${escapeHtml(JSON.stringify(v))}' class="${on ? "on" : ""}" aria-pressed="${on}">${n}</button>`;
  }).join("");
  const segFor = (path, aria, options) => seg(aria, choice(path, get(path), options));
  // A count on a slider: the number alone beside it.
  const countRange = (id, path, [min, max], label) => `<span class="rng"><input type="range" id="${id}" data-range="${path}" data-unit="" min="${min}" max="${max}" value="${get(path)}" aria-label="${escapeHtml(label)}"><span class="v" data-v="${id}">${get(path)}</span></span>`;
  const range = (id, path, [min, max, step = 1], value, label) => `<span class="rng"><input type="range" id="${id}" data-range="${path}" min="${min}" max="${max}" step="${step}" value="${value}" aria-label="${escapeHtml(label)}"><span class="v" data-v="${id}">${tv("setPx", value)}</span></span>`;
  const resetLink = (path) => ` · <button type="button" class="linkish" data-reset="${path}">${t("posReset")}</button>`;
  const resetIfChanged = (path) => (changed(path) ? resetLink(path) : "");
  /* Why a change of `path` was refused, under its row (PLAN M19 U2: the page said only that
     it was refused, at its top). Noted so the top says it when no row of the page has it. */
  function refusalFor(path) {
    if (!ui.refused || ui.refused.path !== path) return "";
    ui.refusalShown = true;
    return `<span class="h" role="alert" data-refusal style="display:block;color:var(--err-fg)">${escapeHtml(refusalText())}</span>`;
  }
  /* Why the last change was refused; a rule of the user's in the way is named (fewer
     workspaces than a rule sends windows to). */
  function refusalText() {
    const r = ui.refused;
    const why = r.reason || t("setRefused");
    const where = /^rules\.rule\[(\d+)\]/.exec(r.at || "");
    const rule = where && r.path !== "rules.rule" ? (cfg().rules.rule || [])[Number(where[1])] : null;
    return rule ? `${tv("setRefusedByRule", ruleName(rule))}: ${why}` : why;
  }
  // A row for setting `path`: its "Use default" sits at the end of its hint, as a switch's
  // does, never between the label and the control (found by the user).
  const rowFor = (path, label, hint, control) =>
    row(label, hint + (changed(path) ? (hint ? resetLink(path) : resetLink(path).replace(" · ", "")) : "") + refusalFor(path), control);

  /* ---------- shortcuts ---------- */
  const MODS = [["ctrlKey", "Ctrl"], ["altKey", "Alt"], ["shiftKey", "Shift"], ["metaKey", "Win"]];
  const NAMED = { Space: "Space", Enter: "Enter", Tab: "Tab", Escape: "Esc", Backspace: "Backspace", Delete: "Delete", Insert: "Insert", Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown", ArrowLeft: "Left", ArrowRight: "Right", ArrowUp: "Up", ArrowDown: "Down" };
  function keyName(e) {
    if (/^Key[A-Z]$/.test(e.code)) return e.code.slice(3);
    if (/^Digit[0-9]$/.test(e.code)) return e.code.slice(5);
    if (/^F([1-9]|1[0-9]|2[0-4])$/.test(e.code)) return e.code;
    return NAMED[e.code] || null;
  }
  const modsOf = (e) => MODS.filter(([k]) => e[k]).map(([, n]) => n);
  /* Windows' own Win chords (as Kabuk writes them): one chosen loses its Windows meaning. */
  const WINDOWS_CHORDS = ["Win+A", "Win+B", "Win+D", "Win+E", "Win+G", "Win+H", "Win+I", "Win+K", "Win+M", "Win+N", "Win+P", "Win+Q", "Win+R", "Win+S", "Win+T", "Win+U", "Win+V", "Win+W", "Win+X", "Win+Z", "Win+Tab", "Win+Space", "Win+Shift+S", "Win+Ctrl+D", "Win+Ctrl+Left", "Win+Ctrl+Right", "Win+Up", "Win+Down", "Win+Left", "Win+Right", "Win+Home", "Win+Enter"];
  const kbds = (chord) => chord.split("+").map((x) => `<kbd>${escapeHtml(x)}</kbd>`).join(" ");
  function chordEditor(path) {
    const value = get(path);
    const list = Array.isArray(value) ? value : [value];
    const capturing = (i) => ui.capture && ui.capture.path === path && ui.capture.index === i;
    const chips = list.map((c, i) => capturing(i)
      ? `<kbd>${Array.isArray(value) ? t("setKeyPress") : t("setKeyMods")}</kbd>`
      : `<button type="button" class="linkish" data-chord="${path}" data-index="${i}">${kbds(c)}</button>`);
    if (Array.isArray(value)) {
      chips.push(capturing(list.length) ? `<kbd>${t("setKeyPress")}</kbd>` : `<button type="button" class="linkish" data-chord="${path}" data-index="${list.length}">${t("setAdd")}</button>`);
      if (list.length > 1) chips.push(`<button type="button" class="linkish" data-chord-drop="${path}">${t("setRemove")}</button>`);
    }
    if (changed(path)) chips.push(`<button type="button" class="linkish" data-reset="${path}">${t("posReset")}</button>`);
    // A chord Windows has too: it does Kabuk's action while Kabuk runs (SPEC §6; PLAN M19 U3).
    // Only for a chord the user chose: Kabuk's own defaults are SPEC §6's choice.
    const defaults = [].concat(at(S.defaults, path) ?? []);
    const windows = list.filter((c) => WINDOWS_CHORDS.includes(c) && !defaults.includes(c)).map((c) => `<span class="h" style="display:block">${escapeHtml(tv("setKeyWindowsHint", c))}</span>`);
    return `<span class="keys" style="display:flex;gap:8px;flex-wrap:wrap;justify-content:flex-end">${chips.join(" ")}${windows.join("")}${refusalFor(path)}</span>`;
  }
  function onKey(e) {
    const c = ui.capture;
    if (!c) return;
    e.preventDefault();
    e.stopPropagation();
    const mods = modsOf(e);
    if (e.type === "keydown" && e.code === "Escape" && mods.length === 0) { ui.capture = null; capture(false); render(); return; }
    if (c.mods) {
      // Modifier-only settings: what is held when the last key is released.
      if (e.type === "keydown") { c.held = mods.length >= (c.held || []).length ? mods : c.held; return; }
      if (mods.length === 0 && c.held && c.held.length) { save(c.path, c.held.join("+")); }
      return;
    }
    if (e.type !== "keydown") return;
    const key = keyName(e);
    if (!key) return;
    const chord = [...mods, key].join("+");
    const list = [...get(c.path)];
    list[c.index] = chord;
    save(c.path, list);
  }
  function save(path, value) { ui.capture = null; capture(false); set(path, value); }
  /* While the recorder waits for a chord, Rust takes the keys (Win chords never reach the
     page: Kabuk and Windows take them first) and sends each chord as {"op":"chord"}. */
  let capturingKeys = false;
  function capture(on) {
    if (on === capturingKeys) return;
    capturingKeys = on;
    post({ op: "capture-keys", on });
  }
  function onChord(chord) {
    const c = ui.capture;
    if (!c || c.mods) return;
    if (chord === "Esc") { ui.capture = null; capture(false); render(); return; }
    const list = [...get(c.path)];
    list[c.index] = chord;
    save(c.path, list);
  }
  document.addEventListener("keydown", onKey, true);
  // In the search field Enter opens the first setting found and Esc empties it.
  document.addEventListener("keydown", (e) => {
    if (e.target.id !== "set-search") return;
    if (e.key === "Escape" && ui.search) { e.preventDefault(); ui.search = ""; render(); }
    if (e.key === "Enter" && ui.search.trim()) { e.preventDefault(); const hit = searchSettings(ui.search)[0]; if (hit) goToFound(hit); }
  });
  // Away from the window, the keys are the other programs' again.
  window.addEventListener("blur", () => { if (ui.capture) { ui.capture = null; capture(false); render(); } });
  document.addEventListener("keyup", onKey, true);

  /* ---------- pages ---------- */
  const navItems = [["look", "app"], ["wall", "image"], ["saver", "lock"], ["bar", "tile"], ["windows", "tile"], ["rules", "file"], ["game", "pad"], ["launcher", "search"], ["keys", "term"], ["lang", "globe"], ["oled", "shield"], ["about", "cube"]];

  function lookPage() {
    const c = code();
    const accent = cfg().appearance.accent;
    const own = accent === "theme";
    const accentNow = own ? A.lookDefaults[c] : accent;
    const cards = A.looks.filter((d) => LOOK_NAMES[d]).map((d) => {
      const on = d === c;
      return `<button class="theme-card${on ? " on" : ""}" type="button" data-set="appearance.look" data-val='"${LOOK_NAMES[d]}"' aria-pressed="${on}"><span class="tp">${A.miniLook[d].replaceAll("VAR", on ? accentNow : A.lookDefaults[d])}</span><span class="tn">${t("looks-" + d)}<span>${on ? t("inUse") : t("apply")}</span></span></button>`;
    }).join("");
    const swatches = `<button class="swatch def${own ? " on" : ""}" type="button" data-set="appearance.accent" data-val='"theme"' aria-label="${t("themeAccent")}" title="${t("themeAccent")}"></button>` + A.accentList.map((x) => `<button class="swatch${accent === x ? " on" : ""}" type="button" data-set="appearance.accent" data-val='"${x}"' style="background:${x}" aria-label="${t("accents-" + x.slice(1))}" title="${t("accents-" + x.slice(1))}"></button>`).join("");
    const adapted = S.accent_adapted ? `<span class="sw-adapt">${tv("accentAdapted", S.accent_adapted)}</span>` : "";
    return head(t("navLook"), t("lookDesc")) + `
      <div class="set-grp"><span class="gl">${t("theme")}</span><div class="themes">${cards}</div></div>
      <div class="set-grp"><span class="gl">${t("accent")}</span><div class="swatches">${swatches}<span class="sw-label">${own ? t("themeAccent") : t("accents-" + accent.slice(1))}</span></div>${adapted}</div>
      <div class="set-grp">${rowFor("appearance.mode", t("modeLbl"), modeHint(), segFor("appearance.mode", t("modeLbl"), [["dark", t("dark")], ["light", t("light")], ["system", t("modeSystem")], ["time", t("modeSun")]]))}
      ${rowFor("appearance.text_size", t("textSize"), "", segFor("appearance.text_size", t("textSize"), [["small", t("small")], ["normal", t("normal")], ["large", t("large")]]))}
      ${rowFor("appearance.animations", t("setAnimations"), t("setAnimationsH"), segFor("appearance.animations", t("setAnimations"), [["full", t("setAnimationsFull")], ["reduced", t("setAnimationsReduced")], ["off", t("setAnimationsOff")]]))}
      ${S.store ? row(t("setCursors"), t("setStoreMissing"), "") : rowFor("appearance.cursors", t("setCursors"), t("setCursorsH"), segFor("appearance.cursors", t("setCursors"), [["windows", t("setCursorsWindows")], ["kabuk", t("setCursorsKabuk")], ["custom", t("setCursorsCustom")]]))}
      ${S.store || cfg().appearance.cursors === "windows" ? "" : rowFor("appearance.cursor_size", t("setCursorSize"), t("setCursorSizeH"), segFor("appearance.cursor_size", t("setCursorSize"), [["normal", t("cursorSizeNormal")], ["large", t("cursorSizeLarge")], ["huge", t("cursorSizeHuge")]]))}</div>
      ${!S.store && cfg().appearance.cursors === "custom" ? cursorPicker() : ""}
      <div class="set-grp"><span class="gl">${t("setSysTheme")}</span>
      ${S.store ? row(t("setSysColors"), t("setStoreMissing"), "") : tgl("system_theme.colors", t("setSysColors"), cfg().appearance.mode === "system" ? t("setSysColorsFollow") : t("setSysColorsH"))}</div>
      ${S.mode === "light" ? `<div class="note">${t("lightNote")}</div>` : ""}
      ${themeFileGroup()}`;
  }

  // Theme files (DECISIONS D64): the look's settings out to a file to share, and in.
  function themeFileGroup() {
    return `<div class="set-grp"><span class="gl">${t("th")}. ${t("thH")}</span>
      <div style="display:flex;gap:8px"><button class="set-btn ghost" type="button" data-theme-file="export">${t("thExport")}</button><button class="set-btn ghost" type="button" data-theme-file="import">${t("thImport")}</button></div>
      ${ui.themeMsg ? `<div class="note" role="status">${escapeHtml(ui.themeMsg)}</div>` : ""}</div>`;
  }

  /* Cursors picked apart from the look (DECISIONS D41): a style and a colour, each card
     previewed by the shell's own drawings (`state.cursor_previews`). */
  const CURSOR_STYLES = ["round", "slim", "circuit", "bold", "block", "cartoon", "comic"];
  const CURSOR_COLORS = ["light", "dark", "accent"];
  function cursorCards(path, now, list, previews, name) {
    return list.map((v) => {
      const on = v === now;
      const pics = ((previews || {})[v] || []).map((svg) => `<span class="cp-i">${svg}</span>`).join("");
      return `<button class="theme-card cursor-card${on ? " on" : ""}" type="button" data-set="${path}" data-val='"${v}"' aria-pressed="${on}"><span class="cp">${pics}</span><span class="tn">${t(name + v)}</span></button>`;
    }).join("");
  }
  function cursorPicker() {
    const a = cfg().appearance;
    const p = S.cursor_previews || {};
    return `<div class="set-grp"><span class="gl">${t("setCursorStyle")}</span><div class="cursor-cards">${cursorCards("appearance.cursor_style", a.cursor_style, CURSOR_STYLES, p.styles, "cursorStyle-")}</div></div>
      <div class="set-grp"><span class="gl">${t("setCursorColor")}</span><div class="cursor-cards">${cursorCards("appearance.cursor_color", a.cursor_color, CURSOR_COLORS, p.colors, "cursorColor-")}</div></div>`;
  }


  /* Wallpaper: one choice for every monitor, or one per monitor (`[[wallpaper.monitor]]`). */
  const WALLS = [["windows", "wWindows", "wWindowsH"], ["circuit", "wMilva", "wMilvaH"], ["constellation", "wStars", "wStarsH"], ["black", "wBlack", "wBlackH"], ["gradient", "wGrad", "wGradH"], ["image", "wImage", "wImageH"]];
  const monitorEntry = (n) => (cfg().wallpaper.monitor || []).find((m) => m.index === n);
  /** The created wallpaper chosen where the choice applies (`null` when another kind). */
  function wallGenNow() {
    const w = cfg().wallpaper;
    if (ui.wallMon === "all") return (w.monitor || []).length ? null : w.generated;
    const e = monitorEntry(Number(ui.wallMon));
    return e ? e.generated : w.generated;
  }
  /** The gallery design chosen where the choice applies (`null` when another kind). */
  function wallGalNow() {
    const w = cfg().wallpaper;
    if (ui.wallMon === "all") return (w.monitor || []).length ? null : w.gallery;
    const e = monitorEntry(Number(ui.wallMon));
    return e ? e.gallery : w.gallery;
  }
  // The gallery's cards (DECISIONS D58): each design's preview as the bridge draws it, a
  // placeholder in its place until it comes (PLAN M19 U20: an empty black area before).
  function galleryCards(designs, isOn, attrs, hint) {
    return designs.map((d) => {
      const on = isOn(d.key);
      const url = ui.previews[d.key];
      const waiting = !url && !ui.previewsDone;
      const img = `<img data-gprev="${escapeHtml(d.key)}" ${url ? `src="${url}"` : ""} alt="" style="width:100%;height:100%;object-fit:cover;display:block">`;
      return `<button class="saver${on ? " on" : ""}" type="button" ${attrs(d)} aria-pressed="${on}"${waiting ? ` aria-busy="true"` : ""}><span class="sp${waiting ? " skel" : ""}">${img}</span><span class="sn">${t(d.name)}<span>${hint(d)}</span></span></button>`;
    }).join("");
  }
  /* A preview came (or none will): its card's placeholder goes, without drawing the page. */
  function previewCame(img) {
    img.closest(".sp")?.classList.remove("skel");
    img.closest(".saver")?.removeAttribute("aria-busy");
  }
  function askPreviews() {
    if (ui.previewsAsked) return;
    ui.previewsAsked = true;
    post({ op: "gallery-previews" });
  }
  function wallNow() {
    const w = cfg().wallpaper;
    if (ui.wallMon === "all") return (w.monitor || []).length ? null : w.kind;
    const e = monitorEntry(Number(ui.wallMon));
    return e ? e.kind : w.kind;
  }
  function wallArt(kind) {
    if (kind === "black") return "";
    // The user's own picture is not drawn here; the card shows the picture icon.
    if (kind === "windows") return `<div class="wall">${A.wallSvg("image", 400, 225, 5)}</div>`;
    if (kind === "gradient") return `<div class="wall gradient"></div>`;
    const scene = { circuit: ["milva", 7], constellation: ["stars", 11], image: ["image", 3] }[kind];
    return `<div class="wall">${A.wallSvg(scene[0], 400, 225, scene[1])}</div>`;
  }
  function chooseWall(kind, image, generated, gallery) {
    const w = cfg().wallpaper;
    if (ui.wallMon === "all") {
      if (image) set("wallpaper.image", image);
      if (generated) set("wallpaper.generated", generated);
      if (gallery) set("wallpaper.gallery", gallery);
      set("wallpaper.kind", kind);
      if ((w.monitor || []).length) set("wallpaper.monitor", null);
      return;
    }
    const n = Number(ui.wallMon);
    const others = (w.monitor || []).filter((m) => m.index !== n);
    const old = monitorEntry(n) || {};
    const entry = { index: n, kind };
    const img = image || (kind === "image" ? old.image : "");
    if (img) entry.image = img;
    const gen = generated || (kind === "generated" ? old.generated : "");
    if (gen) entry.generated = gen;
    const gal = gallery || (kind === "gallery" ? old.gallery : "");
    if (gal) entry.gallery = gal;
    set("wallpaper.monitor", [...others, entry].sort((a, b) => a.index - b.index));
  }
  const genSeed = (id) => Number(String(id).replace(/\D/g, "").slice(-6)) || 7;
  const num = (v, digits) => Number(v).toLocaleString(S.lang, { minimumFractionDigits: digits, maximumFractionDigits: digits });

  /* The Create tab (crate::create in the bridge). */
  function genConnected(p) {
    return p === "ollama" ? !!(ui.ollama && ui.ollama.ok) : !!S.create.keys[p];
  }
  function genPanel() {
    const p = cfg().create.provider, connected = genConnected(p), g = ui.gen;
    let status, button;
    if (p === "ollama") {
      status = ui.ollama ? (ui.ollama.ok ? tv("genOllamaModel", escapeHtml(ui.ollama.model)) : tv("genOllamaMissing", escapeHtml(cfg().create.ollama_url))) : tv("genOllamaAt", escapeHtml(cfg().create.ollama_url));
      button = `<button class="set-btn ghost" type="button" data-genconnect>${t("gConnect")}</button>`;
    } else if (ui.keyEntry) {
      status = `<span class="num"><input type="password" id="gen-key" autocomplete="off" spellcheck="false" style="width:260px;font-family:var(--num)" placeholder="${t("genKeyPh")}" aria-label="${t("genKeyPh")}"></span>`;
      button = `<button class="set-btn" type="button" data-genkeysave disabled>${t("genKeySave")}</button><button class="set-btn ghost" type="button" data-genkeycancel>${t("setCancel")}</button>`;
    } else {
      status = connected ? t("gConnYes") : t("gConnNo");
      button = connected ? `<button class="set-btn ghost" type="button" data-genforget>${t("gDisconnect")}</button>` : `<button class="set-btn ghost" type="button" data-genkey>${t("gAddKey")}</button>`;
    }
    const running = g.phase === "running";
    const runBtn = running
      ? `<button class="set-btn ghost" type="button" data-genstop>${t("genStop")}</button>`
      : `<button class="set-btn" type="button" data-genrun ${connected && ui.prompt.trim() ? "" : "disabled"}>${t("gRun")}</button>`;
    const texts = [t("gS1"), t("gS2"), g.ms == null ? t("gS3").split(":")[0] : tv("genStepPerf", num(g.ms, 2)), g.average == null ? t("gS4").split(":")[0] : tv("genStepOled", num(g.average, 1))];
    const steps = texts.map((txt, i) => {
      const cls = i < g.step ? "ok" : i === g.step && running ? "run" : "";
      return `<li class="${cls}"><span class="dot">${cls === "ok" ? icon("check") : cls === "run" ? "●" : "·"}</span>${txt}</li>`;
    }).join("");
    const tries = g.phase === "idle" ? "" : `<p class="gen-note" data-gentries>${tv("genTry", g.attempt)}${g.reason && g.phase !== "failed" ? " · " + escapeHtml(g.reason) : ""}</p>`;
    const failed = g.phase === "failed" ? `<div class="note" role="alert" style="color:var(--err-fg)" data-genfailed>${escapeHtml(g.reason)}</div>` : "";
    const result = g.phase === "done" ? `<div class="saver gen-res"><span class="sp">${g.preview ? `<img src="${g.preview}" alt="" style="width:100%;height:100%;object-fit:cover">` : A.wallSvg("waves", 400, 225, 7)}</span><span class="sn">${escapeHtml(g.name)}<span>${t("wGenH")}</span></span><span class="acts"><button class="set-btn" type="button" data-genapply>${t("gApply")}</button><button class="set-btn ghost" type="button" data-genrun>${t("gRetry")}</button></span></div>` : "";
    // Ollama's address (PLAN M19 U11: it was in kabuk.toml alone).
    const address = p === "ollama" ? rowFor("create.ollama_url", t("genOllamaUrl"), t("genOllamaUrlH"), `<span class="num"><input type="text" id="gen-ollama-url" data-text="create.ollama_url" spellcheck="false" style="width:260px;font-family:var(--num)" value="${escapeHtml(cfg().create.ollama_url)}" placeholder="${escapeHtml(S.defaults.create.ollama_url)}" aria-label="${t("genOllamaUrl")}"></span>`) : "";
    return `<div class="set-grp">${rowFor("create.provider", t("gProv"), t("gProvH"), segFor("create.provider", t("gProv"), [["claude", "Claude"], ["openai", "OpenAI"], ["ollama", t("gOllama")]]))}
        ${address}
        ${row(t("gConn"), "", `<span class="conn"><span class="st${connected ? " yes" : ""}">${status}</span>${button}</span>`)}</div>
        <div class="set-grp"><span class="gl">${t("gPrompt")}</span><textarea class="gen-in" id="gen-prompt" rows="2" placeholder="${t("gPromptPh")}" aria-label="${t("gPrompt")}" ${running ? "disabled" : ""}>${escapeHtml(ui.prompt)}</textarea>
        <div class="gen-run">${runBtn}<span class="gen-note">${t("genNote")}</span></div></div>
        ${failed}${g.phase === "idle" ? "" : `<div class="gen-grid"><div><ul class="gen-steps">${steps}</ul>${tries}</div>${result}</div>`}`;
  }
  function wallPage() {
    const tabs = seg(t("navWall"), [["pick", t("wTabPick")], ["gen", t("wTabGen")]].map(([v, n]) => `<button type="button" data-walltab="${v}" class="${ui.wallTab === v ? "on" : ""}" aria-pressed="${ui.wallTab === v}">${n}</button>`).join(""));
    const top = `<div class="wall-tabs"><div><h3>${t("navWall")}</h3><p>${t("wallDesc")}</p></div>${tabs}</div>`;
    if (ui.wallTab === "gen") return top + genPanel();
    const count = Math.max(1, (S.info && S.info.monitors) || 1);
    const mons = [["all", t("wMonAll")], ...Array.from({ length: count }, (_, i) => [String(i + 1), i === 0 ? t("wMon1") : i === 1 ? t("wMon2") : tv("setRuleMonitor", i + 1)])];
    const monSeg = seg(t("wMon"), mons.map(([v, n]) => `<button type="button" data-wallmon="${v}" class="${ui.wallMon === v ? "on" : ""}" aria-pressed="${ui.wallMon === v}">${n}</button>`).join(""));
    const now = wallNow();
    const cards = WALLS.map(([k, name, hint]) => `<button class="saver${now === k ? " on" : ""}" type="button" data-wall="${k}" aria-pressed="${now === k}"><span class="sp">${wallArt(k)}</span><span class="sn">${t(name)}<span>${t(hint)}</span></span></button>`).join("")
      + S.create.walls.map((c) => {
        const on = now === "generated" && wallGenNow() === c.id;
        return `<button class="saver${on ? " on" : ""}" type="button" data-wall="generated" data-gen="${escapeHtml(c.id)}" aria-pressed="${on}"><span class="sp"><div class="wall">${A.wallSvg("waves", 400, 225, genSeed(c.id))}</div></span><span class="sn">${escapeHtml(c.name)}<span>${t("wGenH")}</span></span></button>`;
      }).join("");
    const w = cfg().wallpaper;
    const image = ui.wallMon === "all" ? w.image : (monitorEntry(Number(ui.wallMon)) || {}).image;
    askPreviews();
    const designs = (cat) => (S.gallery || []).filter((d) => d.category === cat);
    const wallCards = (cat) => galleryCards(designs(cat), (k) => now === "gallery" && wallGalNow() === k, (d) => `data-wall="gallery" data-gal="${escapeHtml(d.key)}"`, (d) => t(d.name + "-h"));
    const gallery = [["wall-live", "galWallsLive"], ["wall-still", "galWallsStill"]]
      .map(([cat, title]) => `<div class="set-grp"><span class="gl">${t(title)}</span><div class="walls">${wallCards(cat)}</div></div>`).join("");
    return top + `
      <div class="set-grp">${row(t("wMon"), t("wMonH"), monSeg)}</div>
      <div class="walls">${cards}</div>
      ${gallery}
      <div class="set-grp">${now === "image" ? row(t("wPick"), image ? escapeHtml(image) : t("wPickH"), `<button class="set-btn ghost" type="button" data-pick="image">${t("wPick")}</button>`) : ""}
        ${row(t("wBright"), t("wBrightH"), `<span class="rng"><input type="range" id="wall-bright" data-range="wallpaper.brightness" data-unit="%" min="${RANGES.bright[0]}" max="${RANGES.bright[1]}" step="${RANGES.bright[2]}" value="${w.brightness}" aria-label="${t("wBright")}"><span class="v" data-v="wall-bright">${w.brightness}%</span></span>`)}
        ${tgl("wallpaper.pause_when_hidden", t("wPause"), t("wPauseH"))}</div>`;
  }

  function saverPage() {
    const art = A.saverArt();
    const kind = cfg().screensaver.kind;
    const savers = [["circuit", "milva", "Milvasoft", t("sMilvaH")], ["constellation", "stars", t("sStars"), t("sStarsH")], ["none", "off", t("sOff"), t("sOffH")]];
    askPreviews();
    const designs = (cat) => (S.gallery || []).filter((d) => d.category === cat);
    // The live wallpapers play as screen savers too (asked for by the user); they are not
    // made for OLED screens, and say so.
    const hint = (d) => d.category === "wall-live" ? `${t(d.name + "-h")}. ${t("galWallSaverH")}` : d.live ? t(d.name + "-h") : `${t(d.name + "-h")}. ${t("galStillSaverH")}`;
    const saverCards = (cat) => galleryCards(designs(cat), (k) => kind === "gallery" && cfg().screensaver.gallery === k, (d) => `data-saver-gal="${escapeHtml(d.key)}"`, hint);
    const gallery = [["saver-live", "galSaversLive"], ["saver-still", "galSaversStill"], ["wall-live", "galWallsAsSavers"]]
      .map(([cat, title]) => `<div class="set-grp"><span class="gl">${t(title)}</span><div class="savers">${saverCards(cat)}</div></div>`).join("");
    // The Store's Kabuk cannot set Windows' screen saver (D89).
    if (S.store) return head(t("navSaver"), t("saverDesc")) + `<div class="note">${escapeHtml(t("setStoreMissing"))}</div>`;
    return head(t("navSaver"), t("saverDesc")) + `
      <div class="set-grp"><span class="gl">${t("saverPick")}</span><div class="savers">${savers.map(([k, a, n, h]) => `<button class="saver${kind === k ? " on" : ""}" type="button" data-set="screensaver.kind" data-val='"${k}"' aria-pressed="${kind === k}"><span class="sp">${art[a]}</span><span class="sn">${n}<span>${h}</span></span></button>`).join("")}</div></div>
      ${gallery}
      <div class="set-grp">${rowFor("screensaver.wait_minutes", t("wait"), t("waitH"), segFor("screensaver.wait_minutes", t("wait"), SAVER_WAITS.map((m) => [m, `${m} ${t("min")}`])))}
      ${tgl("screensaver.respect_video", t("video"), t("videoH"))}
      ${saverCornerRow()}
      ${row(t("preview"), t("previewH"), `<button class="set-btn ghost" type="button" data-preview-saver ${kind === "none" ? "disabled" : ""}>${t("previewBtn")}</button>`)}</div>`;
  }

  /* The hot corner that starts the screen saver (PLAN M19 U20): which one, chosen here as
     on the windows page, and that it works only while Kabuk runs. */
  function saverCornerRow() {
    const c = cfg().hot_corners;
    const now = (CORNERS.find(([k]) => c[k] === "screen_saver") || ["none"])[0];
    const name = (CORNERS.find(([k]) => k === now) || [])[1];
    const hint = name ? tv("setSaverCornerOn", t(name).toLocaleLowerCase(S.lang)) : t("setSaverCornerOff");
    const options = [["none", "hcNone"], ...CORNERS].map(([v, n]) => `<option value="${v}"${v === now ? " selected" : ""}>${t(n)}</option>`).join("");
    return row(t("setSaverCorner"), hint, `<span class="num"><select data-saver-corner aria-label="${t("setSaverCorner")}">${options}</select></span>`);
  }

  // The bar's colour (DECISIONS D57): dark tones that keep any look's text readable, one
  // light one, and any colour from the picker.
  const BAR_COLORS = ["#000000", "#16161A", "#1F2430", "#0E1A2B", "#1E1433", "#2B1220", "#122318", "#2E2414", "#E9E6E1"];
  // WCAG's contrast between two #rrggbb colours.
  const contrast = (a, b) => {
    const lum = (hex) => {
      const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
      return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    };
    const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
    return (x + 0.05) / (y + 0.05);
  };
  const HEX = /^#[0-9a-fA-F]{6}$/;
  function barColorRow(b) {
    const chosen = (b.color || "").toLowerCase();
    const own = !chosen;
    const fg = S.tokens["fg-s"];
    const low = !own && HEX.test(chosen) && HEX.test(fg || "") && contrast(chosen, fg) < 4.5;
    const swatches = `<button class="swatch def${own ? " on" : ""}" type="button" data-set="bar.color" data-val='""' aria-label="${t("setBarColorOwn")}" title="${t("setBarColorOwn")}"></button>`
      + BAR_COLORS.map((x) => `<button class="swatch${chosen === x.toLowerCase() ? " on" : ""}" type="button" data-set="bar.color" data-val='"${x}"' style="background:${x}" aria-label="${x}" title="${x}"></button>`).join("")
      + `<input type="color" class="swatch-pick" id="bar-color" value="${HEX.test(chosen) ? chosen : "#000000"}" aria-label="${t("setBarColorCustom")}" title="${t("setBarColorCustom")}">`;
    const label = `<span class="sw-label">${own ? t("setBarColorOwn") : escapeHtml(chosen)}</span>`;
    const warn = low ? `<span class="sw-warn" role="note">${t("setBarColorLow")}</span>` : "";
    return `<div class="set-grp"><span class="gl">${t("setBarColor")}. ${t("setBarColorH")}</span><div class="swatches">${swatches}${label}</div>${warn}</div>`;
  }

  // The design's order, with the apps after the title (DECISIONS D25).
  const MODULES = ["app_grid", "title", "taskbar", "clock", "date", "calendar", "media", "cpu", "ram", "gpu", "net", "vol", "tray", "privacy", "keyboard", "network", "battery", "quick_settings", "notif", "power", "show_desktop"];
  function barPage() {
    const b = cfg().bar;
    const own = A.lookPos[code()];
    const current = b.position === "default" ? own : b.position;
    const posHint = tv("posDefault", t("positions-" + own)) + (b.position !== "default" ? resetLink("bar.position") : "");
    const vertical = current === "left" || current === "right";
    const ownHeight = parseFloat(S.tokens[vertical ? "rail" : "bar-h"]) || RANGES.height[0];
    const height = b.height ?? ownHeight;
    const heightHint = tv("setHeightDefault", ownHeight) + (b.height != null ? resetLink("bar.height") : "");
    const trayRange = `<span class="rng"><input type="range" id="tray-shown" data-range="bar.tray_shown" data-unit="" min="${RANGES.tray[0]}" max="${RANGES.tray[1]}" value="${b.tray_shown}" aria-label="${t("setTrayShown")}"><span class="v" data-v="tray-shown">${b.tray_shown}</span></span>`;
    const chips = MODULES.map((k) => `<button type="button" class="chip${b.modules[k] ? " on" : ""}" data-tgl="bar.modules.${k}" aria-pressed="${b.modules[k]}">${icon("check")}${t("mod-" + k)}</button>`).join("");
    return head(t("navBar"), t("barDesc")) + `
      <div class="set-grp">${row(t("pos"), posHint, seg(t("pos"), choice("bar.position", current, ["top", "bottom", "left", "right"].map((p) => [p, t("positions-" + p)]))))}
      ${rowFor("bar.monitors", t("monitors"), t("monitorsH"), segFor("bar.monitors", t("monitors"), [["all", t("allMon")], ["main", t("mainMon")]]))}
      ${tgl("bar.autohide", t("autohide"), t("autohideH"))}
      ${rowFor("bar.background", t("setBg"), t("setBgH"), segFor("bar.background", t("setBg"), [["solid", t("setBgSolid")], ["blur", t("setBgBlur")], ["off", t("setBgOff")]]))}
      ${row(t("setHeight"), heightHint, range("bar-height", "bar.height", RANGES.height, height, t("setHeight")))}
      ${row(t("setTrayShown"), t("setTrayShownH"), trayRange)}</div>
      ${barColorRow(b)}
      <div class="set-grp"><span class="gl">${t("modules")}. ${t("modulesH")}</span>
        <div class="chips" role="group" aria-label="${t("modules")}">${chips}</div></div>`;
  }

  function windowsPage() {
    const w = cfg().windows;
    const gap = w.gap ?? A.lookGaps[code()];
    const gapHint = t("gapH") + (w.gap != null ? resetLink("windows.gap") : "");
    return head(t("navWindows"), t("winDesc")) + `
      <div class="set-grp">${rowFor("windows.default_layout", t("layoutDef"), t("layoutDefH"), segFor("windows.default_layout", t("layoutDef"), [["tiling", t("tiling")], ["floating", t("floating")]]))}
      ${rowFor("windows.tiling_layout", t("setTilingLayout"), t("setTilingLayoutH"), segFor("windows.tiling_layout", t("setTilingLayout"), [["dwindle", t("setLayoutDwindle")], ["master", t("setLayoutMaster")]]))}
      ${rowFor("windows.workspaces", t("setWorkspaces"), t("setWorkspacesH"), countRange("ws-count", "windows.workspaces", RANGES.workspaces, t("setWorkspaces")))}
      ${rowFor("windows.shown_workspaces", t("setShownWorkspaces"), t("setShownWorkspacesH"), countRange("ws-shown", "windows.shown_workspaces", RANGES.workspaces, t("setShownWorkspaces")))}
      ${tgl("windows.drag_to_float", t("dragFloat"), t("dragFloatH"))}
      ${tgl("windows.alt_drag", t("altDrag"), t("altDragH"))}
      ${tgl("windows.double_click_to_tile", t("setDoubleClick"), t("setDoubleClickH"))}
      ${row(t("gap"), gapHint, range("gap-range", "windows.gap", RANGES.gap, gap, t("gap")))}
      ${rowFor("windows.new_window", t("newWin"), t("newWinH"), segFor("windows.new_window", t("newWin"), [["right", t("right")], ["below", t("below")], ["floating", t("floating")]]))}
      ${tgl("windows.active_border", t("border"), t("borderH"))}
      ${tgl("windows.follow_look", t("setFollowLook"), t("setFollowLookH"))}
      ${rowFor("windows.minimize_effect", t("setMinimize"), t("setMinimizeH"), segFor("windows.minimize_effect", t("setMinimize"), [["genie", t("setMinimizeGenie")], ["windows", t("setMinimizeWindows")]]))}</div>
      ${dropdownGroup()}
      ${profilesGroup()}
      ${hotCornersGroup()}
      ${gesturesGroup()}`;
  }

  // Layout profiles (DECISIONS D65): programs over monitors and workspaces, saved from the
  // windows open now and opened again in one go.
  function profilesGroup() {
    const list = cfg().profile || [];
    const rows = list.map((p, i) => row(escapeHtml(p.name), tv("pfCount", (p.window || []).length), `<span style="display:flex;gap:10px"><button type="button" class="linkish" data-profile-open="${i}">${t("pfOpen")}</button><button type="button" class="linkish" data-profile-drop="${i}">${t("setRemove")}</button></span>`)).join("");
    return `<div class="set-grp"><span class="gl">${t("pf")}. ${t("pfH")}</span>${rows || `<div class="set-row"><span>${t("pfNone")}</span></div>`}
      <div style="display:flex;gap:8px;align-items:center"><span class="num"><input type="text" id="profile-name" style="width:200px" placeholder="${t("pfNamePh")}" aria-label="${t("pfName")}"></span><button class="set-btn ghost" type="button" data-profile-save>${t("pfSave")}</button></div>
      ${ui.profileMsg ? `<div class="note" role="status">${escapeHtml(ui.profileMsg)}</div>` : ""}</div>`;
  }

  // The drop-down terminal (DECISIONS D62): its program and how tall it drops.
  function dropdownGroup() {
    const d = cfg().dropdown;
    const command = `<span class="num"><input type="text" data-text="dropdown.command" style="width:260px;font-family:var(--num)" value="${escapeHtml(d.command)}" placeholder="${t("ddCommandPh")}" aria-label="${t("ddCommand")}"></span>`;
    const height = `<span class="rng"><input type="range" id="dd-height" data-range="dropdown.height" data-unit="%" min="${RANGES.dropdown[0]}" max="${RANGES.dropdown[1]}" step="${RANGES.dropdown[2]}" value="${d.height}" aria-label="${t("ddHeight")}"><span class="v" data-v="dd-height">${d.height}%</span></span>`;
    return `<div class="set-grp"><span class="gl">${t("dd")}. ${t("ddH")}</span>
      ${row(t("ddCommand"), t("ddCommandH"), command)}
      ${row(t("ddHeight"), "", height)}
      ${rowFor("shortcuts.dropdown", t("setKeyDropdown"), "", chordEditor("shortcuts.dropdown"))}</div>`;
  }

  // Hot corners (asked for by the user, off by default): what the mouse pushed into each
  // corner of the screen does.
  const CORNERS = [["top_left", "hcTopLeft"], ["top_right", "hcTopRight"], ["bottom_left", "hcBottomLeft"], ["bottom_right", "hcBottomRight"]];
  const CORNER_ACTIONS = [["none", "hcNone"], ["app_grid", "hcAppGrid"], ["overview", "hcOverview"], ["launcher", "hcLauncher"], ["screen_saver", "hcSaver"]];
  function hotCornersGroup() {
    const c = cfg().hot_corners;
    // A corner that starts the screen saver says what that does, and when (PLAN M19 U20).
    const rows = CORNERS.map(([k, name]) => row(t(name), c[k] === "screen_saver" ? t("hcSaverH") : "", `<span class="num"><select data-select="hot_corners.${k}" aria-label="${t(name)}">${CORNER_ACTIONS.map(([v, n]) => `<option value="${v}"${c[k] === v ? " selected" : ""}>${t(n)}</option>`).join("")}</select></span>`)).join("");
    return `<div class="set-grp"><span class="gl">${t("hc")}. ${t("hcH")}</span>${rows}</div>`;
  }

  // Touchpad swipes (DECISIONS D68, off by default): Windows' own gestures for as many
  // fingers must be set to "Nothing" first, so a button opens its touchpad page.
  const SWIPES = [["swipe_left", "gsLeft"], ["swipe_right", "gsRight"], ["swipe_up", "gsUp"], ["swipe_down", "gsDown"]];
  const SWIPE_ACTIONS = [["none", "hcNone"], ["next_workspace", "gsNext"], ["previous_workspace", "gsPrevious"], ["overview", "hcOverview"], ["app_grid", "hcAppGrid"], ["launcher", "hcLauncher"]];
  function gesturesGroup() {
    const g = cfg().gestures;
    const ways = g.enabled ? SWIPES.map(([k, name]) => row(t(name), "", `<span class="num"><select data-select="gestures.${k}" aria-label="${t(name)}">${SWIPE_ACTIONS.map(([v, n]) => `<option value="${v}"${g[k] === v ? " selected" : ""}>${t(n)}</option>`).join("")}</select></span>`)).join("") : "";
    const fingers = g.enabled ? rowFor("gestures.fingers", t("gsFingers"), "", segFor("gestures.fingers", t("gsFingers"), [[3, "3"], [4, "4"]])) : "";
    return `<div class="set-grp"><span class="gl">${t("gs")}. ${t("gsH")}</span>
      ${tgl("gestures.enabled", t("gsOn"), t("gsOnH"))}${fingers}${ways}
      <div><button class="set-btn ghost" type="button" data-touchpad-settings>${t("gsWindows")}</button></div></div>`;
  }

  /* ---------- window rules (SPEC §2.3) ---------- */
  const RULE_TEXTS = [["exe", "setRuleExe", "setRuleExePh"], ["class", "setRuleClass", "setRuleClassPh"], ["title", "setRuleTitle", "setRuleTitlePh"]];
  const RULE_SWITCHES = [["float", "setRuleFloat", "setRuleFloatH"], ["ignore", "setRuleIgnore", "setRuleIgnoreH"], ["no_grab", "setRuleNoGrab", "setRuleNoGrabH"]];
  const RULE_NUMBERS = [["min_width", "setRuleMin", "setRuleMinH", RANGES.minWidth]];
  const PLACEMENTS = ["left", "right", "top", "bottom", "center"];
  const ruleName = (r) => r.exe || r.class || r.title;
  function ruleText(r) {
    const parts = [];
    if (r.ignore) parts.push(t("setRuleIgnore"));
    if (r.float) parts.push(t("setRuleFloat"));
    if (r.workspace) parts.push(tv("setRuleWorkspace", r.workspace));
    if (r.monitor) parts.push(tv("setRuleMonitor", r.monitor));
    if (r.min_width) parts.push(tv("setRuleMinWidth", r.min_width));
    if (r.placement) parts.push(`${t("setRulePlace")}: ${t("setRulePlace-" + r.placement)}`);
    if (r.no_grab) parts.push(t("setRuleNoGrab"));
    return escapeHtml(parts.join(", "));
  }
  // What a rule matches: its program and class as code, its title as text.
  function ruleMatch(r) {
    const parts = [r.exe && `<code>${escapeHtml(r.exe)}</code>`, r.class && `<code>${escapeHtml(r.class)}</code>`, r.title && escapeHtml(r.title)].filter(Boolean);
    return parts.join(" · ") + (r.regex ? ` <span style="color:var(--dim)">· ${t("setRuleRegex")}</span>` : "");
  }
  /** A rule as the editor's fields read it (text, so a wrong number can be shown). */
  const ruleDraft = (r = {}, index = null) => ({
    index, exe: r.exe || "", class: r.class || "", title: r.title || "", regex: !!r.regex,
    float: !!r.float, ignore: !!r.ignore, no_grab: !!r.no_grab, placement: r.placement || "",
    workspace: r.workspace ? String(r.workspace) : "", monitor: r.monitor ? String(r.monitor) : "", min_width: r.min_width ? String(r.min_width) : "",
  });
  /** What keeps the rule from being saved, by field (`match` and `action` for the groups). */
  function ruleProblems(f) {
    const p = {};
    for (const [k, , , [min, max]] of RULE_NUMBERS) {
      const raw = f[k].trim();
      if (raw && !(/^\d+$/.test(raw) && Number(raw) >= min && Number(raw) <= max)) p[k] = tv("setRuleRange", `${min}–${max}`);
    }
    if (!RULE_TEXTS.some(([k]) => f[k].trim())) p.match = t("setRuleNeedMatch");
    if (!(f.float || f.ignore || f.no_grab || f.workspace || f.monitor.trim() || f.min_width.trim() || f.placement)) p.action = t("setRuleNeedAction");
    return p;
  }
  /** The rule the fields make, as kabuk.toml takes it. */
  function ruleOf(f) {
    const r = {};
    for (const [k] of RULE_TEXTS) if (f[k].trim()) r[k] = f[k].trim();
    if (f.regex) r.regex = true;
    if (f.float) r.float = true;
    if (f.ignore) r.ignore = true;
    if (f.workspace) r.workspace = Number(f.workspace);
    if (f.monitor.trim()) r.monitor = Number(f.monitor);
    if (f.min_width.trim()) r.min_width = Number(f.min_width);
    if (f.placement) r.placement = f.placement;
    if (f.no_grab) r.no_grab = true;
    return r;
  }
  /* Why kabuk.toml refused the rule, under the field it is about (the bridge names the
     key: `rules.rule[2].title`); about the rule as a whole or another one, at its foot. */
  function ruleFault(field) {
    const r = ui.refused, f = ui.ruleForm;
    if (!r || r.path !== "rules.rule" || !f) return "";
    const where = /^rules\.rule\[(\d+)\](?:\.(\w+))?$/.exec(r.at || "");
    const index = f.index ?? (cfg().rules.rule || []).length;
    const known = [...RULE_TEXTS, ...RULE_NUMBERS].map(([k]) => k).concat(["workspace", "monitor"]);
    const key = where && Number(where[1]) === index && known.includes(where[2]) ? where[2] : "";
    if (key !== field) return "";
    ui.refusalShown = true;
    return `<span class="h" role="alert" data-refusal style="display:block;color:var(--err-fg)">${escapeHtml(r.reason || t("setRefused"))}</span>`;
  }
  const ruleInput = (k, label, ph, numeric) => `<span class="num"><input type="text" id="rule-${k}" data-rule-field="${k}" ${numeric ? `inputmode="numeric" aria-describedby="rule-${k}-h"` : `spellcheck="false" style="width:260px"`} value="${escapeHtml(ui.ruleForm[k])}" placeholder="${escapeHtml(ph)}" aria-label="${escapeHtml(label)}"></span>`;
  const ruleSwitch = (k, label, hint) => row(label, hint, `<button class="tgl" type="button" role="switch" aria-checked="${ui.ruleForm[k]}" aria-label="${escapeHtml(label)}" data-rule-flag="${k}"></button>`);
  const ruleSelect = (k, label, options) => `<span class="num"><select id="rule-${k}" data-rule-field="${k}" aria-label="${escapeHtml(label)}">${[["", t("setRuleAny")], ...options].map(([v, n]) => `<option value="${v}"${ui.ruleForm[k] === String(v) ? " selected" : ""}>${n}</option>`).join("")}</select></span>`;
  /* The connected monitors by Windows' number, the main one named so (PLAN M19 U11: the
     monitor was a number to type), and the rule's own monitor while it is not connected. */
  function monitorOptions(chosen) {
    const screens = (S.info && S.info.screens) || [];
    const options = screens.map((m) => [m.number, tv(m.primary ? "setRuleMonitorMain" : "setRuleMonitor", m.number)]);
    const n = Number(chosen);
    if (chosen && !screens.some((m) => m.number === n)) options.push([n, tv("setRuleMonitorAway", n)]);
    return options.sort((a, b) => a[0] - b[0]);
  }
  // The open windows to fill the rule from (SPEC §2.3 "Pick an open window").
  function rulePicker() {
    if (!ui.rulePicking) return `<div><button class="set-btn ghost" type="button" data-rule-pick>${t("pickWin")}</button></div>`;
    if (ui.ruleWindows == null) return `<div class="h" role="status">${t("setRulePickWait")}</div>`;
    if (!ui.ruleWindows.length) return `<div class="h" role="status">${t("setRulePickNone")}</div>`;
    return `<div class="rule-wins" role="group" aria-label="${t("pickWin")}">${ui.ruleWindows.map((w, i) => `<button type="button" class="rule-win" data-rule-win="${i}"><b>${escapeHtml(w.title || w.exe || w.class)}</b><span>${escapeHtml([w.exe, w.class].filter(Boolean).join(" · "))}</span></button>`).join("")}</div>`;
  }
  function askWindows() {
    ui.rulePicking = true;
    ui.ruleWindows = null;
    post({ op: "windows" });
  }
  function ruleEditor() {
    const f = ui.ruleForm;
    const workspaces = Array.from({ length: cfg().windows.workspaces }, (_, i) => [i + 1, tv("setRuleWorkspace", i + 1)]);
    const texts = RULE_TEXTS.map(([k, label, ph]) => row(t(label), ruleFault(k), ruleInput(k, t(label), t(ph), false))).join("");
    const numbers = RULE_NUMBERS.map(([k, label, hint, [min, max]]) => `<div class="set-row"><span><b>${t(label)}</b><span class="h">${t(hint)}${ruleFault(k)}</span></span>${ruleInput(k, t(label), `${min}–${max}`, true)}<span class="num-h" id="rule-${k}-h" aria-live="polite"></span></div>`).join("");
    return `<div class="set-grp rule-edit" data-rule-editor>
        <span class="gl">${t("setRuleMatch")}. ${t("setRuleMatchH")}</span>
        ${rulePicker()}
        ${texts}
        ${ruleSwitch("regex", t("setRuleRegex"), t("setRuleRegexH"))}
        <span class="h" data-rule-note="match" role="status"></span>
        <span class="gl">${t("setRuleDo")}. ${t("setRuleDoH")}</span>
        ${RULE_SWITCHES.map(([k, label, hint]) => ruleSwitch(k, t(label), t(hint))).join("")}
        ${row(t("setRuleWs"), t("setRuleWsH") + ruleFault("workspace"), ruleSelect("workspace", t("setRuleWs"), workspaces))}
        ${row(t("setRuleMon"), t("setRuleMonH") + ruleFault("monitor"), ruleSelect("monitor", t("setRuleMon"), monitorOptions(f.monitor)))}
        ${numbers}
        ${row(t("setRulePlace"), t("setRulePlaceH"), ruleSelect("placement", t("setRulePlace"), PLACEMENTS.map((p) => [p, t("setRulePlace-" + p)])))}
        <span class="h" data-rule-note="action" role="status"></span>
        ${ruleFault("")}
        <div><button class="set-btn" type="button" data-rule-save>${f.index == null ? t("setAdd") : t("setSave")}</button> <button class="set-btn ghost" type="button" data-rule-cancel>${t("setCancel")}</button></div></div>`;
  }
  /* The editor's state from its fields, without drawing it again (the caret stays): each
     wrong number and what the rule still lacks, and Save only with nothing wrong. */
  function checkRule() {
    const f = ui.ruleForm;
    if (!f || !document.querySelector("[data-rule-editor]")) return;
    const p = ruleProblems(f);
    // The range is the field's placeholder; under it only what is wrong.
    for (const [k] of RULE_NUMBERS) {
      const input = document.getElementById(`rule-${k}`), hint = document.getElementById(`rule-${k}-h`);
      input.setAttribute("aria-invalid", String(!!p[k]));
      hint.classList.toggle("err", !!p[k]);
      hint.hidden = !p[k];
      hint.textContent = p[k] || "";
    }
    for (const g of ["match", "action"]) document.querySelector(`[data-rule-note="${g}"]`).textContent = p[g] || "";
    document.querySelector("[data-rule-save]").disabled = Object.keys(p).length > 0;
  }
  // The built-in rules' programs that keep their Alt + clicks (kabuk-core rules::NO_GRAB_PROGRAMS).
  const NO_GRAB = ["blender.exe", "Code.exe", "Photoshop.exe"];
  function rulesPage() {
    const rules = cfg().rules.rule || [];
    const defaults = cfg().rules.defaults ? [[t("rDialogs"), t("rDialogsR")], [t("rSplash"), t("rSplashR")], ["<code>Taskmgr.exe</code>", t("rTaskmgr")], ...NO_GRAB.map((exe) => [`<code>${exe}</code>`, t("setRuleNoGrab")])].map(([a, r]) => `<div>${a}</div><div>${r}</div><div class="src">${t("srcDefault")}</div>`).join("") : "";
    const f = ui.ruleForm;
    // A rule is edited in place: its editor right under its row (PLAN M19 U11).
    const mine = rules.map((r, i) => `<div>${ruleMatch(r)}</div><div>${ruleText(r)}</div><div class="src mine">${t("srcMine")} · <button type="button" class="linkish" data-rule-edit="${i}">${t("setEdit")}</button> · <button type="button" class="linkish" data-rule-drop="${i}">${t("setRemove")}</button></div>`
      + (f && f.index === i ? `<div class="rule-edit-row">${ruleEditor()}</div>` : "")).join("");
    const form = f ? (f.index == null ? ruleEditor() : "") : `<div><button class="set-btn" type="button" data-rule-new>${t("addRule")}</button> <button class="set-btn ghost" type="button" data-rule-new="pick">${t("pickWin")}</button></div>`;
    return head(t("navRules"), t("rulesDesc")) + `
      <div class="set-grp">${tgl("rules.float_unfit", t("autofloat"), t("autofloatH"))}
      ${tgl("rules.defaults", t("setRuleDefaults"), t("setRuleDefaultsH"))}</div>
      <div class="rules-t"><div class="h">${t("colApp")}</div><div class="h">${t("colRule")}</div><div class="h src">${t("colSrc")}</div>${defaults}${mine}</div>
      ${rules.length ? "" : `<div class="note">${t("setRuleNone")}</div>`}
      ${form}`;
  }

  function gamePage() {
    return head(t("navGame"), t("gameDesc")) + `
      <div class="set-grp">${tgl("game.detect", t("gDetect"), t("gDetectH"))}
      ${tgl("game.hide_bar", t("gBar"), t("gBarH"))}
      ${tgl("game.pause_tiling", t("gPause"), t("gPauseH"))}
      ${tgl("game.silence_notifications", t("gQuiet"), t("gQuietH"))}
      ${tgl("game.other_monitors_normal", t("gOther"), t("gOtherH"))}</div>
      <div class="note">${t("setGameNote")}</div>`;
  }

  const SOURCES = ["windows", "apps", "recent", "files", "bookmarks", "commands", "calculator"];
  function launcherPage() {
    const l = cfg().launcher;
    const chips = SOURCES.map((k) => `<button type="button" class="chip${l.sources[k] ? " on" : ""}" data-tgl="launcher.sources.${k}" aria-pressed="${l.sources[k]}">${icon("check")}${t("setSrc-" + k)}</button>`).join("");
    const folders = l.indexed_folders.map((f, i) => `<div class="set-row"><span><b style="font-family:var(--num);font-weight:400">${escapeHtml(f)}</b></span><button type="button" class="linkish" data-folder-drop="${i}">${t("setRemove")}</button></div>`).join("");
    return head(t("navLauncher"), t("lpDesc")) + `
      <div class="set-grp">${row(t("lpKey"), t("lpKeyH"), chordEditor("shortcuts.launcher"))}</div>
      <div class="set-grp"><span class="gl">${t("lpSrc")}. ${t("lpSrcH")}</span><div class="chips" role="group" aria-label="${t("lpSrc")}">${chips}</div></div>
      <div class="set-grp"><span class="gl">${t("lpDirs")}. ${t("lpDirsH")}</span>${folders}
        <div><button class="set-btn ghost" type="button" data-pick="folder">${t("setFolderAdd")}</button></div></div>
      ${appGridGroup()}`;
  }

  // The all-apps grid's arrangement (asked for by the user): the apps hidden on the grid,
  // each shown again by a click, and its order back to alphabetical.
  function appGridGroup() {
    const g = S.app_grid || { hidden: [], ordered: false };
    const hidden = g.hidden.map((h) => `<div class="set-row"><span><b>${escapeHtml(h.name)}</b></span><button type="button" class="linkish" data-grid-show="${escapeHtml(h.open)}">${t("lpGridShow")}</button></div>`).join("")
      || `<div class="set-row"><span>${t("lpGridNone")}</span></div>`;
    // A part of its own under a line and its title (asked for by the user: it read as
    // more of the launcher's folders).
    return `<div class="set-part"><h4>${t("lpGridTitle")}</h4></div>
      <div class="set-grp"><span class="gl">${t("lpGridH")}</span>${hidden}
        <div><button class="set-btn ghost" type="button" data-grid-reset ${g.ordered ? "" : "disabled"}>${t("lpGridReset")}</button></div></div>`;
  }

  const KEYS = ["launcher", "workspace", "move_to_workspace", "focus_left", "focus_down", "focus_up", "focus_right", "move_left", "move_down", "move_up", "move_right", "toggle_float", "close_window", "cycle_layout", "toggle_scratchpad", "to_scratchpad", "toggle_group", "next_tab", "overview", "start_saver", "app_grid", "keys_card", "dropdown", "drag"];
  function keysPage() {
    const any = KEYS.some((k) => changed("shortcuts." + k));
    return head(t("navKeys"), t("keysDesc")) + (any ? `<div><button class="set-btn ghost" type="button" data-reset-keys>${t("setResetKeys")}</button></div>` : "") + `
      <div class="keys">${KEYS.map((k) => `<div>${t("setKey-" + k.replaceAll("_", "-"))}</div><div>${chordEditor("shortcuts." + k)}</div>`).join("")}</div>`;
  }

  function langPage() {
    const sys = S.lang_names[S.system_lang];
    const now = new Date();
    const date = `${t("days-" + now.getDay())} ${now.getDate()} ${t("months-" + now.getMonth())}`;
    return head(t("navLang"), t("langDesc")) + `
      <div class="set-grp">${rowFor("language.language", t("langLbl"), "", segFor("language.language", t("langLbl"), [["system", `${t("system")} (${sys})`], ["tr", S.lang_names.tr], ["en", S.lang_names.en]]))}
      ${rowFor("language.clock", t("timeFmt"), "", segFor("language.clock", t("timeFmt"), [["24h", t("h24")], ["12h", t("h12")]]))}
      ${row(t("dateLbl"), t("dateH"), `<span class="h" style="color:var(--fg-s);font-family:var(--num)">${date}</span>`)}
      ${row(t("decimal"), "", `<span class="h" style="color:var(--fg-s);font-family:var(--num)">${t("decimalV")}</span>`)}</div>
      <div class="note">${t("langNote")}</div>`;
  }

  const dimLabel = (sec) => sec % 60 === 0 ? `${sec / 60} ${t("unitM")}` : `${sec} ${t("unitS")}`;
  function dimRow() {
    const o = cfg().oled;
    const value = o.dim_after_secs;
    const custom = ui.dimCustom ?? !DIM_PRESETS.includes(value);
    const unitM = value % 60 === 0 && value >= 60;
    const presets = [[30, t("s30")], [60, t("m1")], [120, t("m2")]].map(([v, n]) => {
      const on = !custom && value === v;
      return `<button type="button" data-dim="${v}" class="${on ? "on" : ""}" aria-pressed="${on}">${n}</button>`;
    }).join("") + `<button type="button" data-dim="custom" class="${custom ? "on" : ""}" aria-pressed="${custom}">${t("custom")}</button>`;
    const inputs = custom ? `
      <input type="number" id="dim-value" inputmode="numeric" min="1" max="${unitM ? 60 : DIM_MAX}" step="1" value="${unitM ? value / 60 : value}" aria-label="${t("dimValue")}" aria-describedby="dim-hint">
      <select id="dim-unit" aria-label="${t("dimUnit")}"><option value="s"${unitM ? "" : " selected"}>${t("unitS")}</option><option value="m"${unitM ? " selected" : ""}>${t("unitM")}</option></select>` : "";
    const hint = custom ? `<span class="num-h" id="dim-hint" aria-live="polite">${t("dimRange")} · ${tv("dimNow", dimLabel(value))}</span>` : "";
    return `<div class="set-row${o.idle_dim ? "" : " off"}"><span><b>${t("dimAfter")}</b><span class="h">${t("dimAfterH")}</span></span><span class="num">${seg(t("dimAfter"), presets)}${inputs}</span>${hint}</div>`;
  }
  /* The custom dimming delay: a whole number, 10 s to 60 min; anything else is an error. */
  function readDim() {
    const input = document.getElementById("dim-value");
    const unit = document.getElementById("dim-unit");
    const hint = document.getElementById("dim-hint");
    if (!input || !unit || !hint) return;
    const raw = input.value.trim();
    const n = /^\d+$/.test(raw) ? Number(raw) : NaN;
    const sec = unit.value === "m" ? n * 60 : n;
    const ok = Number.isInteger(sec) && sec >= DIM_MIN && sec <= DIM_MAX;
    input.setAttribute("aria-invalid", String(!ok));
    input.max = unit.value === "m" ? 60 : DIM_MAX;
    hint.classList.toggle("err", !ok);
    hint.textContent = ok ? `${t("dimRange")} · ${tv("dimNow", dimLabel(sec))}` : t("dimBad");
    return ok ? sec : null;
  }
  // How often a still picture drifts (DECISIONS D47); off while it does not drift.
  function driftRow() {
    const w = cfg().wallpaper;
    const path = "wallpaper.drift_minutes";
    const control = `<span class="rng"><input type="range" id="drift-minutes" data-range="${path}" data-unit=" ${t("min")}" min="${RANGES.drift[0]}" max="${RANGES.drift[1]}" value="${w.drift_minutes}" aria-label="${t("setDriftEvery")}"><span class="v" data-v="drift-minutes">${w.drift_minutes} ${t("min")}</span></span>`;
    return row(t("setDriftEvery"), t("setDriftEveryH") + resetIfChanged(path) + refusalFor(path), control, w.drift ? "" : " off");
  }
  function oledPage() {
    return head(t("navOled"), t("oledDesc")) + `
      <div class="set-grp">${tgl("oled.bar_shift", t("shift"), t("shiftH"))}
      ${tgl("wallpaper.drift", t("setDrift"), tv("setDriftH", cfg().wallpaper.drift_minutes))}
      ${driftRow()}
      ${tgl("oled.idle_dim", t("dim"), t("dimH"))}
      ${dimRow()}</div>`;
  }

  /** What the chosen mode does (found using Settings: "By time of day…" showed under "Dark"). */
  function modeHint() {
    return { dark: t("setModeDarkH"), light: t("setModeLightH"), system: t("setModeSystemH") }[get("appearance.mode")] ?? t("modeH");
  }

  function aboutPage() {
    const i = S.info || {};
    const meters = [[i.components ?? 0, t("mComp")], [i.monitors ?? 0, t("mMon")], [i.windows ?? 0, t("mWin")], [i.memory_mb ?? "–", t("setMem")]];
    return head(t("navAbout"), tv("setVersion", S.version)) + `
      <div class="meter" style="grid-template-columns:repeat(4,1fr)">${meters.map(([v, n]) => `<div><b>${escapeHtml(v)}</b><span>${n}</span></div>`).join("")}</div>
      ${i.running ? row(t("setQuit"), t("setQuitH"), `<button class="set-btn ghost" type="button" data-quit>${t("setQuitBtn")}</button>`) : `<div class="note">${t("setNotRunning")}</div>`}
      ${updateRow()}
      ${settingsFileGroup()}`;
  }
  /* The settings file (PLAN M19 U20): kabuk.toml saved to a file and read back, and every
     setting back to its default behind a confirmation. An import or a reset keeps the file
     as it was first, so the settings before it can be brought back. */
  function settingsFileGroup() {
    const m = ui.fileMsg;
    const alert = (text, list = "") => `<div class="note" role="alert" style="color:var(--err-fg)" data-file-said>${escapeHtml(text)}${list}</div>`;
    const said = !m ? ""
      : m.problems ? alert(tv("setFileRefused", m.path), `<ul>${m.problems.map((p) => `<li>${escapeHtml(p)}</li>`).join("")}</ul>`)
      : m.failed ? alert(m.text)
      : `<div class="note" role="status" data-file-said>${escapeHtml(m.text)}${m.undo ? ` <button type="button" class="linkish" data-settings-undo>${t("setFileUndo")}</button>` : ""}</div>`;
    const confirm = ui.confirmReset ? `<div class="note" role="alertdialog" aria-labelledby="reset-all-q" aria-describedby="reset-all-d" data-reset-confirm>
        <b id="reset-all-q">${t("setResetAllQ")}</b><p id="reset-all-d">${t("setResetAllD")}</p>
        <div style="display:flex;gap:8px"><button class="set-btn" type="button" data-reset-all-yes>${t("setResetAllYes")}</button><button class="set-btn ghost" type="button" data-reset-all-no>${t("setCancel")}</button></div></div>` : "";
    const button = (attr, label, extra = "") => `<button class="set-btn ghost" type="button" ${attr} ${extra}>${label}</button>`;
    return `<div class="set-part"><h4>${t("setFile")}</h4></div>
      <div class="set-grp"><span class="gl">${t("setFileH")}</span>
        ${row(t("setFileExport"), t("setFileExportH"), button(`data-settings-file="export"`, t("setFileExportBtn")))}
        ${row(t("setFileImport"), t("setFileImportH"), button(`data-settings-file="import"`, t("setFileImportBtn")))}
        ${row(t("setResetAll"), t("setResetAllH"), button("data-reset-all", t("setResetAllBtn"), ui.confirmReset ? "disabled" : ""))}
        ${confirm}${said}</div>`;
  }
  /** "Check for updates" and what Kabuk's latest GitHub release said. */
  function updateRow() {
    // Installed from the Microsoft Store, which updates it (DECISIONS D84).
    if (S && S.store) return `<div class="note" data-update-said="store">${escapeHtml(t("setUpdateStore"))}</div>`;
    const u = ui.update || {};
    const checking = u.state === "checking";
    const check = `<button class="set-btn" type="button" data-update-check ${checking ? "disabled" : ""}>${t("update")}</button>`;
    const said = {
      checking: t("setUpdateChecking"),
      latest: t("setUpdateLatest"),
      failed: t("setUpdateFailed"),
      newer: u.version ? tv("setUpdateNewer", u.version) : "",
    }[u.state];
    const open = u.state === "newer" ? ` <button class="set-btn ghost" type="button" data-update-open>${t("setUpdateGet")}</button>` : "";
    return `<div>${check}${open}</div>${said ? `<div class="note" data-update-said="${u.state}">${escapeHtml(said)}</div>` : ""}`;
  }

  const PAGES = { look: lookPage, wall: wallPage, saver: saverPage, bar: barPage, windows: windowsPage, rules: rulesPage, game: gamePage, launcher: launcherPage, keys: keysPage, lang: langPage, oled: oledPage, about: aboutPage };

  /* ---------- search (PLAN M19 U20) ---------- */
  /* Settings found by their label, hint and page in the language in use: every page drawn
     as it opens, off screen, and what it shows read back, so nothing is listed twice and a
     new row is found without more work. A hit opens its page at the setting. */
  const FINDABLE = ".set-row, .set-grp > .gl, .chip, .keys > div:nth-child(odd), .theme-card, .saver";
  // The page state a page opens with (as a click in the menu leaves it).
  const OPENED = { refused: null, capture: null, ruleForm: null, rulePicking: false, dimCustom: null, fileMsg: null, confirmReset: false, wallTab: "pick" };
  let searchIndex = null; // { of: S, entries }, drawn again when the state changes
  const textOf = (el) => {
    if (!el) return "";
    const c = el.cloneNode(true);
    // A text node has nothing in it to leave out.
    c.querySelectorAll?.("button, [data-refusal]").forEach((x) => x.remove());
    return c.textContent.replace(/\s+/g, " ").replace(/^[\s·]+|[\s·]+$/g, "");
  };
  // What a found element is called and what is said of it.
  function findable(el) {
    if (el.matches(".set-row")) return [textOf(el.querySelector(":scope > span > b")), textOf(el.querySelector(":scope > span > .h"))];
    if (el.matches(".gl")) { const [label, ...hint] = el.textContent.split(". "); return [label, hint.join(". ")]; }
    if (el.matches(".saver")) return [textOf(el.querySelector(".sn")?.firstChild), textOf(el.querySelector(".sn > span"))];
    if (el.matches(".theme-card")) return [textOf(el.querySelector(".tn")?.firstChild), ""];
    return [textOf(el), ""];
  }
  function indexSettings() {
    if (searchIndex && searchIndex.of === S) return searchIndex.entries;
    // Drawn to be read, not shown: the gallery's previews are not asked for.
    const kept = Object.fromEntries([...Object.keys(OPENED), "previewsAsked"].map((k) => [k, ui[k]]));
    const entries = [];
    try {
      for (const [page] of navItems) {
        Object.assign(ui, OPENED, { previewsAsked: true });
        const tpl = document.createElement("template");
        tpl.innerHTML = PAGES[page]();
        const section = t("nav-" + page);
        entries.push({ page, n: -1, label: section, hint: textOf(tpl.content.querySelector("h3 + p")), section });
        tpl.content.querySelectorAll(FINDABLE).forEach((el, n) => {
          const [label, hint] = findable(el);
          if (label) entries.push({ page, n, label, hint, section });
        });
      }
    } finally {
      Object.assign(ui, kept);
    }
    searchIndex = { of: S, entries };
    return entries;
  }
  // Every word typed is in the setting's label, hint or page; those found by their label first.
  function searchSettings(query) {
    const low = (s) => s.toLocaleLowerCase(S.lang);
    const words = low(query).split(/\s+/).filter(Boolean);
    const all = indexSettings().filter((e) => words.every((w) => low(`${e.label} ${e.hint} ${e.section}`).includes(w)));
    const byLabel = all.filter((e) => words.every((w) => low(e.label).includes(w)));
    return [...byLabel, ...all.filter((e) => !byLabel.includes(e))];
  }
  function searchPage() {
    ui.hits = searchSettings(ui.search);
    const hits = ui.hits.map((h, i) => `<button type="button" data-found="${i}"><b>${escapeHtml(h.label)}</b><span>${escapeHtml(h.n < 0 ? h.hint : h.section + (h.hint ? " · " + h.hint : ""))}</span></button>`).join("");
    return `<div><h3>${t("setSearchTitle")}</h3><p role="status">${hits ? tv("setSearchCount", ui.hits.length) : t("setSearchNone")}</p></div>
      ${hits ? `<div class="set-found" role="group" aria-label="${t("setSearchTitle")}">${hits}</div>` : ""}`;
  }
  // A hit opens its page at the setting, marked, its control focused.
  function goToFound(hit) {
    Object.assign(ui, OPENED, { page: hit.page, search: "", found: hit.n < 0 ? null : { page: hit.page, n: hit.n } });
    capture(false);
    if (ui.page === "about" || ui.page === "wall" || ui.page === "rules") post({ op: "info" });
    render();
    const el = document.querySelector(".set-main .found");
    if (!el) { document.querySelector(`[data-page="${hit.page}"]`)?.focus({ preventScroll: true }); return; }
    el.scrollIntoView({ block: "center" });
    (el.matches("button") ? el : el.querySelector("button, input, select, textarea"))?.focus({ preventScroll: true });
  }

  /* ---------- drawing ---------- */
  function render() {
    if (!S) return;
    const app = document.getElementById("app");
    const scrolled = app.querySelector(".set-main")?.scrollTop || 0;
    const focused = document.activeElement?.id;
    // Where the caret was in the field typed in (the search field draws the page again).
    let caret = null;
    try { caret = focused && [document.activeElement.selectionStart, document.activeElement.selectionEnd]; } catch { caret = null; }
    ui.refusalShown = false;
    const page = ui.search.trim() ? searchPage() : PAGES[ui.page]();
    // Refused where no row of this page shows it: said at the top.
    const refused = ui.refused && !ui.refusalShown
      ? `<div class="note" role="alert" style="color:var(--err-fg)">${escapeHtml(refusalText())}</div>` : "";
    // kabuk.toml has a mistake: what and where, and a way to it (PLAN M19 U1).
    const problems = (S.problems || []).length
      ? `<div class="note" role="alert" style="color:var(--err-fg)">${t("setProblems")}<ul>${S.problems.map((p) => `<li>${escapeHtml(p)}</li>`).join("")}</ul><button class="set-btn ghost" type="button" data-open-config>${t("setOpenConfig")}</button></div>` : "";
    app.innerHTML = `<div class="screen settings-screen" data-dir="${code()}" data-mode="${S.mode}" data-motion="${cfg().appearance.animations}">
      <div class="set-win">
        <nav class="set-side" aria-label="${t("setNav")}"><div class="set-brand"><img class="mark" src="mark-${S.mode === "light" ? "light" : "dark"}.svg" alt="">${t("setBrand")}</div>
          <div class="set-find" role="search">${icon("search")}<input type="search" id="set-search" value="${escapeHtml(ui.search)}" placeholder="${t("setSearch")}" aria-label="${t("setSearch")}" autocomplete="off" spellcheck="false"></div>
          ${navItems.map(([k, ic]) => `<button class="set-nav${ui.page === k ? " on" : ""}" type="button" data-page="${k}" aria-current="${ui.page === k ? "page" : "false"}">${icon(ic)}${t("nav-" + k)}</button>`).join("")}
        </nav>
        <div class="set-main">${problems}${refused}${page}</div>
      </div></div>`;
    const screen = app.querySelector(".screen");
    for (const [k, v] of Object.entries(S.tokens)) {
      screen.style.setProperty("--" + k, v);
      document.documentElement.style.setProperty("--" + k, v);
    }
    app.querySelector(".set-main").scrollTop = scrolled;
    if (ui.found && ui.found.page === ui.page && !ui.search.trim()) app.querySelector(".set-main").querySelectorAll(FINDABLE)[ui.found.n]?.classList.add("found");
    if (focused) document.getElementById(focused)?.focus({ preventScroll: true });
    if (caret && caret[0] != null) { try { document.getElementById(focused)?.setSelectionRange(caret[0], caret[1]); } catch { /* not a text field */ } }
    checkRule();
    document.title = t("setBrand");
  }

  /* ---------- input ---------- */
  document.addEventListener("click", (e) => {
    const on = (sel) => e.target.closest(sel);
    let el;
    ui.found = null;
    if ((el = on("[data-found]"))) { const hit = (ui.hits || [])[Number(el.dataset.found)]; if (hit) goToFound(hit); return; }
    if ((el = on("[data-page]"))) {
      ui.page = el.dataset.page; ui.refused = null; ui.capture = null; capture(false); ui.ruleForm = null; ui.rulePicking = false; ui.dimCustom = null;
      ui.fileMsg = null; ui.confirmReset = false; ui.search = "";
      if (ui.page === "about" || ui.page === "wall" || ui.page === "rules") post({ op: "info" });
      render();
      document.querySelector(`[data-page="${ui.page}"]`)?.focus({ preventScroll: true });
      return;
    }
    if ((el = on("[data-set]"))) { set(el.dataset.set, JSON.parse(el.dataset.val)); return; }
    if ((el = on("[data-tgl]"))) { set(el.dataset.tgl, !get(el.dataset.tgl)); return; }
    if ((el = on("[data-reset]"))) { set(el.dataset.reset, el.dataset.reset === "bar.position" ? "default" : null); return; }
    if (on("[data-reset-keys]")) { KEYS.filter((k) => changed("shortcuts." + k)).forEach((k) => set("shortcuts." + k, null)); return; }
    if ((el = on("[data-walltab]"))) { ui.wallTab = el.dataset.walltab; render(); return; }
    if ((el = on("[data-wallmon]"))) { ui.wallMon = el.dataset.wallmon; render(); return; }
    if ((el = on("[data-wall]"))) {
      if (el.dataset.wall === "image") { ui.pending = "wall"; post({ op: "pick", what: "image" }); }
      else chooseWall(el.dataset.wall, null, el.dataset.gen, el.dataset.gal);
      return;
    }
    if ((el = on("[data-saver-gal]"))) {
      // The design first: a gallery saver needs one (DECISIONS D58).
      set("screensaver.gallery", el.dataset.saverGal);
      set("screensaver.kind", "gallery");
      return;
    }
    if (on("[data-genkey]")) { ui.keyEntry = true; render(); document.getElementById("gen-key")?.focus(); return; }
    if (on("[data-genkeycancel]")) { ui.keyEntry = false; render(); return; }
    if (on("[data-genkeysave]")) {
      const input = document.getElementById("gen-key");
      const key = input ? input.value.trim() : "";
      if (input) input.value = "";
      ui.keyEntry = false;
      if (key) post({ op: "gen-key", provider: cfg().create.provider, key });
      return;
    }
    if (on("[data-genforget]")) { post({ op: "gen-forget", provider: cfg().create.provider }); return; }
    if (on("[data-genconnect]")) { post({ op: "gen-connect" }); return; }
    if (on("[data-genrun]")) {
      if (!ui.prompt.trim()) return;
      ui.gen = { phase: "running", step: 0, attempt: 1, ms: null, average: null, reason: "", name: "", preview: null };
      post({ op: "gen-run", prompt: ui.prompt });
      render();
      return;
    }
    if (on("[data-genstop]")) { post({ op: "gen-cancel" }); return; }
    if (on("[data-genapply]")) { post({ op: "gen-apply" }); return; }
    if ((el = on("[data-profile-save]"))) {
      const name = (document.getElementById("profile-name") || {}).value || "";
      if (!name.trim()) { ui.profileMsg = t("pfNameMissing"); render(); return; }
      ui.profileMsg = ""; post({ op: "profile-save", name: name.trim() }); return;
    }
    if ((el = on("[data-profile-open]"))) { const p = (cfg().profile || [])[Number(el.dataset.profileOpen)]; if (p) post({ op: "profile-open", name: p.name }); return; }
    if ((el = on("[data-profile-drop]"))) { const i = Number(el.dataset.profileDrop); set("profile", (cfg().profile || []).filter((_, j) => j !== i)); return; }
    if ((el = on("[data-theme-file]"))) { ui.themeMsg = ""; post({ op: "theme-" + el.dataset.themeFile }); return; }
    // The settings file (PLAN M19 U20); "Reset all" asks first.
    if ((el = on("[data-settings-file]"))) { ui.fileMsg = null; render(); post({ op: "settings-" + el.dataset.settingsFile }); return; }
    if (on("[data-settings-undo]")) { ui.fileMsg = null; post({ op: "settings-import", backup: true }); return; }
    if (on("[data-reset-all]")) { ui.confirmReset = true; ui.fileMsg = null; render(); document.querySelector("[data-reset-all-no]")?.focus(); return; }
    if (on("[data-reset-all-no]")) { ui.confirmReset = false; render(); document.querySelector("[data-reset-all]")?.focus(); return; }
    if (on("[data-reset-all-yes]")) { ui.confirmReset = false; render(); post({ op: "settings-reset" }); return; }
    if ((el = on("[data-grid-show]"))) { post({ op: "app-grid-show", open: el.dataset.gridShow }); return; }
    if ((el = on("[data-grid-reset]"))) { post({ op: "app-grid-reset" }); return; }
    if ((el = on("[data-pick]"))) { ui.pending = el.dataset.pick === "image" ? "wall" : "folder"; post({ op: "pick", what: el.dataset.pick }); return; }
    if (on("[data-preview-saver]")) { post({ op: "preview-saver" }); return; }
    if (on("[data-quit]")) { post({ op: "quit" }); return; }
    if (on("[data-update-check]")) { post({ op: "update-check" }); return; }
    if (on("[data-update-open]")) { if (ui.update && ui.update.page) post({ op: "update-open", page: ui.update.page }); return; }
    if (on("[data-touchpad-settings]")) { post({ op: "touchpad-settings" }); return; }
    if (on("[data-open-config]")) { post({ op: "open-config" }); return; }
    if ((el = on("[data-folder-drop]"))) { set("launcher.indexed_folders", cfg().launcher.indexed_folders.filter((_, i) => i !== Number(el.dataset.folderDrop))); return; }
    if ((el = on("[data-dim]"))) {
      if (el.dataset.dim === "custom") { ui.dimCustom = true; render(); document.getElementById("dim-value")?.focus(); return; }
      ui.dimCustom = false;
      set("oled.dim_after_secs", Number(el.dataset.dim));
      return;
    }
    if ((el = on("[data-chord]"))) {
      const path = el.dataset.chord;
      ui.capture = { path, index: Number(el.dataset.index), mods: !Array.isArray(get(path)) };
      capture(!ui.capture.mods);
      render();
      return;
    }
    if ((el = on("[data-chord-drop]"))) { set(el.dataset.chordDrop, get(el.dataset.chordDrop).slice(0, -1)); return; }
    if ((el = on("[data-rule-new]"))) {
      ui.ruleForm = ruleDraft();
      ui.refused = null;
      ui.rulePicking = false;
      if (el.dataset.ruleNew === "pick") askWindows();
      render();
      document.getElementById("rule-exe")?.focus();
      return;
    }
    if ((el = on("[data-rule-edit]"))) {
      const i = Number(el.dataset.ruleEdit);
      ui.ruleForm = ruleDraft((cfg().rules.rule || [])[i], i);
      ui.refused = null;
      ui.rulePicking = false;
      render();
      document.getElementById("rule-exe")?.focus();
      return;
    }
    if (on("[data-rule-pick]")) { askWindows(); render(); return; }
    if ((el = on("[data-rule-win]"))) {
      const w = (ui.ruleWindows || [])[Number(el.dataset.ruleWin)];
      if (w) Object.assign(ui.ruleForm, { exe: w.exe || "", class: w.class || "", title: w.title || "" });
      ui.rulePicking = false;
      render();
      return;
    }
    if ((el = on("[data-rule-flag]"))) { ui.ruleForm[el.dataset.ruleFlag] = !ui.ruleForm[el.dataset.ruleFlag]; render(); return; }
    if (on("[data-rule-cancel]")) { ui.ruleForm = null; ui.rulePicking = false; ui.refused = null; render(); return; }
    if (on("[data-rule-save]")) {
      const f = ui.ruleForm;
      if (Object.keys(ruleProblems(f)).length) return;
      const rules = [...(cfg().rules.rule || [])];
      if (f.index == null) rules.push(ruleOf(f));
      else rules[f.index] = ruleOf(f);
      // The editor stays open until the rule is saved: a refusal is shown in it.
      set("rules.rule", rules);
      return;
    }
    if ((el = on("[data-rule-drop]"))) {
      const left = (cfg().rules.rule || []).filter((_, i) => i !== Number(el.dataset.ruleDrop));
      set("rules.rule", left.length ? left : null);
      return;
    }
    // A click elsewhere ends a shortcut capture.
    if (ui.capture) { ui.capture = null; capture(false); render(); }
  });
  document.addEventListener("input", (e) => {
    const el = e.target;
    // Other words, other settings found: the list from its top.
    if (el.id === "set-search") { ui.search = el.value; render(); document.querySelector(".set-main").scrollTop = 0; return; }
    if (el.dataset.range) {
      const unit = el.dataset.unit;
      document.querySelector(`[data-v="${el.id}"]`).textContent = unit === undefined ? tv("setPx", el.value) : el.value + unit;
    }
    if (el.id === "dim-value" || el.id === "dim-unit") readDim();
    if (el.id === "gen-prompt") {
      ui.prompt = el.value;
      const run = document.querySelector("[data-genrun]");
      if (run) run.disabled = !(genConnected(cfg().create.provider) && el.value.trim());
    }
    if (el.id === "gen-key") {
      const save = document.querySelector("[data-genkeysave]");
      if (save) save.disabled = !el.value.trim();
    }
    if (el.dataset.ruleField && ui.ruleForm) {
      ui.ruleForm[el.dataset.ruleField] = el.value;
      checkRule();
    }
  });
  document.addEventListener("change", (e) => {
    const el = e.target;
    if (el.dataset.range) set(el.dataset.range, Number(el.value));
    if (el.id === "dim-value" || el.id === "dim-unit") { const sec = readDim(); if (sec != null) set("oled.dim_after_secs", sec); }
    if (el.id === "bar-color") set("bar.color", el.value.toUpperCase());
    if (el.dataset.select) set(el.dataset.select, el.value);
    if (el.dataset.saverCorner !== undefined) {
      // One corner starts the screen saver: the one chosen takes it from any other.
      const c = cfg().hot_corners;
      CORNERS.filter(([k]) => k !== el.value && c[k] === "screen_saver").forEach(([k]) => set("hot_corners." + k, "none"));
      if (el.value !== "none") set("hot_corners." + el.value, "screen_saver");
    }
    if (el.dataset.text) {
      // Another address: what Ollama said at the old one no longer holds.
      if (el.dataset.text === "create.ollama_url") ui.ollama = null;
      set(el.dataset.text, el.value.trim());
    }
  });

  /* ---------- messages from Rust ---------- */
  webview.addEventListener("message", (e) => {
    const m = JSON.parse(e.data);
    switch (m.op) {
      case "state":
        searchIndex = null;
        // After a change only what it can have changed comes (PLAN M19 P18).
        S = m.partial && S ? { ...S, ...m } : m;
        if (m.theme_imported != null) ui.themeMsg = tv("thImported", m.theme_imported);
        if (m.settings_done) {
          const said = { imported: "setFileImported", reset: "setResetDone", undone: "setFileUndone" }[m.settings_done.done];
          ui.fileMsg = { text: tv(said, m.settings_done.backup), undo: m.settings_done.done !== "undone" };
        }
        A = A || window.kabukDesignArt(t, escapeHtml);
        ui.refused = null;
        // A rule saved (or removed): its editor closes.
        if (m.saved === "rules.rule") { ui.ruleForm = null; ui.rulePicking = false; }
        if (m.applied_wall) {
          ui.gen = { ...ui.gen, phase: "idle" };
          ui.wallTab = "pick";
          chooseWall("generated", null, m.applied_wall);
        }
        render();
        break;
      case "gen-connected":
        ui.ollama = { ok: m.ok, model: m.model || "" };
        render();
        break;
      case "gen-started":
      case "capturing":
        break;
      case "chord":
        onChord(m.chord);
        break;
      case "gen-step": {
        const g = ui.gen;
        if (m.step === "asking") { g.attempt = m.attempt; g.step = 0; g.ms = null; g.average = null; }
        else if (m.step === "wrote") g.step = 1;
        else if (m.step === "compiled") g.step = 2;
        else if (m.step === "performance") { g.step = 3; g.ms = m.ms; }
        else if (m.step === "oled") { g.step = 4; g.average = m.average; }
        else if (m.step === "refused") g.reason = m.reason;
        render();
        break;
      }
      case "gen-done":
        ui.gen = { ...ui.gen, phase: "done", step: 4, name: m.name, preview: m.preview, ms: m.ms, average: m.average, reason: "" };
        render();
        break;
      case "gen-failed":
        ui.gen = { ...ui.gen, phase: "failed", reason: m.reason };
        render();
        break;
      case "refused":
        ui.refused = { path: m.path, reason: m.reason, at: m.at };
        if (String(m.path).startsWith("gen-") && ui.gen.phase === "running") ui.gen = { ...ui.gen, phase: "idle" };
        render();
        break;
      case "info":
        if (S) { S.info = m.info; render(); }
        break;
      case "update":
        // "opened" only says the browser was asked; what was found stays shown.
        if (m.state !== "opened") { ui.update = m; render(); }
        break;
      case "windows":
        ui.ruleWindows = m.windows;
        render();
        break;
      case "gallery-preview":
        ui.previews[m.key] = m.url;
        document.querySelectorAll("img[data-gprev]").forEach((img) => { if (img.dataset.gprev === m.key) { img.src = m.url; previewCame(img); } });
        break;
      case "gallery-previews-done":
        ui.previewsDone = true;
        document.querySelectorAll("img[data-gprev]").forEach(previewCame);
        break;
      case "profile":
        ui.profileMsg = m.done === "empty" ? t("pfEmpty") : m.done === "not-running" ? t("pfNotRunning") : t("pfOpened");
        render();
        break;
      case "settings":
        ui.fileMsg = m.done === "exported" ? { text: tv("setFileExported", m.path) }
          : m.done === "failed" ? { text: m.reason, failed: true }
          : m.done === "refused" ? { path: m.path, problems: m.problems }
          : null;
        render();
        break;
      case "theme":
        // A file that could not be read or written says why (PLAN M19 U20).
        ui.themeMsg = m.done === "exported" ? tv("thExported", m.path) : m.done === "failed" ? m.reason || t("thFailed") : "";
        render();
        break;
      case "picked":
        if (!m.path) break;
        if (ui.pending === "wall") chooseWall("image", m.path);
        else if (ui.pending === "folder") set("launcher.indexed_folders", [...cfg().launcher.indexed_folders, m.path]);
        ui.pending = null;
        break;
    }
  });

  fetch("sprite.svg").then((r) => r.text()).then((svg) => { document.getElementById("sprite").innerHTML = svg; });
  post({ op: "load" });
})();
