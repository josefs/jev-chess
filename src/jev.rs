//! Minimal client for the Jev decision API (TypeSafe AI "System One" model).
//!
//! Jev takes a `state` (free text) plus a set of typed questions and returns
//! typed answers. We only use the `choice` question type, which returns a
//! probability for each declared option.

use std::{collections::BTreeMap, time::Duration};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

pub const DEFAULT_URL: &str = "https://api.typesafe.ai/v1/systemone";
pub const DEFAULT_MODEL: &str = "jev-latest";
const QUESTION_KEY: &str = "move";

pub struct JevClient {
    http: reqwest::blocking::Client,
    url: String,
    api_key: String,
    model: String,
}

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    state: &'a str,
    questions: BTreeMap<&'a str, Question<'a>>,
}

#[derive(Serialize)]
struct Question<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    instructions: &'a str,
    criteria: &'a BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Response {
    answers: BTreeMap<String, Answer>,
}

#[derive(Deserialize)]
struct Answer {
    choice: Option<String>,
    #[serde(default)]
    probabilities: BTreeMap<String, f64>,
}

/// The result of a choice question.
#[derive(Debug)]
pub struct Decision {
    pub choice: String,
    pub probabilities: BTreeMap<String, f64>,
}

impl JevClient {
    pub fn new(url: String, api_key: String, model: String) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            http,
            url,
            api_key,
            model,
        })
    }

    /// Asks Jev to pick one of `options` (key -> description) given `state`.
    pub fn choose(
        &self,
        state: &str,
        instructions: &str,
        options: &BTreeMap<String, String>,
    ) -> Result<Decision> {
        let request = Request {
            model: &self.model,
            state,
            questions: BTreeMap::from([(
                QUESTION_KEY,
                Question {
                    kind: "choice",
                    instructions,
                    criteria: options,
                },
            )]),
        };

        let response = self
            .http
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .context("request to Jev failed")?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().unwrap_or_default();
            return Err(anyhow!("Jev returned {status}: {body}"));
        }

        let mut parsed: Response = response.json().context("invalid Jev response")?;
        let answer = parsed
            .answers
            .remove(QUESTION_KEY)
            .ok_or_else(|| anyhow!("Jev response has no '{QUESTION_KEY}' answer"))?;

        let choice = answer
            .probabilities
            .iter()
            .filter(|(k, _)| options.contains_key(*k))
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(k, _)| k.clone())
            .or(answer.choice)
            .filter(|c| options.contains_key(c))
            .ok_or_else(|| anyhow!("Jev did not return a valid choice"))?;

        Ok(Decision {
            choice,
            probabilities: answer.probabilities,
        })
    }
}
