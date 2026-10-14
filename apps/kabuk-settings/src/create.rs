//! The Wallpaper page's Create tab (SPEC §2.8, PLAN M13): the API keys (Windows
//! Credential Manager, never a file or a log), the connection to a local Ollama, and
//! creating a wallpaper on a thread of its own: the model is asked over WinHTTP, each
//! answer judged on the GPU, the steps sent to the page as they happen, the result
//! previewed and, when the user applies it, kept in the state folder.
//!
//! Messages:
//! - `{"op":"gen-key","provider":"claude","key":"…"}` stores a key → `state`.
//! - `{"op":"gen-forget","provider":"claude"}` removes it → `state`.
//! - `{"op":"gen-connect"}` → `{"op":"gen-connected","ok":…,"model":…}` (Ollama).
//! - `{"op":"gen-run","prompt":"…"}` → `{"op":"gen-started"}`, then `gen-step` messages
//!   and one `gen-done` (name, preview) or `gen-failed` (reason, in the page's language).
//! - `{"op":"gen-cancel"}` stops between steps.
//! - `{"op":"gen-apply"}` keeps the last result → `state` with `"applied_wall": id`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kabuk_core::config::Config;
use kabuk_i18n::I18n;
use kabuk_ui::render::{Gpu, GpuJudge, Raster, WallInputs};
use kabuk_wallgen::check::OledFault;
use kabuk_wallgen::pipeline::{self, Created, Failure, Model, Refusal, Step};
use kabuk_wallgen::providers::{self, Message, Provider, ProviderError};
use serde_json::{json, Value};

/// Sends a message to the page from any thread.
pub type Outbox = Arc<dyn Fn(String) + Send + Sync>;

/// A model writing a wallpaper takes a while; a local one longer.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(300);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// The preview on the tab, and the wallpaper's time it shows.
const PREVIEW_SIZE: (u32, u32) = (400, 225);
const PREVIEW_AT: f32 = 12.0;

/// What the Create tab keeps between messages.
#[derive(Default)]
pub struct Creator {
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    /// The last wallpaper created and not kept yet.
    last: Arc<Mutex<Option<Created>>>,
}

pub fn provider(cfg: &Config) -> Provider {
    match cfg.create.provider {
        kabuk_core::config::Provider::Claude => Provider::Claude,
        kabuk_core::config::Provider::Openai => Provider::Openai,
        kabuk_core::config::Provider::Ollama => Provider::Ollama,
    }
}

fn named(name: &str) -> Option<Provider> {
    [Provider::Claude, Provider::Openai, Provider::Ollama]
        .into_iter()
        .find(|p| p.name() == name)
}

/// The look's colours a wallpaper is checked and previewed with (dark mode, as it plays).
pub fn colors(cfg: &Config) -> Vec<(String, (f32, f32, f32))> {
    let accent = (cfg.appearance.accent != kabuk_core::config::THEME_ACCENT).then_some(cfg.appearance.accent.as_str());
    let Some(r) = kabuk_theme::resolve::resolve(cfg.appearance.look.code(), kabuk_theme::Mode::Dark, accent) else {
        return Vec::new();
    };
    kabuk_wallgen::format::TOKENS
        .iter()
        .filter_map(|(_, token)| {
            let c = r.tokens.color(token)?;
            let f = |v: u8| f32::from(v) / 255.0;
            Some(((*token).to_string(), (f(c.r), f(c.g), f(c.b))))
        })
        .collect()
}

/// Asks a provider over WinHTTP.
struct HttpModel {
    provider: Provider,
    key: Option<String>,
    ollama: (String, String),
}

impl Model for HttpModel {
    fn ask(&mut self, system: &str, messages: &[Message]) -> Result<String, ProviderError> {
        let r = providers::ask(
            self.provider,
            self.key.as_deref(),
            (&self.ollama.0, &self.ollama.1),
            system,
            messages,
        )?;
        let answer = kabuk_win::http::send(&r.url, &r.headers, r.body.as_deref(), ANSWER_TIMEOUT)
            .map_err(|e| ProviderError::Network(e.to_string()))?;
        tracing::info!(
            provider = self.provider.name(),
            status = answer.status,
            bytes = answer.body.len(),
            "model answered"
        );
        providers::reply(self.provider, answer.status, &answer.body)
    }
}

/// The model Ollama has installed, asked of Ollama itself.
fn ollama_model(url: &str) -> Result<String, ProviderError> {
    let r = providers::ollama_tags(url);
    let answer = kabuk_win::http::send(&r.url, &r.headers, None, CONNECT_TIMEOUT)
        .map_err(|e| ProviderError::Network(e.to_string()))?;
    providers::ollama_model(answer.status, &answer.body)
}

