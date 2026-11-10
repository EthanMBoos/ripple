//! The language model as fleet manager. It reads the situation the dispatcher
//! writes (`dispatcher.rs`) and answers with orders (`order.rs`).
//!
//! Replies are kept in one folder, one readable JSON file per prompt, named by
//! a hash of the client, the model, the sample index, the system prompt and
//! the prompt. A rerun sends the same prompts, finds every reply in the
//! folder, and so replays the whole day exactly without calling the model.
//! `sample` selects an independent reply to the same prompt.

use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::client::Client;
use crate::order::{Order, Reply, parse_reply};

#[derive(Clone)]
pub struct ModelManager {
    pub model: String,
    pub sample: u32,
    /// The system prompt, frozen in the mission.
    pub system: String,
    pub cache_dir: PathBuf,
    pub client: Client,
    /// Use only the replies already in the folder; a missing one is an error.
    pub offline: bool,
}

/// What one call produced.
#[derive(Debug, Default)]
pub struct Asked {
    pub orders: Vec<Order>,
    /// Orders in the reply that did not read.
    pub unreadable: u32,
    /// The final reply could not be read at all.
    pub parse_failed: bool,
    pub prompt_chars: usize,
    pub reply_chars: usize,
}

#[derive(Serialize, Deserialize)]
struct CachedReply {
    client: String,
    model: String,
    sample: u32,
    system: String,
    prompt: String,
    reply: String,
}

impl ModelManager {
    /// One call: the situation in, the orders out.
    pub fn ask(&self, situation: &str) -> Result<Asked> {
        let reply = self.reply(situation)?;
        let mut asked = Asked {
            prompt_chars: situation.len(),
            reply_chars: reply.len(),
            ..Asked::default()
        };
        match parse_reply(&reply) {
            Reply::Orders { orders, unreadable } => {
                asked.orders = orders;
                asked.unreadable = unreadable;
            }
            Reply::Unreadable => asked.parse_failed = true,
        }
        Ok(asked)
    }

    /// One call, the reply as written: for the veto manager, which reads
    /// it differently.
    pub fn ask_text(&self, prompt: &str) -> Result<String> {
        self.reply(prompt)
    }

    fn reply(&self, prompt: &str) -> Result<String> {
        let key = Sha256::digest(
            format!(
                "{}{}\n{}\n{}\n{prompt}",
                self.client.key_prefix(),
                self.model,
                self.sample,
                self.system
            )
            .as_bytes(),
        );
        let hex: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
        let path = self.cache_dir.join(format!("{hex}.json"));
        let read_cached = || -> Result<String> {
            let cached: CachedReply = serde_json::from_str(&std::fs::read_to_string(&path)?)
                .with_context(|| format!("reading cached reply {}", path.display()))?;
            ensure!(
                cached.client == self.client.name(),
                "cached reply {} is from client {}, not {}",
                path.display(),
                cached.client,
                self.client.name()
            );
            Ok(cached.reply)
        };
        if path.exists() {
            return read_cached();
        }
        ensure!(
            !self.offline,
            "no saved reply {} (offline: true)",
            path.display()
        );
        let reply = self
            .client
            .ask(&self.model, &self.system, prompt, self.sample)?;
        std::fs::create_dir_all(&self.cache_dir)?;
        let cached = CachedReply {
            client: self.client.name().into(),
            model: self.model.clone(),
            sample: self.sample,
            system: self.system.clone(),
            prompt: prompt.into(),
            reply: reply.clone(),
        };
        // Write, then link into place, so a crash never leaves half a reply in
        // the cache, and two runs asking the same prompt at once keep one.
        let partial = path.with_extension(format!(
            "part{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&partial, serde_json::to_string_pretty(&cached)?)?;
        let linked = std::fs::hard_link(&partial, &path);
        std::fs::remove_file(&partial)?;
        match linked {
            Ok(()) => Ok(reply),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => read_cached(),
            Err(error) => Err(error.into()),
        }
    }
}
