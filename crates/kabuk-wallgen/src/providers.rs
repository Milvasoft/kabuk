//! The model providers (ARCHITECTURE §11b): Anthropic's Messages API, OpenAI's chat
//! completions and a local Ollama. Pure: requests are built and replies read here; the
//! transport (WinHTTP) sends them. A request's `Debug` never shows header values, as
//! they hold the API key.

use serde_json::{json, Value};

/// The models asked. Ollama's is the user's own, picked from what is installed.
pub const CLAUDE_MODEL: &str = "claude-opus-5-5";
pub const OPENAI_MODEL: &str = "gpt-5";
const CLAUDE_URL: &str = "https://api.anthropic.com/v1/messages";
const CLAUDE_VERSION: &str = "2023-06-01";
const OPENAI_URL: &str = "https://api.openai.com/v1/chat/completions";
/// A wallpaper's two files fit in this many tokens with room to spare.
const MAX_TOKENS: u32 = 8192;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    Claude,
    Openai,
    Ollama,
}

impl Provider {
    /// Whether the provider needs an API key (Ollama runs on the user's computer).
    pub fn needs_key(self) -> bool {
        self != Provider::Ollama
    }

    pub fn name(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Openai => "openai",
            Provider::Ollama => "ollama",
        }
    }

    /// The name the key is stored under in Windows Credential Manager, apart for each
    /// Kabuk (`instance`: its state folder's id), so a test's Kabuk never touches the
    /// user's key.
    pub fn credential(self, instance: &str) -> String {
        format!("Kabuk/{instance}/wallgen/{}", self.name())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub text: String,
}

/// An HTTP request to send.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    /// `GET` without a body, else `POST` with this JSON.
    pub body: Option<String>,
    pub url: String,
    pub headers: Vec<(String, String)>,
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(n, _)| n.as_str()).collect();
        f.debug_struct("Request")
            .field("url", &self.url)
            .field("headers", &names)
            .field("body_bytes", &self.body.as_ref().map(String::len))
            .finish()
    }
}

/// Why a provider gave no reply.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ProviderError {
    #[error("no API key")]
    NoKey,
    #[error("the API key was refused")]
    KeyRefused,
    #[error("could not reach the provider: {0}")]
    Network(String),
    #[error("the provider answered {status}: {message}")]
    Api { status: u16, message: String },
    #[error("the provider's answer could not be read")]
    Unreadable,
    #[error("no model is installed in Ollama")]
    NoModel,
}

fn base(url: &str) -> &str {
    url.trim_end_matches('/')
}

/// The request asking `model` for the next reply in `messages`.
pub fn ask(
    provider: Provider,
    key: Option<&str>,
    ollama: (&str, &str),
    system: &str,
    messages: &[Message],
) -> Result<Request, ProviderError> {
    let turns = |with_system: bool| -> Vec<Value> {
        let mut v = Vec::new();
        if with_system {
            v.push(json!({ "role": "system", "content": system }));
        }
        v.extend(
            messages
                .iter()
                .map(|m| json!({ "role": m.role.name(), "content": m.text })),
        );
        v
    };
    let json = |headers: Vec<(String, String)>| {
        let mut h = vec![("content-type".to_string(), "application/json".to_string())];
        h.extend(headers);
        h
    };
    match provider {
        Provider::Claude => {
            let key = key.filter(|k| !k.trim().is_empty()).ok_or(ProviderError::NoKey)?;
            Ok(Request {
                url: CLAUDE_URL.into(),
                headers: json(vec![
                    ("x-api-key".into(), key.trim().into()),
                    ("anthropic-version".into(), CLAUDE_VERSION.into()),
                ]),
                body: Some(
                    json!({ "model": CLAUDE_MODEL, "max_tokens": MAX_TOKENS, "system": system, "messages": turns(false) })
                        .to_string(),
                ),
            })
        }
        Provider::Openai => {
            let key = key.filter(|k| !k.trim().is_empty()).ok_or(ProviderError::NoKey)?;
            Ok(Request {
                url: OPENAI_URL.into(),
                headers: json(vec![("authorization".into(), format!("Bearer {}", key.trim()))]),
                body: Some(json!({ "model": OPENAI_MODEL, "messages": turns(true) }).to_string()),
            })
        }
        Provider::Ollama => {
            let (url, model) = ollama;
            Ok(Request {
                url: format!("{}/api/chat", base(url)),
                headers: json(Vec::new()),
                body: Some(json!({ "model": model, "messages": turns(true), "stream": false }).to_string()),
            })
        }
    }
}