/// A refusal in the user's words.
fn why(r: &Refusal, i18n: &I18n) -> String {
    match r {
        Refusal::Reply(_) => i18n.tr("gen-why-reply"),
        Refusal::Format(_) => i18n.tr("gen-why-format"),
        Refusal::Compile(_) => i18n.tr("gen-why-compile"),
        Refusal::Slow(ms) => i18n.tr_args("gen-why-slow", &[("value", i18n.decimal(f64::from(*ms), 1).into())]),
        Refusal::Draw(_) => i18n.tr("gen-why-draw"),
        Refusal::Oled(OledFault::NotBlack(_)) => i18n.tr("gen-why-black"),
        Refusal::Oled(OledFault::TooBright(a)) => i18n.tr_args(
            "gen-why-bright",
            &[("value", i18n.decimal(f64::from(*a) * 100.0, 1).into())],
        ),
        Refusal::Oled(OledFault::StaticBright(_)) => i18n.tr("gen-why-static"),
    }
}

/// Why nothing was created, in the user's words.
fn failure(f: &Failure, provider: Provider, i18n: &I18n) -> String {
    let name = i18n.tr(&format!("gen-name-{}", provider.name()));
    match f {
        Failure::Provider(ProviderError::NoKey) => i18n.tr("gen-fail-no-key"),
        Failure::Provider(ProviderError::KeyRefused) => i18n.tr("gen-fail-key"),
        Failure::Provider(ProviderError::NoModel) => i18n.tr("gen-fail-no-model"),
        Failure::Provider(ProviderError::Network(_)) => i18n.tr_args("gen-fail-network", &[("value", name.into())]),
        Failure::Provider(ProviderError::Api { message, .. }) => i18n.tr_args(
            "gen-fail-api",
            &[(
                "value",
                format!("{name}: {}", message.chars().take(160).collect::<String>()).into(),
            )],
        ),
        Failure::Provider(ProviderError::Unreadable) => i18n.tr_args("gen-fail-unreadable", &[("value", name.into())]),
        Failure::Refused(r) => i18n.tr_args("gen-fail-refused", &[("value", why(r, i18n).into())]),
        Failure::Cancelled => i18n.tr("gen-fail-cancelled"),
    }
}

/// `v` rounded to 1/`per` for the page (in f64, so the JSON shows no f32 tail).
fn round_to(v: f32, per: f64) -> f64 {
    (f64::from(v) * per).round() / per
}

fn step_message(s: &Step, i18n: &I18n) -> Value {
    match s {
        Step::Asking(n) => json!({ "op": "gen-step", "step": "asking", "attempt": n }),
        Step::Wrote => json!({ "op": "gen-step", "step": "wrote" }),
        Step::Compiled => json!({ "op": "gen-step", "step": "compiled" }),
        Step::Performance(ms) => json!({ "op": "gen-step", "step": "performance", "ms": round_to(*ms, 100.0) }),
        Step::Oled(o) => json!({ "op": "gen-step", "step": "oled", "average": round_to(o.average * 100.0, 10.0) }),
        Step::Refused(n, r) => json!({ "op": "gen-step", "step": "refused", "attempt": n, "reason": why(r, i18n) }),
    }
}

/// A frame of `c` for the tab, as a picture URL.
fn preview(gpu: &Gpu, c: &Created, colors: &[(String, (f32, f32, f32))], brightness: f32) -> Option<String> {
    let shader = gpu.compile_wall(&c.manifest, &c.hlsl).ok()?;
    let tokens = |name: &str| colors.iter().find(|(n, _)| n == name).map(|(_, c)| *c);
    let values = c.manifest.defaults();
    let inputs = WallInputs {
        tokens: &tokens,
        values: &values,
        brightness,
    };
    let frame = gpu.wall_frames(&shader, inputs, PREVIEW_SIZE, &[PREVIEW_AT]).ok()?;
    let p = frame.first()?;
    let bgr: Vec<u8> = p.rgba.chunks_exact(4).flat_map(|px| [px[2], px[1], px[0]]).collect();
    Some(kabuk_core::wallpaper::bmp_data_url(p.width, p.height, &bgr))
}

/// Everything a run needs, taken on the page's thread.
struct Job {
    provider: Provider,
    key: Option<String>,
    ollama_url: String,
    prompt: String,
    colors: Vec<(String, (f32, f32, f32))>,
    brightness: f32,
    /// The page's language (the messages are made on the worker's thread).
    lang: kabuk_i18n::Lang,
}

