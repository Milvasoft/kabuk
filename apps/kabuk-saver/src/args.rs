//! The command line: Windows' screen saver arguments (`/s`, `/p <hwnd>`, `/c[:hwnd]`,
//! any case, `-` for `/`, the number after a space or a colon) and Kabuk's own. Pure.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `/s`: full screen on every monitor until the user moves.
    Saver,
    /// `/p <hwnd>`: inside Windows' preview box.
    Preview(u64),
    /// `/c` or no argument: the settings, which are Kabuk's.
    Configure,
    /// `--window <w>x<h>`: in an ordinary window (tests and trying a scene).
    Window(u32, u32),
    /// `--wallpaper`: behind the desktop icons, on the monitors `[wallpaper]` animates.
    Wallpaper,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub mode: Mode,
    /// `--config <file>`: `kabuk.toml` (default: the user's).
    pub config: Option<PathBuf>,
    /// `--state-dir <folder>`: where the logs go (default: the user's).
    pub state_dir: Option<PathBuf>,
    /// `--frames <n>`: stop after this many frames (tests).
    pub frames: Option<u64>,
    /// `--scene circuit|constellation`: instead of the configured one.
    pub scene: Option<String>,
    /// `--keep`: input does not end the saver (trying it by hand with `--frames`).
    pub keep: bool,
    /// `--parent <pid>`: the wallpaper ends with this process (Kabuk).
    pub parent: Option<u32>,
}

fn number(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}

/// The arguments after the program's name.
pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut out = Args {
        mode: Mode::Configure,
        config: None,
        state_dir: None,
        frames: None,
        scene: None,
        keep: false,
        parent: None,
    };
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let value = |i: usize| args.get(i + 1).cloned().ok_or_else(|| format!("{a} needs a value"));
        match a.as_str() {
            "--config" => {
                out.config = Some(PathBuf::from(value(i)?));
                i += 1;
            }
            "--state-dir" => {
                out.state_dir = Some(PathBuf::from(value(i)?));
                i += 1;
            }
            "--frames" => {
                out.frames = Some(number(&value(i)?).ok_or("--frames needs a number")?);
                i += 1;
            }
            "--keep" => out.keep = true,
            "--wallpaper" => out.mode = Mode::Wallpaper,
            "--parent" => {
                let pid = number(&value(i)?).and_then(|n| u32::try_from(n).ok());
                out.parent = Some(pid.ok_or("--parent needs a process id")?);
                i += 1;
            }
            "--scene" => {
                out.scene = Some(value(i)?);
                i += 1;
            }
            "--window" => {
                let v = value(i)?;
                let (w, h) = v.split_once('x').ok_or("--window needs <w>x<h>")?;
                let size = |t: &str| t.parse::<u32>().ok().filter(|n| *n > 0);
                out.mode = Mode::Window(size(w).ok_or("bad width")?, size(h).ok_or("bad height")?);
                i += 1;
            }
            _ if a.starts_with('/') || a.starts_with('-') => {
                let flag = a[1..].to_ascii_lowercase();
                let (key, rest) = flag.split_at(flag.len().min(1));
                let rest = rest.trim_start_matches(':');
                match key {
                    "s" => out.mode = Mode::Saver,
                    "c" => out.mode = Mode::Configure,
                    "p" => {
                        let hwnd = match number(rest) {
                            Some(h) => h,
                            None => {
                                i += 1;
                                args.get(i).and_then(|t| number(t)).ok_or("/p needs a window")?
                            }
                        };
                        out.mode = Mode::Preview(hwnd);
                    }
                    _ => return Err(format!("unknown argument {a}")),
                }
            }
            _ => return Err(format!("unknown argument {a}")),
        }
        i += 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    //! Edge cases: every form Windows uses (`/s`, `/S`, `-s`, `/p 123`, `/p:123`,
    //! `/c:456`), no argument, Kabuk's own options, and mistakes explained.
    use super::*;

    fn p(line: &str) -> Result<Args, String> {
        parse(&line.split_whitespace().map(String::from).collect::<Vec<_>>())
    }

    #[test]
    fn windows_forms_and_kabuks_options() {
        assert_eq!(p("").unwrap().mode, Mode::Configure);
        for s in ["/s", "/S", "-s"] {
            assert_eq!(p(s).unwrap().mode, Mode::Saver, "{s}");
        }
        assert_eq!(p("/p 123").unwrap().mode, Mode::Preview(123));
        assert_eq!(p("/p:123").unwrap().mode, Mode::Preview(123));
        assert_eq!(p("/c:456").unwrap().mode, Mode::Configure);
        let a = p("--window 640x360 --frames 30 --scene constellation --keep --config C:\\k.toml --state-dir C:\\s")
            .unwrap();
        assert_eq!(a.mode, Mode::Window(640, 360));
        assert_eq!(a.frames, Some(30));
        assert!(a.keep);
        assert!(!p("/s").unwrap().keep);
        let w = p("--wallpaper --parent 4321").unwrap();
        assert_eq!((w.mode, w.parent), (Mode::Wallpaper, Some(4321)));
        assert!(p("--parent x").is_err());
        assert_eq!(a.scene.as_deref(), Some("constellation"));
        assert_eq!(a.config, Some(PathBuf::from("C:\\k.toml")));
        assert_eq!(a.state_dir, Some(PathBuf::from("C:\\s")));
    }

    #[test]
    fn mistakes_are_explained() {
        assert!(p("/p").is_err());
        assert!(p("/x").is_err());
        assert!(p("--window 0x10").is_err());
        assert!(p("--window big").is_err());
        assert!(p("--frames many").is_err());
        assert!(p("--config").is_err());
        assert!(p("extra").is_err());
    }
}
