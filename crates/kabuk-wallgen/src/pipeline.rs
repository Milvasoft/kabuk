//! Creating a wallpaper (SPEC §2.8): the model writes it, Kabuk checks it (format,
//! compile, frame time at 4K, OLED) and sends any refusal back for the model to fix, at
//! most [`MAX_ATTEMPTS`] times. The model and the GPU are behind traits, so the loop is
//! tested without either.

use crate::check::{Oled, OledFault};
use crate::format::{check_source, FormatError, Manifest};
use crate::prompt::{self, MAX_FRAME_MS};
use crate::providers::{Message, ProviderError, Role};
use crate::reply::{self, ReplyError};

/// Tries in all, the first one included (SPEC §2.8: at most three).
pub const MAX_ATTEMPTS: usize = 3;

/// Asks the model for its next reply.
pub trait Model {
    fn ask(&mut self, system: &str, messages: &[Message]) -> Result<String, ProviderError>;
}

/// Compiles and measures a wallpaper on the GPU.
pub trait Judge {
    type Compiled;
    /// The compiler's messages on failure (and the instruction and loop rules).
    fn compile(&mut self, manifest: &Manifest, hlsl: &str) -> Result<Self::Compiled, String>;
    /// The slowest frame at 4K, in milliseconds.
    fn frame_ms(&mut self, compiled: &Self::Compiled) -> Result<f32, String>;
    /// The frames over a minute, as brightness grids ([`crate::check`]).
    fn oled_frames(&mut self, compiled: &Self::Compiled) -> Result<Vec<Vec<f32>>, String>;
}

/// Why a try was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    Reply(ReplyError),
    Format(FormatError),
    Compile(String),
    Slow(f32),
    /// The GPU could not draw it at all.
    Draw(String),
    Oled(OledFault),
}

impl Refusal {
    /// What the model is told.
    pub fn for_model(&self) -> String {
        match self {
            Refusal::Reply(e) => e.to_string(),
            Refusal::Format(e) => e.to_string(),
            Refusal::Compile(m) => format!("it does not compile:\n{m}"),
            Refusal::Slow(ms) => format!("a frame takes {ms:.1} ms at 3840x2160; at most {MAX_FRAME_MS} ms is allowed"),
            Refusal::Draw(m) => format!("it could not be drawn: {m}"),
            Refusal::Oled(OledFault::NotBlack(share)) => format!(
                "the background is not black: only {:.0} % of the screen is black, at least {:.0} % must be",
                share * 100.0,
                crate::check::BLACK_SHARE * 100.0
            ),
            Refusal::Oled(OledFault::TooBright(avg)) => format!(
                "the average brightness is {:.1} %; at most {:.0} % is allowed",
                avg * 100.0,
                crate::check::MAX_AVERAGE * 100.0
            ),
            Refusal::Oled(OledFault::StaticBright(share)) => format!(
                "{:.1} % of the screen stays bright in the same place for a minute; bright parts must move or fade",
                share * 100.0
            ),
        }
    }
}

/// A step of the work, for the steps list on the Create tab.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// The model is asked (attempt 1-3).
    Asking(usize),
    Wrote,
    Compiled,
    Performance(f32),
    Oled(Oled),
    Refused(usize, Refusal),
}

/// A wallpaper that passed every check.
#[derive(Clone, Debug, PartialEq)]
pub struct Created {
    pub manifest: Manifest,
    pub toml: String,
    pub hlsl: String,
    pub frame_ms: f32,
    pub oled: Oled,
    pub attempts: usize,
}

/// Why nothing was created.
#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    /// The provider could not be asked or did not answer.
    Provider(ProviderError),
    /// Every try was refused; the last reason.
    Refused(Refusal),
    /// The user stopped it.
    Cancelled,
}

