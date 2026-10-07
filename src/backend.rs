//! The decision APIs that can pick moves, behind one interface.

use std::{collections::BTreeMap, env, time::Duration};

use anyhow::{Result, anyhow};

use crate::{jev, openai};

/// A decision API that can choose one of a set of options.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Provider {
    /// TypeSafe AI's Jev (System One).
    #[default]
    Jev,
    /// OpenAI's Decisions API.
    Openai,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "jev" => Some(Self::Jev),
            "openai" => Some(Self::Openai),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Jev => "jev",
            Self::Openai => "openai",
        }
    }

    /// Environment variable holding this provider's API key.
    pub fn key_var(self) -> &'static str {
        match self {
            Self::Jev => "JEV_API_KEY",
            Self::Openai => "OPENAI_API_KEY",
        }
    }

    /// Environment variables overriding this provider's endpoint and model.
    fn url_var(self) -> &'static str {
        match self {
            Self::Jev => "JEV_API_URL",
            Self::Openai => "OPENAI_DECISIONS_URL",
        }
    }

    fn model_var(self) -> &'static str {
        match self {
            Self::Jev => "JEV_MODEL",
            Self::Openai => "OPENAI_DECISIONS_MODEL",
        }
    }

    fn default_url(self) -> &'static str {
        match self {
            Self::Jev => jev::DEFAULT_URL,
            Self::Openai => openai::DEFAULT_URL,
        }
    }

    fn default_model(self) -> &'static str {
        match self {
            Self::Jev => jev::DEFAULT_MODEL,
            Self::Openai => openai::DEFAULT_MODEL,
        }
    }

    /// Resolves the API key: `explicit`, else this provider's own variable.
    /// Keys are never taken from another provider's variable.
    pub fn api_key(self, explicit: Option<String>) -> Option<String> {
        explicit.or_else(|| non_empty_env(self.key_var()))
    }

    pub fn url(self, explicit: Option<String>) -> String {
        explicit
            .or_else(|| non_empty_env(self.url_var()))
            .unwrap_or_else(|| self.default_url().to_string())
    }

    pub fn model(self, explicit: Option<String>) -> String {
        explicit
            .or_else(|| non_empty_env(self.model_var()))
            .unwrap_or_else(|| self.default_model().to_string())
    }
}

fn non_empty_env(var: &str) -> Option<String> {
    env::var(var).ok().filter(|v| !v.is_empty())
}

/// The result of a choice question.
#[derive(Debug)]
pub struct Decision {
    pub choice: String,
    pub probabilities: BTreeMap<String, f64>,
    /// The versioned model ID reported by the API, if any.
    pub model: Option<String>,
}

impl Decision {
    /// Picks the most likely valid option, falling back to the API's own choice.
    pub fn new(
        options: &BTreeMap<String, String>,
        probabilities: BTreeMap<String, f64>,
        choice: Option<String>,
        model: Option<String>,
    ) -> Result<Self> {
        let choice = probabilities
            .iter()
            .filter(|(k, _)| options.contains_key(*k))
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(k, _)| k.clone())
            .or(choice)
            .filter(|c| options.contains_key(c))
            .ok_or_else(|| anyhow!("the API did not return a valid choice"))?;
        Ok(Self {
            choice,
            probabilities,
            model,
        })
    }
}

pub enum Client {
    Jev(jev::JevClient),
    OpenAi(openai::OpenAiClient),
}

impl Client {
    pub fn new(provider: Provider, url: String, api_key: String, model: String) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(match provider {
            Provider::Jev => Self::Jev(jev::JevClient::new(http, url, api_key, model)),
            Provider::Openai => Self::OpenAi(openai::OpenAiClient::new(http, url, api_key, model)),
        })
    }

    pub fn provider(&self) -> Provider {
        match self {
            Self::Jev(_) => Provider::Jev,
            Self::OpenAi(_) => Provider::Openai,
        }
    }

    /// Asks the API to pick one of `options` (key -> description) given `state`.
    pub fn choose(
        &self,
        state: &str,
        instructions: &str,
        options: &BTreeMap<String, String>,
    ) -> Result<Decision> {
        match self {
            Self::Jev(c) => c.choose(state, instructions, options),
            Self::OpenAi(c) => c.choose(state, instructions, options),
        }
    }
}

/// Sends `body` as JSON with bearer auth and returns the response body, or an
/// error that includes the API's error message.
pub fn post_json(
    http: &reqwest::blocking::Client,
    url: &str,
    api_key: &str,
    body: &impl serde::Serialize,
    api: &str,
) -> Result<reqwest::blocking::Response> {
    let response = http
        .post(url)
        .bearer_auth(api_key)
        .json(body)
        .send()
        .map_err(|e| anyhow!("request to {api} failed: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().unwrap_or_default();
        return Err(anyhow!("{api} returned {status}: {body}"));
    }
    Ok(response)
}