/// The request listing Ollama's installed models.
pub fn ollama_tags(url: &str) -> Request {
    Request {
        url: format!("{}/api/tags", base(url)),
        headers: Vec::new(),
        body: None,
    }
}

/// The model to ask among Ollama's installed ones: the first made for code, else the
/// first.
pub fn ollama_model(status: u16, body: &str) -> Result<String, ProviderError> {
    let v = ok_json(status, body)?;
    let names: Vec<String> = v["models"]
        .as_array()
        .map(|a| a.iter().filter_map(|m| m["name"].as_str().map(String::from)).collect())
        .unwrap_or_default();
    names
        .iter()
        .find(|n| n.contains("coder"))
        .or_else(|| names.first())
        .cloned()
        .ok_or(ProviderError::NoModel)
}

fn ok_json(status: u16, body: &str) -> Result<Value, ProviderError> {
    let v: Option<Value> = serde_json::from_str(body).ok();
    if status == 401 || status == 403 {
        return Err(ProviderError::KeyRefused);
    }
    if !(200..300).contains(&status) {
        // Each provider puts its message under "error" ({"message": …} or a string).
        let message = v
            .as_ref()
            .and_then(|v| {
                v["error"]["message"]
                    .as_str()
                    .or_else(|| v["error"].as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| body.chars().take(200).collect());
        return Err(ProviderError::Api { status, message });
    }
    v.ok_or(ProviderError::Unreadable)
}

/// The reply's text from a provider's answer.
pub fn reply(provider: Provider, status: u16, body: &str) -> Result<String, ProviderError> {
    let v = ok_json(status, body)?;
    let text = match provider {
        Provider::Claude => v["content"].as_array().map(|parts| {
            parts
                .iter()
                .filter(|p| p["type"] == "text")
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        }),
        Provider::Openai => v["choices"][0]["message"]["content"].as_str().map(String::from),
        Provider::Ollama => v["message"]["content"].as_str().map(String::from),
    };
    text.filter(|t| !t.trim().is_empty()).ok_or(ProviderError::Unreadable)
}

#[cfg(test)]
mod tests {
    //! Edge cases: each provider's request (where the key goes, the system prompt, the
    //! turns); no key; a request's Debug hides the key; each provider's answer read;
    //! refused keys, API errors in each provider's shape, unreadable answers; Ollama's
    //! model picked.
    use super::*;

    fn turns() -> Vec<Message> {
        vec![
            Message {
                role: Role::User,
                text: "mor dalgalar".into(),
            },
            Message {
                role: Role::Assistant,
                text: "```toml".into(),
            },
        ]
    }

    fn body(r: &Request) -> Value {
        serde_json::from_str(r.body.as_ref().unwrap()).unwrap()
    }

    #[test]
    fn claude_gets_the_key_in_its_header_and_the_system_prompt_apart() {
        let r = ask(Provider::Claude, Some(" sk-ant-1 "), ("", ""), "SYS", &turns()).unwrap();
        assert_eq!(r.url, CLAUDE_URL);
        assert!(r.headers.contains(&("x-api-key".into(), "sk-ant-1".into())));
        assert!(r.headers.iter().any(|(n, _)| n == "anthropic-version"));
        let b = body(&r);
        assert_eq!(
            (b["model"].as_str(), b["system"].as_str()),
            (Some(CLAUDE_MODEL), Some("SYS"))
        );
        assert_eq!(b["messages"].as_array().unwrap().len(), 2);
        assert_eq!(b["messages"][1]["role"], "assistant");
        assert!(
            !r.body.as_ref().unwrap().contains("sk-ant-1"),
            "the key is only in the header"
        );
    }

    #[test]
    fn openai_and_ollama_take_the_system_prompt_as_the_first_turn() {
        let o = ask(Provider::Openai, Some("sk-1"), ("", ""), "SYS", &turns()).unwrap();
        assert!(o.headers.contains(&("authorization".into(), "Bearer sk-1".into())));
        let b = body(&o);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"].as_array().unwrap().len(), 3);
        let l = ask(
            Provider::Ollama,
            None,
            ("http://localhost:11434/", "qwen-coder"),
            "SYS",
            &turns(),
        )
        .unwrap();
        assert_eq!(l.url, "http://localhost:11434/api/chat");
        assert!(l.headers.iter().all(|(n, _)| n == "content-type"));
        let b = body(&l);
        assert_eq!(
            (b["model"].as_str(), b["stream"].as_bool()),
            (Some("qwen-coder"), Some(false))
        );
        assert_eq!(b["messages"][0]["content"], "SYS");
    }

    #[test]
    fn without_a_key_nothing_is_sent_and_debug_hides_the_key() {
        assert_eq!(
            ask(Provider::Claude, None, ("", ""), "", &[]),
            Err(ProviderError::NoKey)
        );
        assert_eq!(
            ask(Provider::Openai, Some("  "), ("", ""), "", &[]),
            Err(ProviderError::NoKey)
        );
        let r = ask(Provider::Openai, Some("sk-secret"), ("", ""), "", &[]).unwrap();
        let shown = format!("{r:?}");
        assert!(
            !shown.contains("sk-secret") && shown.contains("authorization"),
            "{shown}"
        );
        assert!(Provider::Claude.needs_key() && !Provider::Ollama.needs_key());
        assert_eq!(Provider::Openai.credential("ab12"), "Kabuk/ab12/wallgen/openai");
    }

    #[test]
    fn each_providers_answer_is_read() {
        let c = r#"{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}"#;
        assert_eq!(reply(Provider::Claude, 200, c).unwrap(), "ab");
        let o = r#"{"choices":[{"message":{"role":"assistant","content":"x"}}]}"#;
        assert_eq!(reply(Provider::Openai, 200, o).unwrap(), "x");
        let l = r#"{"message":{"role":"assistant","content":"y"},"done":true}"#;
        assert_eq!(reply(Provider::Ollama, 200, l).unwrap(), "y");
        assert_eq!(
            reply(Provider::Claude, 200, r#"{"content":[]}"#),
            Err(ProviderError::Unreadable)
        );
        assert_eq!(reply(Provider::Openai, 200, "not json"), Err(ProviderError::Unreadable));
    }

    #[test]
    fn refusals_and_errors_are_told_apart() {
        assert_eq!(reply(Provider::Claude, 401, "{}"), Err(ProviderError::KeyRefused));
        assert_eq!(reply(Provider::Openai, 403, ""), Err(ProviderError::KeyRefused));
        let claude = r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        assert_eq!(
            reply(Provider::Claude, 529, claude),
            Err(ProviderError::Api {
                status: 529,
                message: "Overloaded".into()
            })
        );
        let ollama = r#"{"error":"model 'x' not found"}"#;
        assert_eq!(
            reply(Provider::Ollama, 404, ollama),
            Err(ProviderError::Api {
                status: 404,
                message: "model 'x' not found".into()
            })
        );
        assert!(
            matches!(reply(Provider::Openai, 500, "boom"), Err(ProviderError::Api { status: 500, message }) if message == "boom")
        );
    }

    #[test]
    fn ollamas_model_is_picked_from_what_is_installed() {
        assert_eq!(ollama_tags("http://h:1/").url, "http://h:1/api/tags");
        let tags = r#"{"models":[{"name":"llama3:8b"},{"name":"qwen2.5-coder:7b"}]}"#;
        assert_eq!(ollama_model(200, tags).unwrap(), "qwen2.5-coder:7b");
        assert_eq!(
            ollama_model(200, r#"{"models":[{"name":"llama3:8b"}]}"#).unwrap(),
            "llama3:8b"
        );
        assert_eq!(ollama_model(200, r#"{"models":[]}"#), Err(ProviderError::NoModel));
    }
}
