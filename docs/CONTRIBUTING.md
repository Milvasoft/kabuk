# Contributing to Kabuk

Kabuk changes how Windows' desktop behaves while it runs, on a machine someone uses every day. These rules keep that safe and keep the code in one shape.

| File | Contents |
| --- | --- |
| `docs/SPEC.md` | Product: what it does, looks, settings, acceptance criteria. **The source of truth.** |
| `docs/ARCHITECTURE.md` | Technical design: processes, crates, the Win32 approach for each component, risks. |
| `docs/PLAN.md` | Milestones, their order and the "done" criteria for each. |
| `docs/TESTING.md` | Test rules, mandatory tests, coverage targets. |
| `docs/DECISIONS.md` | Decisions made and decisions still open. |

## Environment

- Windows 11. Win32 code cannot be tested in a cloud or Linux environment.
- Rust stable (`x86_64-pc-windows-msvc`), Visual Studio Build Tools (C++ workload + Windows SDK), WebView2 Runtime (part of Windows 11), Node.js (only for the design tools and the settings page test).

## Safety rules: never leave the machine locked up

1. Nothing hides windows, hides the taskbar or changes the work area unless the watchdog and the emergency restore work.
2. The emergency exit always works: `kabuk --restore` and `Ctrl+Alt+Shift+End` show every hidden window, bring the taskbar back and reset the work area.
3. The handle of every window Kabuk hides is written to the state file (`%LOCALAPPDATA%\kabuk\state.json`) **before** it is hidden, so it comes back after a crash.
4. No code is injected into other processes and no global DLL hook is installed. `SetWinEventHook` is used only with `WINEVENT_OUTOFCONTEXT`. Games with anti-cheat must not break.
5. Every registry value Kabuk changes is backed up first and written back on exit.

## Code rules

- Rust 2021, `cargo fmt` and `cargo clippy --all-targets -- -D warnings` clean. `unsafe` blocks are small and each has a `// SAFETY:` comment above it saying why it is safe.
- Win32 calls live behind safe wrappers in `kabuk-win`; application code does not call the raw `windows` crate.
- Platform-independent logic (layout, configuration, search, contrast, translation) lives in its own crates as pure functions with unit tests.
- Code, comments, logs, commit messages and documentation are in English. Turkish appears only in `locales/tr.ftl` and in quoted Turkish UI or test examples. Every user-visible string comes from `locales/`; no UI text is hard-coded.
- No magic numbers: sizes, durations and colours are theme or configuration values.
- Before inventing a new pattern, look for one in the codebase that does the same job and follow its form.
- Idle CPU stays close to zero: nothing draws on a timer, only when something changes. The clock wakes on the minute boundary.

## Done means

1. Tests are written and `tools/check.ps1` passes (fmt, clippy, all tests, the Windows integration tests). The rules in `docs/TESTING.md` are mandatory: every behaviour has a test, every bug is first caught by a failing test, no test is skipped, no expected value is changed to match the actual result.
2. The changed behaviour was tried by running it; anything visual was compared with the design.
3. The milestone's "done" criteria in `docs/PLAN.md` are checked off.
4. A new setting is wired end to end: schema → reading → applying → Settings → translations (tr and en) → live reload.
5. Anything left undone is written down, not skipped silently.

Commits are small and single-topic, with messages in English in the imperative mood.