fn run(job: Job, outbox: &Outbox, cancel: &AtomicBool, last: &Mutex<Option<Created>>) {
    let i18n = I18n::new(job.lang);
    let send = |v: Value| outbox(v.to_string());
    let fail = |f: Failure| {
        tracing::warn!(?f, "no wallpaper created");
        send(json!({ "op": "gen-failed", "reason": failure(&f, job.provider, &i18n) }));
    };
    let model = if job.provider == Provider::Ollama {
        match ollama_model(&job.ollama_url) {
            Ok(m) => m,
            Err(e) => return fail(Failure::Provider(e)),
        }
    } else {
        String::new()
    };
    let gpu = match Gpu::with(Raster::Hardware) {
        Ok(g) => g,
        Err(e) => return fail(Failure::Refused(Refusal::Draw(e.to_string()))),
    };
    let colors = job.colors.clone();
    let tokens = |name: &str| colors.iter().find(|(n, _)| n == name).map(|(_, c)| *c);
    let mut judge = GpuJudge {
        gpu: &gpu,
        tokens: &tokens,
    };
    let mut model = HttpModel {
        provider: job.provider,
        key: job.key.clone(),
        ollama: (job.ollama_url.clone(), model),
    };
    tracing::info!(provider = job.provider.name(), "creating a wallpaper");
    let result = pipeline::create(
        &mut model,
        &mut judge,
        &job.prompt,
        &mut |s| send(step_message(&s, &i18n)),
        &|| cancel.load(Ordering::Acquire),
    );
    match result {
        Ok(c) => {
            tracing::info!(name = %c.manifest.name, attempts = c.attempts, frame_ms = c.frame_ms, "wallpaper created");
            let picture = preview(&gpu, &c, &job.colors, job.brightness);
            send(json!({
                "op": "gen-done",
                "name": c.manifest.name,
                "preview": picture,
                "ms": round_to(c.frame_ms, 100.0),
                "average": round_to(c.oled.average * 100.0, 10.0),
            }));
            if let Ok(mut l) = last.lock() {
                *l = Some(c);
            }
        }
        Err(f) => fail(f),
    }
}

impl super::bridge::Bridge {
    /// The Create tab's part of the state: which keys are stored, the created
    /// wallpapers, and whether one is being made.
    pub fn create_state(&self) -> Value {
        let id = self.instance.id();
        let walls: Vec<Value> = kabuk_wallgen::store::list(&self.walls_dir())
            .into_iter()
            .map(|(wid, m)| json!({ "id": wid, "name": m.name }))
            .collect();
        json!({
            "keys": {
                "claude": kabuk_win::credentials::exists(&Provider::Claude.credential(id)),
                "openai": kabuk_win::credentials::exists(&Provider::Openai.credential(id)),
            },
            "walls": walls,
            "running": self.creator.running.load(Ordering::Acquire),
        })
    }

    fn walls_dir(&self) -> std::path::PathBuf {
        self.state_dir.join(kabuk_wallgen::store::WALLS_DIR)
    }