/// One reply's checks, in the order of the steps list.
pub fn assess<J: Judge>(judge: &mut J, text: &str, step: &mut dyn FnMut(Step)) -> Result<Created, Refusal> {
    let files = reply::files(text).map_err(Refusal::Reply)?;
    let manifest = Manifest::parse(&files.toml).map_err(Refusal::Format)?;
    check_source(&files.hlsl).map_err(Refusal::Format)?;
    step(Step::Wrote);
    let compiled = judge.compile(&manifest, &files.hlsl).map_err(Refusal::Compile)?;
    step(Step::Compiled);
    let ms = judge.frame_ms(&compiled).map_err(Refusal::Draw)?;
    if ms > MAX_FRAME_MS {
        return Err(Refusal::Slow(ms));
    }
    step(Step::Performance(ms));
    let oled = Oled::measure(&judge.oled_frames(&compiled).map_err(Refusal::Draw)?);
    if let Some(f) = oled.fault() {
        return Err(Refusal::Oled(f));
    }
    step(Step::Oled(oled));
    Ok(Created {
        manifest,
        toml: files.toml,
        hlsl: files.hlsl,
        frame_ms: ms,
        oled,
        attempts: 0,
    })
}

/// Creates a wallpaper for `request`. `step` hears of each step; `cancelled` is asked
/// between them.
pub fn create<M: Model, J: Judge>(
    model: &mut M,
    judge_: &mut J,
    request: &str,
    step: &mut dyn FnMut(Step),
    cancelled: &dyn Fn() -> bool,
) -> Result<Created, Failure> {
    let system = prompt::system();
    let mut messages = vec![Message {
        role: Role::User,
        text: prompt::request(request),
    }];
    let mut last = None;
    for attempt in 1..=MAX_ATTEMPTS {
        if cancelled() {
            return Err(Failure::Cancelled);
        }
        step(Step::Asking(attempt));
        let text = model.ask(&system, &messages).map_err(Failure::Provider)?;
        if cancelled() {
            return Err(Failure::Cancelled);
        }
        match assess(judge_, &text, step) {
            Ok(mut c) => {
                c.attempts = attempt;
                return Ok(c);
            }
            Err(r) => {
                tracing::info!(attempt, reason = %r.for_model(), "created wallpaper refused");
                step(Step::Refused(attempt, r.clone()));
                messages.push(Message {
                    role: Role::Assistant,
                    text,
                });
                messages.push(Message {
                    role: Role::User,
                    text: prompt::fix(&r.for_model()),
                });
                last = Some(r);
            }
        }
    }
    Err(last.map_or(Failure::Cancelled, Failure::Refused))
}

#[cfg(test)]
mod tests {
    //! Edge cases: a first reply that passes; each kind of refusal sent back and then
    //! fixed; three refusals end it with the last reason; a provider error ends it at
    //! once; cancelling between steps; the model sees its own reply and the reason.
    use super::*;
    use crate::check::SAMPLES;
    use crate::prompt::{SAMPLE_HLSL, SAMPLE_TOML};

    fn good() -> String {
        format!("```toml\n{SAMPLE_TOML}```\n```hlsl\n{SAMPLE_HLSL}```\n")
    }

    /// Replies in turn; records what it was asked.
    struct Script {
        replies: Vec<Result<String, ProviderError>>,
        seen: Vec<Vec<Message>>,
    }

    impl Model for Script {
        fn ask(&mut self, _system: &str, messages: &[Message]) -> Result<String, ProviderError> {
            self.seen.push(messages.to_vec());
            self.replies.remove(0)
        }
    }

    fn script(replies: Vec<Result<String, ProviderError>>) -> Script {
        Script {
            replies,
            seen: Vec::new(),
        }
    }

    /// Compiles anything containing "wall", times and lights by markers in the source.
    #[derive(Default)]
    struct Fake;

    impl Judge for Fake {
        type Compiled = String;
        fn compile(&mut self, _m: &Manifest, hlsl: &str) -> Result<String, String> {
            if hlsl.contains("SYNTAX") {
                Err("wall.hlsl(3,5): error X3000: syntax error".into())
            } else {
                Ok(hlsl.to_string())
            }
        }
        fn frame_ms(&mut self, c: &String) -> Result<f32, String> {
            Ok(if c.contains("SLOW") { 9.5 } else { 0.8 })
        }
        fn oled_frames(&mut self, c: &String) -> Result<Vec<Vec<f32>>, String> {
            let v = if c.contains("GREY") { 0.1 } else { 0.0 };
            Ok(vec![vec![v; 64]; SAMPLES])
        }
    }

