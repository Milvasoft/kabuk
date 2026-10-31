//! Configuration parsing tests.
//!
//! Edge cases: empty document, full example, defaults round trip, random valid configs
//! round trip, syntax error location, wrong types, unknown enum words, every range
//! bound (just inside / just outside), bad colours, bad and conflicting shortcuts
//! (including workspace-digit expansion), regex errors, incomplete rules, duplicate
//! monitors, missing dependent values, unknown keys (top level, nested, array of
//! tables), schema missing / too new / not an integer / older (migrated), and a bad
//! reload keeping the last valid configuration.

use super::*;
use proptest::prelude::*;

const EXAMPLE: &str = include_str!("../../../../config/kabuk.example.toml");

fn errors(src: &str) -> Vec<ConfigError> {
    match parse(src) {
        Ok(l) => panic!("expected errors, got {:?}", l.config),
        Err(e) => e,
    }
}

fn single_error(src: &str) -> ConfigError {
    let mut e = errors(src);
    assert_eq!(e.len(), 1, "{e:#?}");
    e.remove(0)
}

fn path(p: &str) -> Option<KeyPath> {
    // Accepts "a.b[2].c".
    let mut segs = Vec::new();
    for part in p.split('.') {
        let (key, rest) = part.split_once('[').map_or((part, None), |(k, r)| (k, Some(r)));
        segs.push(Segment::Key(key.to_string()));
        if let Some(r) = rest {
            segs.push(Segment::Index(r.trim_end_matches(']').parse().unwrap()));
        }
    }
    Some(KeyPath(segs))
}

fn at(line: usize, column: usize) -> Option<Location> {
    Some(Location { line, column })
}

#[test]
fn empty_document_gives_defaults_and_warns_about_schema() {
    let loaded = parse("").unwrap();
    assert_eq!(loaded.config, Config::default());
    assert_eq!(loaded.warnings.len(), 1);
    assert_eq!(loaded.warnings[0].kind, WarningKind::MissingSchema);
}