    pub fn create_message(&self, op: &str, m: &Value) -> Option<Value> {
        match op {
            "gen-key" => {
                let p = named(m.get("provider")?.as_str()?).filter(|p| p.needs_key())?;
                let key = m.get("key")?.as_str()?.trim();
                if key.is_empty() {
                    return Some(json!({ "op": "refused", "path": "gen-key" }));
                }
                match kabuk_win::credentials::write(&p.credential(self.instance.id()), key) {
                    Ok(()) => tracing::info!(provider = p.name(), "API key stored"),
                    Err(e) => tracing::error!(provider = p.name(), error = %e, "API key not stored"),
                }
                Some(self.state())
            }
            "gen-forget" => {
                let p = named(m.get("provider")?.as_str()?)?;
                if let Err(e) = kabuk_win::credentials::delete(&p.credential(self.instance.id())) {
                    tracing::error!(provider = p.name(), error = %e, "API key not removed");
                }
                tracing::info!(provider = p.name(), "API key removed");
                Some(self.state())
            }
            "gen-connect" => {
                let cfg = self.state_config();
                let answer = ollama_model(&cfg.create.ollama_url);
                tracing::info!(ok = answer.is_ok(), "Ollama asked for its models");
                Some(json!({ "op": "gen-connected", "ok": answer.is_ok(), "model": answer.ok() }))
            }
            "gen-run" => {
                let prompt = m.get("prompt")?.as_str()?.trim().to_string();
                if prompt.is_empty() || self.creator.running.swap(true, Ordering::AcqRel) {
                    return Some(json!({ "op": "refused", "path": "gen-run" }));
                }
                let cfg = self.state_config();
                let provider = provider(&cfg);
                let job = Job {
                    provider,
                    key: provider
                        .needs_key()
                        .then(|| kabuk_win::credentials::read(&provider.credential(self.instance.id())))
                        .flatten(),
                    ollama_url: cfg.create.ollama_url.clone(),
                    prompt,
                    colors: colors(&cfg),
                    brightness: cfg.wallpaper.brightness as f32 / 100.0,
                    lang: kabuk_win::system::i18n(Some(&cfg)).lang(),
                };
                self.creator.cancel.store(false, Ordering::Release);
                let (outbox, running, cancel, last) = (
                    self.outbox.clone(),
                    self.creator.running.clone(),
                    self.creator.cancel.clone(),
                    self.creator.last.clone(),
                );
                let spawned = std::thread::Builder::new().name("create".into()).spawn(move || {
                    kabuk_win::system::com_init();
                    run(job, &outbox, &cancel, &last);
                    running.store(false, Ordering::Release);
                });
                if let Err(e) = spawned {
                    tracing::error!(error = %e, "create thread not started");
                    self.creator.running.store(false, Ordering::Release);
                    return Some(json!({ "op": "refused", "path": "gen-run" }));
                }
                Some(json!({ "op": "gen-started" }))
            }
            "gen-cancel" => {
                self.creator.cancel.store(true, Ordering::Release);
                tracing::info!("creating cancelled");
                None
            }
            "gen-apply" => {
                let c = self.creator.last.lock().ok()?.take()?;
                let dir = self.walls_dir();
                let id = kabuk_wallgen::store::new_id(
                    &dir,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs()),
                );
                match kabuk_wallgen::store::save(&dir, &id, &c.toml, &c.hlsl) {
                    Ok(_) => {
                        tracing::info!(id, name = %c.manifest.name, "created wallpaper kept");
                        let mut s = self.state();
                        s["applied_wall"] = json!(id);
                        Some(s)
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "created wallpaper not kept");
                        Some(json!({ "op": "refused", "path": "gen-apply" }))
                    }
                }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases: refusals and failures in the user's words (both languages have every
    //! message); steps as the page reads them; the providers by name.
    use super::*;
    use kabuk_i18n::Lang;

    #[test]
    fn every_reason_has_words_in_both_languages() {
        for lang in [Lang::Tr, Lang::En] {
            let i18n = I18n::new(lang);
            let refusals = [
                Refusal::Reply(kabuk_wallgen::reply::ReplyError::NoToml),
                Refusal::Format(kabuk_wallgen::format::FormatError::Name),
                Refusal::Compile("x".into()),
                Refusal::Slow(9.3),
                Refusal::Draw("x".into()),
                Refusal::Oled(OledFault::NotBlack(0.1)),
                Refusal::Oled(OledFault::TooBright(0.123)),
                Refusal::Oled(OledFault::StaticBright(0.01)),
            ];
            for r in &refusals {
                let w = why(r, &i18n);
                assert!(!w.is_empty() && !w.starts_with("gen-"), "{lang:?} {r:?}: {w}");
            }
            let decimal = if lang == Lang::Tr { "9,3" } else { "9.3" };
            assert!(why(&Refusal::Slow(9.3), &i18n).contains(decimal), "{lang:?}");
            for p in [Provider::Claude, Provider::Openai, Provider::Ollama] {
                for f in [
                    Failure::Provider(ProviderError::NoKey),
                    Failure::Provider(ProviderError::KeyRefused),
                    Failure::Provider(ProviderError::NoModel),
                    Failure::Provider(ProviderError::Network("x".into())),
                    Failure::Provider(ProviderError::Api {
                        status: 500,
                        message: "boom".into(),
                    }),
                    Failure::Provider(ProviderError::Unreadable),
                    Failure::Refused(Refusal::Slow(5.0)),
                    Failure::Cancelled,
                ] {
                    let w = failure(&f, p, &i18n);
                    assert!(!w.is_empty() && !w.contains("gen-"), "{lang:?} {f:?}: {w}");
                }
            }
        }
    }

    #[test]
    fn steps_go_to_the_page_as_it_reads_them() {
        let i18n = I18n::new(Lang::En);
        assert_eq!(step_message(&Step::Asking(2), &i18n)["attempt"], 2);
        assert_eq!(step_message(&Step::Performance(0.8123), &i18n)["ms"], 0.81);
        let o = kabuk_wallgen::check::Oled {
            average: 0.0312,
            black_share: 0.9,
            static_share: 0.0,
        };
        assert_eq!(step_message(&Step::Oled(o), &i18n)["average"], 3.1);
        let r = step_message(&Step::Refused(1, Refusal::Compile("e".into())), &i18n);
        assert_eq!((r["step"].as_str(), r["attempt"].as_u64()), (Some("refused"), Some(1)));
        assert_eq!(named("openai"), Some(Provider::Openai));
        assert_eq!(named("bard"), None);
    }
}
