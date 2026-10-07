//! Minimal client for the Jev decision API (TypeSafe AI "System One" model).
//!
//! Jev takes a `state` (free text) plus a set of typed questions and returns
//! typed answers. We only use the `choice` question type, which returns a
//! probability for each declared option.

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::backend::{Decision, post_json};

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
    /// The versioned model ID that answered, e.g. "jev-1.13.0" for "jev-latest".
    #[serde(default)]
    model: Option<String>,
    answers: BTreeMap<String, Answer>,
}

#[derive(Deserialize)]
struct Answer {
    choice: Option<String>,
    #[serde(default)]
    probabilities: BTreeMap<String, f64>,
}

impl JevClient {
    pub fn new(
        http: reqwest::blocking::Client,
        url: String,
        api_key: String,
        model: String,
    ) -> Self {
        Self {
            http,
            url,
            api_key,
            model,
        }
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

        let response = post_json(&self.http, &self.url, &self.api_key, &request, "Jev")?;
        let mut parsed: Response = response.json().context("invalid Jev response")?;
        let answer = parsed
            .answers
            .remove(QUESTION_KEY)
            .ok_or_else(|| anyhow!("Jev response has no '{QUESTION_KEY}' answer"))?;
        Decision::new(options, answer.probabilities, answer.choice, parsed.model)
    }
}
