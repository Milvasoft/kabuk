# Kabuk: test rules

Kabuk runs on the user's everyday desktop; a bug loses windows or leaves the machine unusable. That is why testing is not optional. A task is not considered done until its tests are written and pass.

## 1. Mandatory rules

1. **Every behavior has a test.** A new feature, setting or module comes with at least one automated test. The test verifies the behavior through the result the user will see (e.g. "after switching to workspace 2, workspace 1's windows are hidden and in the state file"), not through an internal implementation detail.
2. **Every bug is first caught by a test.** When a bug is found, first a test that reproduces it and **fails** is written, then the bug is fixed and the test passes. The test stays permanently.
3. **Pure crates: test first, then code.** For `kabuk-layout`, `kabuk-theme`, `kabuk-search`, `kabuk-i18n` and `kabuk-core`, tests describing the expected behavior are written first.
4. **Edge cases are listed explicitly.** At the top of every test file, a comment states which edge cases of that unit are tested (zero windows, one window, many windows, minimum size that doesn't fit, zero-area monitor, monitor with negative coordinates, mixed DPI …).
5. **Tests are not skipped or loosened.** Marking a failing test `#[ignore]`, changing the expected value to match the actual result or widening a tolerance is not done without reporting the reason to the user in writing and getting approval.
6. **No flaky tests.** Time-dependent tests are written with a fake clock (the `Clock` trait in `kabuk-core`); no test waits with `sleep`. Windows integration tests are written with a condition-waiting helper (timeout + polling).
7. **Warning-free build.** `cargo clippy --all-targets --all-features -- -D warnings` and `cargo fmt --check` are part of the tests.
8. **No test may disrupt the user's session.** Integration tests manage only windows opened by `kabuk-testapp` (in test mode the WM is restricted to windows with the class name `KabukTestWindow`), do not hide the taskbar, and write to a temporary directory instead of the real `kabuk.toml`. Every test does its own cleanup in `Drop`; windows are shown even on panic.

## 2. Test layers

| Layer | Where | When it runs | Tool |
| --- | --- | --- | --- |
| Unit | Each crate's `#[cfg(test)]` modules | Every `cargo test` | std test |
| Property | `kabuk-layout`, `kabuk-theme`, `kabuk-search` | Every `cargo test` | `proptest` |
| Snapshot | Configuration parse errors, translation outputs, layout results | Every `cargo test` | `insta` |
| Windows integration | `apps/kabuk/tests/` | `cargo test --features integration` (Windows only, also in CI) | `kabuk-testapp` |
| End-to-end scenario | `apps/kabuk/tests/e2e/` | At the end of each milestone and before a release | driven by the real `kabuk.exe` + IPC, result verified with `query state` |
| Visual and contrast | `tools/` | When the theme or drawing changes | Rendering to WIC + design page screenshot; contrast audit |
| Performance | `apps/kabuk/tests/perf` | At the end of each milestone | 10 min idle CPU/RAM measurement, fails if a limit is exceeded |
| Manual compatibility | `docs/compat.md` | Before a release | List of problematic apps (ARCHITECTURE §13) |

## 3. Mandatory property tests

**Layout (`kabuk-layout`)**, for random monitor sizes, gaps and 0-20 windows:
- No two tiled windows overlap.
- Every tiled window is inside the work area.
- Gaps between windows and at the edges equal the configured gap (±1 px rounding).
- Add a window, then remove it: the layout returns to its initial state.
- Floating windows do not affect the layout of tiled windows.
- A window whose minimum size doesn't fit is marked floating, and the remaining layout stays valid.

**Contrast (`kabuk-theme`)**, for random sets of colors and backgrounds:
- `fitContrast` output is ≥ the target on every background (after rounding to hex).
- Hue (H) is preserved (±1° for non-gray colors).
- A color that already passes is returned unchanged.
- `fillPair`: text on the fill ≥ 4.5:1.
- Sample inputs taken from the design page (7 looks × 2 modes × 8 accents) give the same hex value with the native engine. The expected values are generated from the design page with the script under `tools/` and stored as `tests/fixtures/contrast.json`.

**Search (`kabuk-search`)**:
- Turkish letters: "EKRANI" → "Ekranı kilitle"; "istanbul" → "İstanbul"; "ılık" ≠ "ilik".
- Matched letter positions are correct (for highlighting).
- Calculator: `12*8`, `(3+4)/2`, `2^10`, `9,8+1` (TR decimal), an invalid expression gives no result.
- Ranking is stable: alphabetical on equal score.

**Configuration (`kabuk-core`)**:
- The full sample file is parsed, written, parsed again: equal (round-trip).
- Every invalid value (out-of-range dimming timeout, unknown look, malformed color) gives a clear error with line/column; the last valid configuration is kept.
- An unknown key gives a warning, not an error.
- An old schema version is migrated to the new version.

**Translation (`kabuk-i18n`)**:
- The key sets of `tr.ftl` and `en.ftl` are equal.
- Every key used in the code exists in the file (scanned at compile time or in a test).
- Date/time/decimal formats: TR "Pzt 12 Eki", "9,8", "ÖÖ/ÖS"; EN "Mon 12 Oct", "9.8", "AM/PM".

## 4. Mandatory Windows integration tests

Each with `kabuk-testapp` windows:

- A window is tiled when it opens; when it closes, the layout is recalculated.
- Workspace switch: the old workspace's windows are hidden and in the state file; on switching back they are visible and in the same place.
- **Crash:** `kabuk.exe` is force-terminated → the watchdog shows all test windows within 2 s.
- **Hang:** the shell's window manager stops answering → the watchdog ends the shell, shows the test windows and starts a new shell (DECISIONS D55).
- `kabuk --restore` shows the windows in every state and returns exit code 0.
- A window that enforces a minimum size becomes floating, the others are tiled properly.
- With a hung window present (message loop blocked), moving other windows and shortcuts work within 100 ms.
- Owned dialogs and splash screens are not tiled.
- Drag-and-drop floating: a window moved by its title bar becomes floating; a click without movement does not float it.
- When the configuration file changes, it is applied within 500 ms (a gap change is seen in the windows' actual positions).
- When a fullscreen test window opens, the game mode state appears in `query state` within 1 s; when it closes, the previous layout returns.
- Monitor change simulation: a monitor remove/add event is fed to the layout engine, no windows are lost.
- Background: the animated background stops when a test window fully covers the desktop (the frame counter does not increase) and resumes when the window goes away; when Kabuk exits, the previous wallpaper comes back.
- Create validation pipeline: sample shaders that don't compile, are slow, are too bright or have a fixed bright region are rejected for the right reason; the API key never appears in plain text in any file or log.
- Clean exit: when Kabuk exits, the test windows' DWM border color returns to the default and their visibility to visible.

## 5. Coverage and quality targets

- Line coverage in pure crates ≥ 90% (`cargo llvm-cov`); ≥ 95% for `kabuk-layout` and `kabuk-theme`.
- At the end of each milestone: all tests green, clippy clean, performance test within limits, compatibility list up to date.
- The test count and coverage ratio are reported to the user in the milestone summary.

## 6. Commands

```powershell
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace
cargo test -p kabuk --features integration -- --test-threads=1
cargo llvm-cov --workspace --html
node tools/contrast-audit.js docs/design/kabuk-tasarim.html
```

All of them in one command: `tools/check.ps1` (written in M0). This script must pass cleanly before a task is finished.

### 6.1 The test VM

The Windows integration tests take the tray, hide the taskbar, open windows and reserve screen space for a few seconds. On the user's own desktop that disturbs a full-screen game or video, so they can run in a Hyper-V VM instead (approved by the user on 2026-10-13):

- `tools/vm/new-vm.ps1` creates the VM `kabuk-test` once: Windows 11 Enterprise evaluation installed unattended (`tools/vm/autounattend.xml`), a local account that logs on automatically, no network (nothing updates during a run), COM1 on the host pipe `\.\pipe\kabuk-vm-com1`. Needs the user in the "Hyper-V Administrators" group; the guest's password stays in `E:\kabuk-vm\guest-password.txt`, never in the repository.
- `tools/vm/agent.ps1` runs in the VM's desktop session at logon and takes jobs over COM1.
- `tools/vm/run.ps1 [-Test bar,tray] [-Filter name] [-Wait]` builds on the host, copies the programs and test binaries into the VM at the same paths (`Copy-VMFile`), runs them there and prints their output. Its exit code is the tests'.
- `tools/vm/screen.ps1 <out.png>` saves what the VM's screen shows, for visual checks.

A change is done when `tools/check.ps1` passes on the host (its integration step may run on the host or be replaced by `tools/vm/run.ps1` while the user's desktop is busy; say which).