    fn with(marker: &str) -> String {
        good().replace("float3 c = 0;", &format!("float3 c = 0; // {marker}"))
    }

    fn run(m: &mut Script) -> (Result<Created, Failure>, Vec<Step>) {
        let mut steps = Vec::new();
        let r = create(m, &mut Fake, "mor dalgalar", &mut |s| steps.push(s), &|| false);
        (r, steps)
    }

    #[test]
    fn a_reply_that_passes_is_created_with_every_step() {
        let mut m = script(vec![Ok(good())]);
        let (r, steps) = run(&mut m);
        let c = r.unwrap();
        assert_eq!(
            (c.manifest.name.as_str(), c.attempts, c.frame_ms),
            ("Slow ribbons", 1, 0.8)
        );
        assert!(matches!(
            steps.as_slice(),
            [
                Step::Asking(1),
                Step::Wrote,
                Step::Compiled,
                Step::Performance(_),
                Step::Oled(_)
            ]
        ));
        assert_eq!(m.seen[0].len(), 1);
        assert!(m.seen[0][0].text.contains("mor dalgalar"));
    }

    #[test]
    fn each_refusal_goes_back_to_the_model_and_its_fix_is_taken() {
        for (bad, expect) in [
            ("no blocks".to_string(), "no ```toml block"),
            (
                good().replace("name = \"Slow ribbons\"", "name = \"\""),
                "the name must be",
            ),
            (
                good().replace("float3 wall(float2 px)", "float3 wall2(float2 px)"),
                "must define",
            ),
            (with("SYNTAX"), "error X3000"),
            (with("SLOW"), "9.5 ms"),
            (with("GREY"), "background is not black"),
        ] {
            let mut m = script(vec![Ok(bad.clone()), Ok(good())]);
            let (r, steps) = run(&mut m);
            assert_eq!(r.unwrap().attempts, 2, "{expect}");
            assert!(steps.iter().any(|s| matches!(s, Step::Refused(1, _))), "{expect}");
            // The second ask carries the model's reply and the reason.
            let second = &m.seen[1];
            assert_eq!(second.len(), 3);
            assert_eq!(
                (second[1].role, second[1].text.as_str()),
                (Role::Assistant, bad.as_str())
            );
            assert!(second[2].text.contains(expect), "{expect}: {}", second[2].text);
        }
    }

    #[test]
    fn three_refusals_end_it_with_the_last_reason() {
        let mut m = script(vec![Ok(with("SYNTAX")), Ok(with("SLOW")), Ok(with("GREY")), Ok(good())]);
        let (r, steps) = run(&mut m);
        assert!(
            matches!(r, Err(Failure::Refused(Refusal::Oled(OledFault::NotBlack(_))))),
            "{r:?}"
        );
        assert_eq!(m.seen.len(), MAX_ATTEMPTS, "no fourth ask");
        assert_eq!(steps.iter().filter(|s| matches!(s, Step::Refused(..))).count(), 3);
    }

    #[test]
    fn a_provider_error_ends_it_at_once() {
        let mut m = script(vec![Err(ProviderError::KeyRefused), Ok(good())]);
        let (r, _) = run(&mut m);
        assert_eq!(r, Err(Failure::Provider(ProviderError::KeyRefused)));
        assert_eq!(m.seen.len(), 1);
    }

    #[test]
    fn cancelling_stops_before_the_next_step() {
        let mut m = script(vec![Ok(good())]);
        let r = create(&mut m, &mut Fake, "x", &mut |_| {}, &|| true);
        assert_eq!(r, Err(Failure::Cancelled));
        assert!(m.seen.is_empty());
    }

    #[test]
    fn the_models_message_for_each_oled_fault() {
        let r = Refusal::Oled(OledFault::TooBright(0.123));
        assert!(r.for_model().contains("12.3 %"));
        let s = Refusal::Oled(OledFault::StaticBright(0.02));
        assert!(s.for_model().contains("must move or fade"));
    }
}
