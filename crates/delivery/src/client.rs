//! How the planner reaches a model. A provider's command-line client, its
//! API, or a local Ollama server each take a system prompt and a prompt and
//! give back the reply text. The planner keeps the replies (`planner.rs`) and
//! comes here only for one it does not have.
//!
//! The command-line clients are signed in as the person is and bill to
//! whatever that is; the APIs take a key from the environment. The `claude`
//! command replaces its system prompt and drops its tools, so it is one call
//! with the planner's prompt; `codex exec` is one turn of a coding agent that
//! keeps its tools. A comparison across providers should go `via: api`, where
//! what each provider is sent is exactly the request written here.
//!
//! No client asks for structured output (Codex's `--output-schema`, the
//! APIs' JSON modes, Ollama's `format`): the study counts a reply that does
//! not parse as the model's failure, and that count means the same thing
//! only if no client gets help. Sampling settings are the providers'
//! defaults everywhere, as the command-line clients leave them.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};

/// Whose model answers.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    OpenAi,
    Ollama,
}

/// How a provider is reached: its command-line client or its API.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    Cli,
    Api,
}

/// One way of asking a model, built by `Client::new` from the mission's
/// `provider`, `via`, `command`, and `host`.
pub enum Client {
    /// `claude -p`: the client the study ran on.
    ClaudeCli {
        command: String,
    },
    ClaudeApi {
        url: String,
        key: String,
    },
    /// `codex exec`: one turn of a coding agent, its tools kept.
    CodexCli {
        command: String,
    },
    OpenAiApi {
        url: String,
        key: String,
    },
    Ollama {
        url: String,
    },
}