#[test]
fn defaults_follow_the_spec() {
    let c = Config::default();
    // Rıhtım since 2026-10-14 (asked for by the user).
    assert_eq!(c.appearance.look, Look::Rihtim);
    assert_eq!(c.appearance.mode, Mode::Dark);
    assert_eq!(c.windows.tiling_layout, TilingLayout::Dwindle);
    assert_eq!(c.windows.workspaces, 9);
    assert_eq!(c.windows.shown_workspaces, 4);
    assert_eq!(c.windows.gap, None);
    assert_eq!(c.oled.dim_after_secs, 60);
    assert_eq!(c.screensaver.wait_minutes, 5);
    assert_eq!(c.wallpaper.brightness, 60);
    assert_eq!(c.shortcuts.launcher, vec!["Win+R".to_string()]);
    assert_eq!(c.bar.position.edge(Look::Ray), Edge::Left);
    assert_eq!(c.bar.position.edge(Look::Konsol), Edge::Bottom);
    assert_eq!(c.bar.position.edge(Look::Rihtim), Edge::Top);
    // Pano is a bottom panel (the design's `lookPos`); Izgara and Defter sit on top.
    assert_eq!(c.bar.position.edge(Look::Pano), Edge::Bottom);
    assert_eq!(c.bar.position.edge(Look::Izgara), Edge::Top);
    assert_eq!(c.bar.position.edge(Look::Defter), Edge::Top);
    assert_eq!(BarPosition::Right.edge(Look::Ray), Edge::Right);
    // The design's looks but Piksel (`k`), which the user did not want (2026-10-15).
    let codes: Vec<&str> = Look::ALL.iter().map(|l| l.code()).collect();
    assert_eq!(codes, vec!["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
}

/// DECISIONS D68: touchpad swipes are off by default, three fingers, as macOS; four
/// fingers may be chosen, two or five may not.
#[test]
fn gestures_default_off_with_three_fingers() {
    let g = Config::default().gestures;
    assert!(!g.enabled);
    assert_eq!(g.fingers, 3);
    assert_eq!(
        (g.swipe_left, g.swipe_right, g.swipe_up, g.swipe_down),
        (
            GestureAction::NextWorkspace,
            GestureAction::PreviousWorkspace,
            GestureAction::Overview,
            GestureAction::AppGrid
        )
    );
    let four = parse("schema = 1\n[gestures]\nenabled = true\nfingers = 4\nswipe_up = \"launcher\"\n").unwrap();
    assert_eq!(
        (four.config.gestures.fingers, four.config.gestures.swipe_up),
        (4, GestureAction::Launcher)
    );
    for wrong in [2, 5] {
        assert!(
            parse(&format!("schema = 1\n[gestures]\nfingers = {wrong}\n")).is_err(),
            "{wrong} fingers"
        );
    }
}

/// The new looks are written by their ASCII names, like the others.
#[test]
fn new_looks_parse_by_name() {
    for (name, look) in [("pano", Look::Pano), ("izgara", Look::Izgara), ("defter", Look::Defter)] {
        let loaded = parse(&format!("schema = 1\n[appearance]\nlook = \"{name}\"\n")).unwrap();
        assert_eq!(loaded.config.appearance.look, look);
        assert!(loaded.warnings.is_empty(), "{name}: {:?}", loaded.warnings);
    }
    assert!(parse("schema = 1\n[appearance]\nlook = \"piksel\"\n").is_err());
}

#[test]
fn example_file_parses_without_warnings() {
    let loaded = parse(EXAMPLE).unwrap();
    assert_eq!(loaded.warnings, vec![]);
    assert_eq!(loaded.config.rules.user.len(), 3);
    assert_eq!(loaded.config.wallpaper.monitors[0].index, 2);
    assert_eq!(loaded.config.windows.gap, Some(14));
}

#[test]
fn example_file_round_trips() {
    let first = parse(EXAMPLE).unwrap().config;
    let written = to_toml(&first);
    let second = parse(&written).unwrap();
    assert_eq!(second.config, first);
    assert_eq!(second.warnings, vec![]);
}

#[test]
fn defaults_round_trip() {
    let written = to_toml(&Config::default());
    assert_eq!(parse(&written).unwrap().config, Config::default());
}

#[test]
fn file_round_trip_is_atomic_and_lossless() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("kabuk.toml");
    let config = parse(EXAMPLE).unwrap().config;
    write_file(&file, &config).unwrap();
    assert_eq!(load_file(&file).unwrap().config, config);
    // Only the target file remains; no temporary files are left behind.
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn missing_file_gives_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load_file(&dir.path().join("absent.toml")).unwrap();
    assert_eq!(loaded.config, Config::default());
}

#[test]
fn syntax_error_has_line_and_column() {
    let e = single_error("schema = 1\n[bar]\nautohide = = true\n");
    assert!(matches!(e.problem, Problem::Syntax(_)), "{e:?}");
    assert_eq!(e.location, at(3, 12));
}

#[test]
fn unknown_enum_word_is_located() {
    let e = single_error("schema = 1\n[appearance]\nlook = \"nope\"\n");
    assert!(matches!(e.problem, Problem::Invalid(_)), "{e:?}");
    assert_eq!(e.path, path("appearance.look"));
    assert_eq!(e.location, at(3, 8));
}

#[test]
fn wrong_type_is_located() {
    let e = single_error("schema = 1\n[oled]\nidle_dim = \"yes\"\n");
    assert!(matches!(e.problem, Problem::Invalid(_)), "{e:?}");
    assert_eq!(e.path, path("oled.idle_dim"));
    assert_eq!(e.location, at(3, 12));
}

#[test]
fn dim_after_bounds() {
    for ok in [10, 60, 3600] {
        assert!(
            parse(&format!("schema = 1\n[oled]\ndim_after_secs = {ok}\n")).is_ok(),
            "{ok}"
        );
    }
    for bad in [0, 9, 3601] {
        let e = single_error(&format!("schema = 1\n[oled]\ndim_after_secs = {bad}\n"));
        assert_eq!(e.problem, Problem::OutOfRange { min: 10, max: 3600 }, "{bad}");
        assert_eq!(e.path, path("oled.dim_after_secs"));
        assert_eq!(e.location, at(3, 18));
    }
}

#[test]
fn negative_number_for_unsigned_is_rejected() {
    let e = single_error("schema = 1\n[oled]\ndim_after_secs = -5\n");
    assert!(matches!(e.problem, Problem::Invalid(_)), "{e:?}");
    assert_eq!(e.path, path("oled.dim_after_secs"));
}

/// A change in the configuration's folder that leaves the file as it was (Kabuk's own
/// state file next to it, an editor's temporary file) is not a reload; an edit, a
/// deletion and the file coming back are.
#[test]
fn only_a_changed_file_is_a_reload() {
    const FIRST: &str = "schema = 1\n";
    const EDITED: &str = "schema = 1\n[bar]\nautohide = true\n";
    let mut seen = FileSeen::new(Some(FIRST.into()));
    assert!(!seen.changed(Some(FIRST.into())));
    assert!(seen.changed(Some(EDITED.into())));
    assert!(!seen.changed(Some(EDITED.into())));
    assert!(seen.changed(None));
    assert!(!seen.changed(None));
    assert!(seen.changed(Some(String::new())));
    // Started without a file.
    assert!(!FileSeen::new(None).changed(None));
}

#[test]
fn other_ranges() {
    let cases = [
        (
            "[bar]\ntray_shown = 13",
            "bar.tray_shown",
            Problem::OutOfRange { min: 0, max: 12 },
        ),
        (
            "[bar]\nheight = 23",
            "bar.height",
            Problem::OutOfRange { min: 24, max: 72 },
        ),
        (
            "[bar]\nheight = 73",
            "bar.height",
            Problem::OutOfRange { min: 24, max: 72 },
        ),
        (
            "[windows]\ngap = 33",
            "windows.gap",
            Problem::OutOfRange { min: 0, max: 32 },
        ),
        (
            "[windows]\nworkspaces = 0",
            "windows.workspaces",
            Problem::OutOfRange { min: 1, max: 9 },
        ),
        (
            "[windows]\nworkspaces = 10",
            "windows.workspaces",
            Problem::OutOfRange { min: 1, max: 9 },
        ),
        (
            "[windows]\nshown_workspaces = 10",
            "windows.shown_workspaces",
            Problem::OutOfRange { min: 1, max: 9 },
        ),
        (
            "[wallpaper]\nbrightness = 9",
            "wallpaper.brightness",
            Problem::OutOfRange { min: 10, max: 100 },
        ),
        (
            "[wallpaper]\nbrightness = 101",
            "wallpaper.brightness",
            Problem::OutOfRange { min: 10, max: 100 },
        ),
        (
            "[screensaver]\nwait_minutes = 4",
            "screensaver.wait_minutes",
            Problem::NotOneOf(vec![3, 5, 10, 15]),
        ),
    ];
    for (body, p, problem) in cases {
        let e = single_error(&format!("schema = 1\n{body}\n"));
        assert_eq!(e.problem, problem, "{body}");
        assert_eq!(e.path, path(p), "{body}");
        assert_eq!(e.location.map(|l| l.line), Some(3), "{body}");
    }
    assert!(parse("schema = 1\n[windows]\ngap = 0\n").is_ok());
    assert!(parse("schema = 1\n[windows]\ngap = 32\n").is_ok());
}

#[test]
fn shown_workspaces_may_exceed_workspaces() {
    assert!(parse("schema = 1\n[windows]\nworkspaces = 3\nshown_workspaces = 4\n").is_ok());
}

#[test]
fn an_invalid_workspace_count_is_reported_once() {
    let e = single_error("schema = 1\n[windows]\nworkspaces = 0\n[[rules.rule]]\nexe = \"a.exe\"\nworkspace = 9\n");
    assert_eq!(e.path, path("windows.workspaces"));
}

#[test]
fn accent_colour() {
    assert!(parse("[appearance]\naccent = \"#8fd3ff\"").is_ok());
    assert!(parse("[appearance]\naccent = \"#8FD3FF\"").is_ok());
    assert!(parse("[appearance]\naccent = \"theme\"").is_ok());
    for bad in ["8fd3ff", "#8fd3f", "#8fd3ffa", "#gggggg", "blue", ""] {
        let e = single_error(&format!("schema = 1\n[appearance]\naccent = \"{bad}\"\n"));
        assert_eq!(e.problem, Problem::InvalidColor(bad.to_string()));
        assert_eq!(e.location, at(3, 10));
    }
    assert_eq!(parse_hex_color("#0a84ff"), Some([0x0a, 0x84, 0xff]));
}

/// DECISIONS D57: the bar's colour is a #rrggbb colour or unset.
#[test]
fn bar_colour() {
    assert!(parse("[bar]\ncolor = \"#1e1433\"").is_ok());
    assert!(parse("[bar]\ntray_shown = 3").is_ok());
    for bad in ["1e1433", "#1e143", "theme", "navy"] {
        let e = single_error(&format!("schema = 1\n[bar]\ncolor = \"{bad}\"\n"));
        assert_eq!(e.problem, Problem::InvalidColor(bad.to_string()));
        assert_eq!(e.path, path("bar.color"));
    }
}

#[test]
fn invalid_shortcut_is_located_by_index() {
    let e = single_error("schema = 1\n[shortcuts]\nfocus_left = [\"Alt+H\", \"Alt+Nope\"]\n");
    assert_eq!(
        e.problem,
        Problem::InvalidShortcut(shortcut::ShortcutError::UnknownKey("Nope".into()))
    );
    assert_eq!(e.path, path("shortcuts.focus_left[1]"));
    assert_eq!(e.location, at(3, 24));
}

#[test]
fn reserved_and_altgr_shortcuts_are_rejected() {
    let e = single_error("schema = 1\n[shortcuts]\nlauncher = [\"Win+L\"]\n");
    assert_eq!(e.problem, Problem::InvalidShortcut(shortcut::ShortcutError::Reserved));
    let e = single_error("schema = 1\n[shortcuts]\nclose_window = [\"Ctrl+Alt+Q\"]\n");
    assert_eq!(
        e.problem,
        Problem::InvalidShortcut(shortcut::ShortcutError::AltGrConflict)
    );
    let e = single_error("schema = 1\n[shortcuts]\nworkspace = \"Ctrl+Alt\"\n");
    assert_eq!(
        e.problem,
        Problem::InvalidShortcut(shortcut::ShortcutError::AltGrConflict)
    );
    assert_eq!(e.path, path("shortcuts.workspace"));
}

#[test]
fn conflicting_shortcuts_name_both_actions() {
    let e = single_error("schema = 1\n[shortcuts]\nlauncher = [\"Win+Shift+Q\"]\n");
    // Reported on the shortcut written in the file, naming the default it collides with.
    assert_eq!(
        e.problem,
        Problem::ShortcutConflict {
            other: path("shortcuts.close_window[0]").unwrap(),
            chord: "Win+Shift+Q".into()
        }
    );
    assert_eq!(e.path, path("shortcuts.launcher[0]"));
    assert_eq!(e.location, at(3, 13));

    // Both written in the file: reported on the later one.
    let e = single_error("schema = 1\n[shortcuts]\nlauncher = [\"Alt+F1\"]\ntoggle_float = [\"Alt+F1\"]\n");
    assert_eq!(e.path, path("shortcuts.toggle_float[0]"));
    assert_eq!(e.location, at(4, 17));
}

#[test]
fn workspace_digits_conflict_with_other_shortcuts() {
    let e = single_error("schema = 1\n[shortcuts]\ncycle_layout = [\"Win+3\"]\n");
    assert!(
        matches!(&e.problem, Problem::ShortcutConflict { chord, .. } if chord == "Win+3"),
        "{e:?}"
    );
    // With only two workspaces, Win+3 is free.
    assert!(parse("schema = 1\n[windows]\nworkspaces = 2\n[shortcuts]\ncycle_layout = [\"Win+3\"]\n").is_ok());
    // Same modifiers for switching and moving.
    let e = errors("schema = 1\n[shortcuts]\nmove_to_workspace = \"Win\"\n");
    assert_eq!(e.len(), 9);
    assert!(e.iter().all(|e| e.path == path("shortcuts.move_to_workspace")));
}

#[test]
fn drag_modifier_must_be_a_modifier() {
    let e = single_error("schema = 1\n[shortcuts]\ndrag = \"Shift\"\n");
    assert_eq!(
        e.problem,
        Problem::InvalidShortcut(shortcut::ShortcutError::NeedsCommandModifier)
    );
}

#[test]
fn rules_are_checked() {
    let src = "schema = 1\n\
        [[rules.rule]]\nexe = \"a.exe\"\nfloat = true\n\
        [[rules.rule]]\nfloat = true\n\
        [[rules.rule]]\nexe = \"b.exe\"\n\
        [[rules.rule]]\ntitle = \"(\"\nregex = true\nfloat = true\n\
        [[rules.rule]]\nexe = \"c.exe\"\nworkspace = 10\nmonitor = 0\nmin_width = 0\n";
    let e = errors(src);
    let got: Vec<(String, &Problem)> = e
        .iter()
        .map(|e| (e.path.clone().unwrap().to_string(), &e.problem))
        .collect();
    assert_eq!(got.len(), 6, "{got:#?}");
    assert_eq!(got[0], ("rules.rule[1]".into(), &Problem::RuleWithoutMatch));
    assert_eq!(got[1], ("rules.rule[2]".into(), &Problem::RuleWithoutAction));
    assert_eq!(got[2].0, "rules.rule[3].title");
    assert!(matches!(got[2].1, Problem::InvalidRegex(_)));
    assert_eq!(
        got[3],
        (
            "rules.rule[4].workspace".into(),
            &Problem::OutOfRange { min: 1, max: 9 }
        )
    );
    assert_eq!(
        got[4],
        ("rules.rule[4].monitor".into(), &Problem::OutOfRange { min: 1, max: 16 })
    );
    assert_eq!(
        got[5],
        (
            "rules.rule[4].min_width".into(),
            &Problem::OutOfRange { min: 1, max: 16384 }
        )
    );
    // The regex error points at the title value: `title = "("` is line 10.
    let regex_error = &e[2];
    assert_eq!(regex_error.location, at(10, 9));
}

#[test]
fn plain_text_rules_do_not_need_valid_regex() {
    assert!(parse("[[rules.rule]]\ntitle = \"(\"\nfloat = true\n").is_ok());
}

#[test]
fn wallpaper_dependencies() {
    let e = single_error("schema = 1\n[wallpaper]\nkind = \"image\"\n");
    assert_eq!((e.path, e.problem), (path("wallpaper.image"), Problem::Missing));
    let e = single_error("schema = 1\n[[wallpaper.monitor]]\nindex = 1\nkind = \"generated\"\n");
    assert_eq!(
        (e.path, e.problem),
        (path("wallpaper.monitor[0].generated"), Problem::Missing)
    );
    let e = single_error(
        "schema = 1\n[[wallpaper.monitor]]\nindex = 2\nkind = \"black\"\n[[wallpaper.monitor]]\nindex = 2\nkind = \"circuit\"\n",
    );
    assert_eq!(
        (e.path, e.problem),
        (path("wallpaper.monitor[1].index"), Problem::Duplicate)
    );
    let e = single_error("schema = 1\n[[wallpaper.monitor]]\nindex = 0\nkind = \"black\"\n");
    assert_eq!(e.problem, Problem::OutOfRange { min: 1, max: 16 });
}

/// DECISIONS D65: a layout profile reads back, and one with no name, nothing to start,
/// a workspace past the last or no monitor is refused where it is wrong.
#[test]
fn layout_profiles() {
    let good = "schema = 1\n[[profile]]\nname = \"Kodlama\"\n[[profile.window]]\nexe = \"Code.exe\"\nopen = 'C:\\Code\\Code.exe'\nmonitor = 2\nworkspace = 3\n";
    let c = parse(good).unwrap().config;
    assert_eq!(c.profiles.len(), 1);
    assert_eq!(
        (c.profiles[0].windows[0].monitor, c.profiles[0].windows[0].workspace),
        (2, 3)
    );
    let e = single_error("schema = 1\n[[profile]]\nname = \" \"\n");
    assert_eq!((e.path, e.problem), (path("profile[0].name"), Problem::Empty));
    let e = single_error(
        "schema = 1\n[[profile]]\nname = \"x\"\n[[profile.window]]\nexe = \"a.exe\"\nopen = \"a.exe\"\nmonitor = 1\nworkspace = 12\n",
    );
    assert_eq!(e.path, path("profile[0].window[0].workspace"));
    let e = single_error(
        "schema = 1\n[[profile]]\nname = \"x\"\n[[profile.window]]\nexe = \"a.exe\"\nopen = \"\"\nmonitor = 1\nworkspace = 1\n",
    );
    assert_eq!((e.path, e.problem), (path("profile[0].window[0].open"), Problem::Empty));
    let e = single_error(
        "schema = 1\n[[profile]]\nname = \"x\"\n[[profile.window]]\nexe = \"a.exe\"\nopen = \"a.exe\"\nworkspace = 1\n",
    );
    assert_eq!(e.path, path("profile[0].window[0].monitor"));
}

#[test]
fn launcher_folders_and_url() {
    let e = single_error("schema = 1\n[launcher]\nindexed_folders = [\"C:\\\\src\", \" \"]\n");
    assert_eq!(
        (e.path, e.problem),
        (path("launcher.indexed_folders[1]"), Problem::Empty)
    );
    for bad in ["localhost:11434", "ftp://x", "http://"] {
        let e = single_error(&format!("schema = 1\n[create]\nollama_url = \"{bad}\"\n"));
        assert_eq!(e.problem, Problem::InvalidUrl(bad.into()));
    }
}

#[test]
fn several_errors_are_reported_together() {
    let e = errors("schema = 1\n[oled]\ndim_after_secs = 1\n[windows]\ngap = 99\n");
    assert_eq!(e.len(), 2);
}

#[test]
fn unknown_keys_warn_but_load() {
    let src = "schema = 1\ncolour = 1\n[bar]\nautohide = true\nwobble = true\n[bar.modules]\nclock = false\nweather = true\n[[rules.rule]]\nexe = \"x.exe\"\nfloat = true\nfloaty = true\n";
    let loaded = parse(src).unwrap();
    assert!(loaded.config.bar.autohide);
    assert!(!loaded.config.bar.modules.clock);
    let got: Vec<(String, Option<Location>)> = loaded
        .warnings
        .iter()
        .map(|w| {
            assert_eq!(w.kind, WarningKind::UnknownKey);
            (w.path.to_string(), w.location)
        })
        .collect();
    assert_eq!(
        got,
        vec![
            ("bar.modules.weather".to_string(), at(8, 1)),
            ("bar.wobble".to_string(), at(5, 1)),
            ("colour".to_string(), at(2, 1)),
            ("rules.rule[0].floaty".to_string(), at(12, 1)),
        ]
    );
}

#[test]
fn schema_version_checks() {
    let e = single_error("schema = 2\n");
    assert_eq!(e.problem, Problem::UnsupportedSchema { found: 2, supported: 1 });
    assert_eq!(e.location, at(1, 10));
    let e = single_error("schema = 0\n");
    assert_eq!(e.problem, Problem::UnsupportedSchema { found: 0, supported: 1 });
    let e = single_error("schema = \"1\"\n");
    assert!(matches!(e.problem, Problem::Invalid(_)));
    assert_eq!(parse("schema = 1\n").unwrap().warnings, vec![]);
}

/// Exercises the migration path with a made-up schema 2 that renames `[oled] dim` to `dim_after_secs`.
#[test]
fn older_schema_is_migrated() {
    fn v1_to_v2(t: &mut toml::Table) {
        if let Some(toml::Value::Table(oled)) = t.get_mut("oled") {
            if let Some(v) = oled.remove("dim") {
                oled.insert("dim_after_secs".into(), v);
            }
        }
    }
    let src = "schema = 1\n[oled]\ndim = 120\n";
    let loaded = parse_with(src, 2, &[v1_to_v2]).unwrap();
    assert_eq!(loaded.config.oled.dim_after_secs, 120);
    assert_eq!(loaded.config.schema, 2);
    assert_eq!(loaded.warnings, vec![]);

    // Validation still runs after migration; errors have a path but no file location.
    let e = parse_with("schema = 1\n[oled]\ndim = 1\n", 2, &[v1_to_v2]).unwrap_err();
    assert_eq!(e[0].path, path("oled.dim_after_secs"));
    assert_eq!(e[0].location, None);
}

#[test]
fn bad_reload_keeps_last_valid_config() {
    let mut live = LiveConfig::new(Config::default());
    assert!(matches!(
        live.reload("schema = 1\n[windows]\ngap = 4\n"),
        Reload::Applied { .. }
    ));
    assert_eq!(live.current().windows.gap, Some(4));
    let r = live.reload("schema = 1\n[windows]\ngap = 400\n");
    assert!(matches!(r, Reload::Rejected { ref errors } if errors.len() == 1));
    assert_eq!(live.current().windows.gap, Some(4));
    assert!(matches!(live.reload("[windows\n"), Reload::Rejected { .. }));
    assert_eq!(live.current().windows.gap, Some(4));
}

#[test]
fn error_messages_for_logs() {
    let src = "schema = 1\n[oled]\ndim_after_secs = 5\n[shortcuts]\nlauncher = [\"Win+Shift+Q\"]\n[appearance]\naccent = \"#12\"\nwobble = 1\n";
    let e = errors(src);
    let text: Vec<String> = e.iter().map(ToString::to_string).collect();
    insta::assert_snapshot!(text.join("\n"));
}

#[test]
fn location_from_offset_counts_characters() {
    let src = "a = \"ığü\"\nb = 1";
    assert_eq!(Location::from_offset(src, 0), Location { line: 1, column: 1 });
    let b = src.find('b').unwrap();
    assert_eq!(Location::from_offset(src, b), Location { line: 2, column: 1 });
    let quote = src.rfind('"').unwrap();
    assert_eq!(Location::from_offset(src, quote), Location { line: 1, column: 9 });
    assert_eq!(Location::from_offset(src, 10_000), Location { line: 2, column: 6 });
}

fn arb_config() -> impl Strategy<Value = Config> {
    (
        proptest::sample::select(Look::ALL.to_vec()),
        proptest::option::of(0u32..=32),
        10u32..=3600,
        10u32..=100,
        proptest::sample::select(SAVER_WAIT_CHOICES.to_vec()),
        proptest::array::uniform12(any::<bool>()),
        proptest::option::of(proptest::sample::select(ACCENT_SWATCHES.to_vec())),
        1u32..=9,
    )
        .prop_map(|(look, gap, dim, bright, wait, m, accent, ws)| {
            let mut c = Config::default();
            c.appearance.look = look;
            c.appearance.accent = accent.map_or(THEME_ACCENT.to_string(), str::to_string);
            c.windows.gap = gap;
            c.windows.workspaces = ws;
            c.windows.shown_workspaces = ws.min(4);
            c.oled.dim_after_secs = dim;
            c.wallpaper.brightness = bright;
            c.screensaver.wait_minutes = wait;
            let mods = &mut c.bar.modules;
            [mods.title, mods.clock, mods.date, mods.media, mods.cpu, mods.ram] = [m[0], m[1], m[2], m[3], m[4], m[5]];
            [mods.gpu, mods.net, mods.vol, mods.tray, mods.notif, mods.power] = [m[6], m[7], m[8], m[9], m[10], m[11]];
            c
        })
}

proptest! {
    #[test]
    fn valid_configs_round_trip(c in arb_config()) {
        let loaded = parse(&to_toml(&c)).unwrap();
        prop_assert_eq!(loaded.config, c);
        prop_assert_eq!(loaded.warnings, vec![]);
    }
}

#[test]
fn every_problem_and_warning_has_a_log_line() {
    let p = path("a.b").unwrap();
    let problems = [
        Problem::Syntax("x".into()),
        Problem::Invalid("x".into()),
        Problem::UnsupportedSchema { found: 3, supported: 1 },
        Problem::OutOfRange { min: 1, max: 2 },
        Problem::NotOneOf(vec![3, 5]),
        Problem::InvalidColor("#1".into()),
        Problem::InvalidShortcut(shortcut::ShortcutError::Empty),
        Problem::ShortcutConflict {
            other: p.clone(),
            chord: "Alt+Q".into(),
        },
        Problem::InvalidRegex("x".into()),
        Problem::InvalidUrl("x".into()),
        Problem::RuleWithoutMatch,
        Problem::RuleWithoutAction,
        Problem::Missing,
        Problem::Empty,
        Problem::Duplicate,
    ];
    let mut lines = std::collections::BTreeSet::new();
    for problem in problems {
        let e = ConfigError {
            path: Some(p.clone()),
            location: at(2, 3),
            problem,
        };
        let line = e.to_string();
        assert!(line.starts_with("line 2, column 3: a.b: "), "{line}");
        assert!(lines.insert(line), "two problems print the same line");
    }
    let unlocated = ConfigError {
        path: None,
        location: None,
        problem: Problem::Empty,
    };
    assert_eq!(unlocated.to_string(), "value is empty");
    for kind in [WarningKind::UnknownKey, WarningKind::MissingSchema] {
        let w = ConfigWarning {
            path: p.clone(),
            location: at(1, 1),
            kind,
        };
        assert!(w.to_string().starts_with("line 1, column 1: "));
    }
    assert_eq!(
        KeyPath::of(&["rules", "rule"]).index(2).key("exe").to_string(),
        "rules.rule[2].exe"
    );
}

#[test]
fn a_monitors_own_wallpaper_wins_over_the_default() {
    let mut w = Wallpaper::default();
    assert_eq!(w.for_monitor(1).0, WallpaperKind::Windows, "the user's own by default");
    w.kind = WallpaperKind::Circuit;
    w.monitors.push(MonitorWallpaper {
        index: 2,
        kind: WallpaperKind::Image,
        image: "D:/Pictures/night.png".into(),
        generated: String::new(),
        gallery: String::new(),
    });
    assert_eq!(w.for_monitor(1).0, WallpaperKind::Circuit);
    assert_eq!(w.for_monitor(2), (WallpaperKind::Image, "D:/Pictures/night.png".into()));
    assert!(WallpaperKind::Circuit.animated() && WallpaperKind::Constellation.animated());
    assert!(!WallpaperKind::Image.animated() && !WallpaperKind::Windows.animated());
}

/// DECISIONS D58: a gallery design names its category and id; a live one is played, a
/// still one is not; a monitor's own wins; a missing design is reported.
#[test]
fn gallery_designs_are_chosen_per_monitor_and_saver() {
    let c = parse(
        "schema = 1\n[wallpaper]\nkind = \"gallery\"\ngallery = \"wall-live/aurora-shore\"\n[[wallpaper.monitor]]\nindex = 2\nkind = \"gallery\"\ngallery = \"wall-still/agate-slice\"\n[screensaver]\nkind = \"gallery\"\ngallery = \"saver-still/ember-enso\"\n",
    )
    .unwrap()
    .config;
    let w = &c.wallpaper;
    assert_eq!(w.gallery_for(1), "wall-live/aurora-shore");
    assert_eq!(w.gallery_for(2), "wall-still/agate-slice");
    assert!(w.animated_for(1) && !w.animated_for(2));
    assert!(!WallpaperKind::Gallery.animated(), "only a live design moves");
    assert_eq!(c.screensaver.kind, SaverKind::Gallery);
    assert_eq!(c.screensaver.gallery, "saver-still/ember-enso");
    assert!(is_live_design("saver-live/x") && !is_live_design("wall-still/x") && !is_live_design("x"));
    let e = single_error("schema = 1\n[wallpaper]\nkind = \"gallery\"\n");
    assert_eq!((e.path, e.problem), (path("wallpaper.gallery"), Problem::Missing));
    let e = single_error("schema = 1\n[screensaver]\nkind = \"gallery\"\n");
    assert_eq!((e.path, e.problem), (path("screensaver.gallery"), Problem::Missing));
}

/// DECISIONS D41: the cursors are chosen apart from the look; "kabuk" follows the look's
/// own style and colour, "custom" the user's, and each size is one of Windows' own.
#[test]
fn cursors_follow_the_look_or_the_users_pick() {
    let c = parse("schema = 1\n[appearance]\nlook = \"rihtim\"\ncursors = \"kabuk\"\ncursor_style = \"block\"\ncursor_color = \"accent\"\ncursor_size = \"huge\"\n")
        .unwrap()
        .config;
    assert_eq!(c.appearance.cursor_pick(), (CursorStyle::Round, CursorColor::Dark));
    assert_eq!(c.appearance.cursor_size.base(), 64);
    let custom = Appearance {
        cursors: CursorChoice::Custom,
        ..c.appearance.clone()
    };
    assert_eq!(custom.cursor_pick(), (CursorStyle::Block, CursorColor::Accent));
    for look in Look::ALL {
        let a = Appearance {
            look,
            ..Appearance::default()
        };
        assert_eq!(a.cursor_pick(), (CursorStyle::of(look), CursorColor::of(look)));
    }
    assert_eq!(CursorStyle::of(Look::Konsol), CursorStyle::Block);
    assert_eq!(CursorStyle::of(Look::Cizgi), CursorStyle::Slim);
    let d = Appearance::default();
    assert_eq!((d.cursors, d.cursor_size.base()), (CursorChoice::Windows, 32));
    assert_eq!(
        [CursorSize::Normal, CursorSize::Large, CursorSize::Huge].map(CursorSize::base),
        [32, 48, 64]
    );
    let codes: Vec<&str> = CursorStyle::ALL.iter().map(|s| s.code()).collect();
    assert_eq!(codes, ["round", "slim", "circuit", "bold", "block", "cartoon", "comic"]);
    let cartoon = parse("schema = 1\n[appearance]\ncursors = \"custom\"\ncursor_style = \"comic\"\n")
        .unwrap()
        .config;
    assert_eq!(cartoon.appearance.cursor_pick().0, CursorStyle::Comic);
    assert!(errors("schema = 1\n[appearance]\ncursor_size = \"tiny\"\n").len() == 1);
}

/// Asked for by the user: minimising plays the genie unless Windows' own is chosen.
#[test]
fn the_minimize_effect_is_the_genie_unless_windows_is_chosen() {
    assert_eq!(Config::default().windows.minimize_effect, MinimizeEffect::Genie);
    let c = parse("schema = 1\n[windows]\nminimize_effect = \"windows\"\n")
        .unwrap()
        .config;
    assert_eq!(c.windows.minimize_effect, MinimizeEffect::Windows);
    assert_eq!(errors("schema = 1\n[windows]\nminimize_effect = \"poof\"\n").len(), 1);
}

/// D42: the system theme's colour layer is off unless asked for.
#[test]
fn the_system_theme_is_off_unless_asked_for() {
    assert!(!Config::default().system_theme.colors);
    let c = parse("schema = 1\n[system_theme]\ncolors = true\n").unwrap().config;
    assert!(c.system_theme.colors);
    assert_eq!(errors("schema = 1\n[system_theme]\ncolors = 1\n").len(), 1);
}

/// PLAN M19 U20: Kabuk's notices are shown unless held back.
#[test]
fn kabuks_notices_are_shown_unless_held_back() {
    assert!(!Config::default().notices.quiet);
    let c = parse("schema = 1\n[notices]\nquiet = true\n").unwrap().config;
    assert!(c.notices.quiet);
    assert_eq!(errors("schema = 1\n[notices]\nquiet = \"yes\"\n").len(), 1);
}

/// PLAN M19 U20: workspaces are named by number; a name missing, empty or blank keeps
/// the number, a long one is cut, and names past the ninth are never used.
#[test]
fn workspaces_take_the_names_given_by_number() {
    let d = Config::default().windows;
    assert!(d.workspace_names.is_empty());
    assert_eq!(d.workspace_name(1), None);
    let c = parse("schema = 1\n[windows]\nworkspace_names = [\"Web\", \"\", \" Kod \", \"   \"]\n")
        .unwrap()
        .config
        .windows;
    assert_eq!(c.workspace_name(1), Some("Web".into()));
    assert_eq!(c.workspace_name(2), None);
    assert_eq!(c.workspace_name(3), Some("Kod".into()));
    assert_eq!(c.workspace_name(4), None);
    assert_eq!(c.workspace_name(5), None);
    assert_eq!(c.workspace_name(0), None);
    let long = "ş".repeat(WORKSPACE_NAME_MAX + 5);
    let w = Windows {
        workspace_names: vec![long],
        ..Windows::default()
    };
    assert_eq!(w.workspace_name(1).map(|n| n.chars().count()), Some(WORKSPACE_NAME_MAX));
    assert_eq!(errors("schema = 1\n[windows]\nworkspace_names = \"Web\"\n").len(), 1);
    assert_eq!(errors("schema = 1\n[windows]\nworkspace_names = [1, 2]\n").len(), 1);
}

/// D47: a still picture drifts by default, every ten minutes; the interval has bounds.
/// Asked for by the user (2026-10-31): the screen saver can be turned off, on by default.
#[test]
fn the_screen_saver_is_on_unless_turned_off() {
    assert!(Config::default().screensaver.enabled);
    let c = parse("schema = 1\n[screensaver]\nenabled = false\n").unwrap().config;
    assert!(!c.screensaver.enabled);
    assert_eq!(
        c.screensaver.kind,
        Config::default().screensaver.kind,
        "the choice kept"
    );
}

#[test]
fn a_still_picture_drifts_unless_turned_off() {
    let d = Config::default().wallpaper;
    assert!(d.drift && d.drift_minutes == 10);
    let c = parse("schema = 1\n[wallpaper]\ndrift = false\ndrift_minutes = 30\n")
        .unwrap()
        .config;
    assert!(!c.wallpaper.drift && c.wallpaper.drift_minutes == 30);
    assert_eq!(
        errors("schema = 1\n[wallpaper]\ndrift_minutes = 0\n")[0].path,
        path("wallpaper.drift_minutes")
    );
    assert_eq!(errors("schema = 1\n[wallpaper]\ndrift_minutes = 121\n").len(), 1);
}