/// By name only, so a key never lands in a message.
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl Client {
    /// `via` left out means the command-line client, or the server for
    /// Ollama. `command` overrides the executable's name and `host` the
    /// server. An API key is read from the environment unless `offline`,
    /// when nothing will be asked.
    pub fn new(
        provider: Provider,
        via: Option<Via>,
        command: Option<&str>,
        host: Option<&str>,
        offline: bool,
    ) -> Result<Self> {
        let key = |name: &str, provider: &str| -> Result<String> {
            if offline {
                return Ok(String::new());
            }
            std::env::var(name).map_err(|_| {
                anyhow!(
                    "set {name} in the environment to ask {provider} via api, \
                     or set offline: true to replay saved replies"
                )
            })
        };
        let url = |default: &str| with_scheme(host.unwrap_or(default));
        Ok(match (provider, via) {
            (Provider::Claude, None | Some(Via::Cli)) => Self::ClaudeCli {
                command: command.unwrap_or("claude").into(),
            },
            (Provider::Claude, Some(Via::Api)) => Self::ClaudeApi {
                url: url("https://api.anthropic.com"),
                key: key("ANTHROPIC_API_KEY", "claude")?,
            },
            (Provider::OpenAi, None | Some(Via::Cli)) => Self::CodexCli {
                command: command.unwrap_or("codex").into(),
            },
            (Provider::OpenAi, Some(Via::Api)) => Self::OpenAiApi {
                url: url("https://api.openai.com"),
                key: key("OPENAI_API_KEY", "openai")?,
            },
            (Provider::Ollama, Some(Via::Cli)) => {
                bail!("Ollama is reached through its server: leave via out or say via: api")
            }
            (Provider::Ollama, _) => Self::Ollama {
                url: match host {
                    Some(host) => with_scheme(host),
                    None => with_scheme(
                        &std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "localhost:11434".into()),
                    ),
                },
            },
        })
    }

    /// The name kept in each cached reply. The replies saved before there
    /// was a choice came from the `claude` command and are named `claude`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::ClaudeCli { .. } => "claude",
            Self::ClaudeApi { .. } => "claude-api",
            Self::CodexCli { .. } => "codex",
            Self::OpenAiApi { .. } => "openai-api",
            Self::Ollama { .. } => "ollama",
        }
    }

    /// What the cache key starts with: nothing for the `claude` command, so
    /// the replies already saved keep their names, and the client's name for
    /// every other client, so the same model through two clients never
    /// shares a reply.
    pub fn key_prefix(&self) -> String {
        match self {
            Self::ClaudeCli { .. } => String::new(),
            other => format!("{}\n", other.name()),
        }
    }

    /// Asks once, trying three times a minute apart. `sample` is the seed
    /// for Ollama; the other clients draw as they like.
    pub fn ask(&self, model: &str, system: &str, prompt: &str, sample: u32) -> Result<String> {
        let mut last_error = String::new();
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_secs(60));
            }
            match self.ask_once(model, system, prompt, sample) {
                Ok(reply) => return Ok(reply.trim().to_string()),
                Err(error) => last_error = format!("{error:#}"),
            }
        }
        bail!("{} failed three times: {last_error}", self.name())
    }

    fn ask_once(&self, model: &str, system: &str, prompt: &str, sample: u32) -> Result<String> {
        match self {
            Self::ClaudeCli { command } => {
                let mut client = Command::new(command);
                client
                    .args(["-p", "--model", model, "--output-format", "text"])
                    .args(["--no-session-persistence", "--tools", ""])
                    .args(["--system-prompt", system]);
                run(client, command, prompt)
            }
            Self::CodexCli { command } => {
                // The system prompt goes in through a file that replaces
                // Codex's built-in instructions; the reply comes out through
                // a file, since stdout carries the agent's progress. The
                // instructions file is a quoted TOML string.
                let folder = scratch_folder("codex")?;
                let system_path = folder.join("system.md");
                let reply_path = folder.join("reply.txt");
                std::fs::write(&system_path, system)?;
                let mut client = Command::new(command);
                client
                    .args(["exec", "-m", model, "--ephemeral", "--skip-git-repo-check"])
                    .args([
                        "-s",
                        "read-only",
                        "--color",
                        "never",
                        "--ignore-user-config",
                    ])
                    .arg("-c")
                    .arg(format!(
                        "model_instructions_file=\"{}\"",
                        system_path.display()
                    ))
                    // Codex would otherwise read this repository's AGENTS.md.
                    .args(["-c", "project_doc_max_bytes=0"])
                    .arg("-o")
                    .arg(&reply_path);
                let reply = run(client, command, prompt).and_then(|_| {
                    std::fs::read_to_string(&reply_path).with_context(|| {
                        format!("{command} wrote no reply to {}", reply_path.display())
                    })
                });
                let _ = std::fs::remove_dir_all(&folder);
                reply
            }
            Self::ClaudeApi { url, key } => {
                let body = json!({
                    "model": model,
                    "max_tokens": 4096,
                    "system": system,
                    "messages": [{"role": "user", "content": prompt}],
                });
                let reply = post(
                    &format!("{url}/v1/messages"),
                    &[("x-api-key", key), ("anthropic-version", "2026-10-21")],
                    &body,
                )?;
                let text = reply["content"]
                    .as_array()
                    .and_then(|blocks| blocks.iter().find(|block| block["type"] == "text"))
                    .and_then(|block| block["text"].as_str());
                text.map(str::to_string)
                    .ok_or_else(|| anyhow!("no text block in the reply: {reply}"))
            }
            Self::OpenAiApi { url, key } => {
                let body = json!({
                    "model": model,
                    "messages": [
                        {"role": "system", "content": system},
                        {"role": "user", "content": prompt},
                    ],
                });
                let reply = post(
                    &format!("{url}/v1/chat/completions"),
                    &[("authorization", &format!("Bearer {key}"))],
                    &body,
                )?;
                reply_text(&reply, "/choices/0/message/content")
            }
            Self::Ollama { url } => {
                let body = json!({
                    "model": model,
                    "messages": [
                        {"role": "system", "content": system},
                        {"role": "user", "content": prompt},
                    ],
                    "stream": false,
                    "options": {"seed": sample},
                });
                let reply = post(&format!("{url}/api/chat"), &[], &body).with_context(|| {
                    format!(
                        "asking Ollama at {url} (is `ollama serve` running, and is {model} pulled?)"
                    )
                })?;
                reply_text(&reply, "/message/content")
            }
        }
    }
}

/// `host` with `http://` in front when it names no scheme, as `OLLAMA_HOST`
/// often does not, and no trailing slash.
fn with_scheme(host: &str) -> String {
    let host = host.trim_end_matches('/');
    if host.contains("://") {
        host.to_string()
    } else {
        format!("http://{host}")
    }
}

/// A fresh folder for one call's files, unique to this process and thread.
fn scratch_folder(client: &str) -> Result<PathBuf> {
    let folder = std::env::temp_dir().join(format!(
        "ripple-{client}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&folder)?;
    Ok(folder)
}

/// Runs a command-line client with the prompt on stdin and returns its
/// stdout, or its stderr and stdout as the error.
fn run(mut client: Command, name: &str, prompt: &str) -> Result<String> {
    let mut child = client
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        // The client can be missing for a moment while it updates itself.
        .with_context(|| format!("starting {name}"))?;
    child
        .stdin
        .take()
        .context("model client stdin")?
        .write_all(prompt.as_bytes())?;
    let output = child.wait_with_output()?;
    ensure!(
        output.status.success(),
        // The client may report its error on either stream.
        "{} {}",
        String::from_utf8_lossy(&output.stderr).trim(),
        String::from_utf8_lossy(&output.stdout).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// One JSON request, with the status and body as the error on anything but
/// success. Ten minutes, since a local model loads on its first call and the
/// APIs queue under load.
fn post(url: &str, headers: &[(&str, &str)], body: &Value) -> Result<Value> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(600)))
        .http_status_as_error(false)
        .build();
    let mut request = ureq::Agent::new_with_config(config).post(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = request
        .send_json(body)
        .with_context(|| format!("POST {url}"))?;
    let status = response.status();
    let text = response
        .into_body()
        .read_to_string()
        .with_context(|| format!("reading the reply to POST {url}"))?;
    ensure!(
        status.is_success(),
        "POST {url} returned {status}: {}",
        text.trim()
    );
    serde_json::from_str(&text)
        .with_context(|| format!("POST {url} replied with something other than JSON: {text}"))
}

fn reply_text(reply: &Value, pointer: &str) -> Result<String> {
    reply
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("no {pointer} in the reply: {reply}"))
}

#[cfg(test)]
pub(crate) mod fake_server {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// One request as the server saw it: path, lowercased headers, body.
    pub struct Seen {
        pub path: String,
        pub headers: String,
        pub body: serde_json::Value,
    }

    /// A local HTTP server that answers every request with one status and
    /// body, keeping what it was sent.
    pub struct FakeServer {
        pub url: String,
        pub seen: Arc<Mutex<Vec<Seen>>>,
    }

    impl FakeServer {
        pub fn start(status: u16, reply: &'static str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
            let url = format!("http://{}", listener.local_addr().expect("bound"));
            let seen = Arc::new(Mutex::new(Vec::new()));
            let record = Arc::clone(&seen);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let mut stream = stream.expect("a connection");
                    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                    let mut line = String::new();
                    reader.read_line(&mut line).expect("request line");
                    let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                    let mut headers = String::new();
                    let mut length = 0;
                    loop {
                        let mut header = String::new();
                        reader.read_line(&mut header).expect("header");
                        if header.trim().is_empty() {
                            break;
                        }
                        let header = header.to_ascii_lowercase();
                        if let Some(value) = header.strip_prefix("content-length:") {
                            length = value.trim().parse().expect("a length");
                        }
                        headers.push_str(&header);
                    }
                    let mut body = vec![0; length];
                    reader.read_exact(&mut body).expect("body");
                    record.lock().expect("lock").push(Seen {
                        path,
                        headers,
                        body: serde_json::from_slice(&body).expect("a JSON body"),
                    });
                    let response = format!(
                        "HTTP/1.1 {status} Reply\r\ncontent-type: application/json\r\n\
                         content-length: {}\r\nconnection: close\r\n\r\n{reply}",
                        reply.len()
                    );
                    stream.write_all(response.as_bytes()).expect("write");
                }
            });
            Self { url, seen }
        }

        pub fn last(&self) -> Seen {
            let mut seen = self.seen.lock().expect("lock");
            seen.pop().expect("a request was made")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake_server::FakeServer;
    use super::*;

    #[test]
    fn claude_api_sends_the_system_prompt_and_reads_the_first_text_block() -> Result<()> {
        let server = FakeServer::start(
            200,
            r#"{"content": [{"type": "thinking", "thinking": "hm"},
                            {"type": "text", "text": "{\"assignments\": []}"}]}"#,
        );
        let client = Client::ClaudeApi {
            url: server.url.clone(),
            key: "secret".into(),
        };
        let reply = client.ask_once("claude-haiku-5-5", "Dispatch.", "Fleet: ...", 0)?;
        assert_eq!(reply, "{\"assignments\": []}");
        let seen = server.last();
        assert_eq!(seen.path, "/v1/messages");
        assert!(
            seen.headers.contains("x-api-key: secret"),
            "{}",
            seen.headers
        );
        assert!(seen.headers.contains("anthropic-version: 2026-10-21"));
        assert_eq!(seen.body["model"], "claude-haiku-5-5");
        assert_eq!(seen.body["system"], "Dispatch.");
        assert_eq!(seen.body["messages"][0]["role"], "user");
        assert_eq!(seen.body["messages"][0]["content"], "Fleet: ...");
        assert_eq!(seen.body["max_tokens"], 4096);
        Ok(())
    }

    #[test]
    fn openai_api_sends_system_and_user_messages() -> Result<()> {
        let server = FakeServer::start(
            200,
            r#"{"choices": [{"message": {"role": "assistant", "content": "ok"}}]}"#,
        );
        let client = Client::OpenAiApi {
            url: server.url.clone(),
            key: "secret".into(),
        };
        assert_eq!(client.ask_once("gpt", "Dispatch.", "Fleet: ...", 0)?, "ok");
        let seen = server.last();
        assert_eq!(seen.path, "/v1/chat/completions");
        assert!(
            seen.headers.contains("authorization: bearer secret"),
            "{}",
            seen.headers
        );
        assert_eq!(seen.body["model"], "gpt");
        assert_eq!(seen.body["messages"][0]["role"], "system");
        assert_eq!(seen.body["messages"][0]["content"], "Dispatch.");
        assert_eq!(seen.body["messages"][1]["role"], "user");
        assert_eq!(seen.body["messages"][1]["content"], "Fleet: ...");
        Ok(())
    }

    #[test]
    fn ollama_asks_its_server_unstreamed_with_the_sample_as_seed() -> Result<()> {
        let server = FakeServer::start(
            200,
            r#"{"message": {"role": "assistant", "content": "ok"}, "done": true}"#,
        );
        let client = Client::Ollama {
            url: server.url.clone(),
        };
        assert_eq!(
            client.ask_once("llama3", "Dispatch.", "Fleet: ...", 7)?,
            "ok"
        );
        let seen = server.last();
        assert_eq!(seen.path, "/api/chat");
        assert_eq!(seen.body["model"], "llama3");
        assert_eq!(seen.body["stream"], false);
        assert_eq!(seen.body["options"]["seed"], 7);
        assert_eq!(seen.body["messages"][0]["role"], "system");
        assert_eq!(seen.body["messages"][1]["content"], "Fleet: ...");
        Ok(())
    }

    #[test]
    fn an_error_status_names_the_status_and_what_the_server_said() {
        let server = FakeServer::start(404, r#"{"error": "model 'llama9' not found"}"#);
        let client = Client::Ollama {
            url: server.url.clone(),
        };
        let error = client
            .ask_once("llama9", "Dispatch.", "Fleet: ...", 0)
            .expect_err("a 404 is an error");
        let text = format!("{error:#}");
        assert!(text.contains("404"), "{text}");
        assert!(text.contains("llama9' not found"), "{text}");
    }

    #[test]
    fn the_api_clients_need_their_key_unless_offline() {
        // The key is read from the environment, which a test must not set, so
        // this only checks the message when the variable is absent.
        if std::env::var("OPENAI_API_KEY").is_ok() {
            return;
        }
        let error =
            Client::new(Provider::OpenAi, Some(Via::Api), None, None, false).expect_err("no key");
        assert!(error.to_string().contains("set OPENAI_API_KEY"), "{error}");
        let offline = Client::new(Provider::OpenAi, Some(Via::Api), None, None, true)
            .expect("offline needs no key");
        assert_eq!(offline.name(), "openai-api");
    }

    #[test]
    fn names_and_keys_tell_the_clients_apart_except_the_first() -> Result<()> {
        let claude = Client::new(Provider::Claude, None, None, None, false)?;
        assert_eq!(claude.name(), "claude");
        assert_eq!(claude.key_prefix(), "");
        let codex = Client::new(
            Provider::OpenAi,
            Some(Via::Cli),
            Some("/x/codex"),
            None,
            false,
        )?;
        assert_eq!(codex.name(), "codex");
        assert_eq!(codex.key_prefix(), "codex\n");
        let ollama = Client::new(Provider::Ollama, None, None, Some("box:11434/"), false)?;
        assert!(matches!(&ollama, Client::Ollama { url } if url == "http://box:11434"));
        assert!(Client::new(Provider::Ollama, Some(Via::Cli), None, None, false).is_err());
        Ok(())
    }
}
