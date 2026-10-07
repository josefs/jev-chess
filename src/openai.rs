//! Minimal client for OpenAI's Decisions API (`POST /v1/decisions`).
//!
//! Like Jev, it takes shared input plus typed questions; we only use a single
//! `choice` question, whose answer has a probability for each option.

use std::collections::BTreeMap;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::backend::{Decision, post_json};

pub const DEFAULT_URL: &str = "https://api.openai.com/v1/decisions";
pub const DEFAULT_MODEL: &str = "gpt-6-luna";
const QUESTION_NAME: &str = "move";

pub struct OpenAiClient {
    http: reqwest::blocking::Client,
    url: String,
    api_key: String,
    model: String,
}

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    input: &'a str,
    questions: [Question<'a>; 1],
}

#[derive(Serialize)]
struct Question<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    name: &'a str,
    instructions: &'a str,
    choices: Vec<Choice<'a>>,
}

#[derive(Serialize)]
struct Choice<'a> {
    value: &'a str,
    description: &'a str,
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    model: Option<String>,
    answers: Vec<Answer>,
}

#[derive(Deserialize)]
struct Answer {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    choice: Option<Value>,
    #[serde(default)]
    probabilities: Vec<Probability>,
}

#[derive(Deserialize)]
struct Probability {
    // Choice values may be strings or booleans; ours are always strings.
    value: Value,
    probability: f64,
}

impl OpenAiClient {
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

    pub fn choose(
        &self,
        state: &str,
        instructions: &str,
        options: &BTreeMap<String, String>,
    ) -> Result<Decision> {
        let request = Request {
            model: &self.model,
            input: state,
            questions: [Question {
                kind: "choice",
                name: QUESTION_NAME,
                instructions,
                choices: options
                    .iter()
                    .map(|(value, description)| Choice { value, description })
                    .collect(),
            }],
        };

        let response = post_json(&self.http, &self.url, &self.api_key, &request, "OpenAI")?;
        let parsed: Response = response.json().context("invalid OpenAI response")?;
        let answer = parsed
            .answers
            .into_iter()
            .find(|a| a.name.as_deref().is_none_or(|n| n == QUESTION_NAME))
            .ok_or_else(|| anyhow!("OpenAI response has no '{QUESTION_NAME}' answer"))?;
        if answer.kind == "refusal" {
            return Err(anyhow!("OpenAI refused to answer"));
        }
        if answer.kind != "choice" {
            return Err(anyhow!("unexpected OpenAI answer type '{}'", answer.kind));
        }

        let probabilities = answer
            .probabilities
            .into_iter()
            .filter_map(|p| Some((p.value.as_str()?.to_string(), p.probability)))
            .collect();
        let choice = answer.choice.and_then(|c| c.as_str().map(str::to_string));
        Decision::new(options, probabilities, choice, parsed.model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("e2e4".to_string(), "e4: pawn from e2 to e4".to_string()),
            ("g1f3".to_string(), "Nf3: knight from g1 to f3".to_string()),
        ])
    }

    #[test]
    fn request_matches_the_documented_shape() {
        let opts = options();
        let request = Request {
            model: DEFAULT_MODEL,
            input: "state",
            questions: [Question {
                kind: "choice",
                name: QUESTION_NAME,
                instructions: "pick",
                choices: opts
                    .iter()
                    .map(|(value, description)| Choice { value, description })
                    .collect(),
            }],
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "model": "gpt-6-luna",
                "input": "state",
                "questions": [{
                    "type": "choice",
                    "name": "move",
                    "instructions": "pick",
                    "choices": [
                        {"value": "e2e4", "description": "e4: pawn from e2 to e4"},
                        {"value": "g1f3", "description": "Nf3: knight from g1 to f3"},
                    ],
                }],
            })
        );
    }

    #[test]
    fn parses_choice_answers() {
        let body = r#"{
            "model": "gpt-6-luna-2026-09-30",
            "answers": [{
                "type": "choice", "name": "move", "choice": "g1f3", "confidence": 0.5,
                "probabilities": [
                    {"value": "e2e4", "probability": 0.7},
                    {"value": "g1f3", "probability": 0.3}
                ]
            }],
            "usage": {}
        }"#;
        let parsed: Response = serde_json::from_str(body).unwrap();
        assert_eq!(parsed.model.as_deref(), Some("gpt-6-luna-2026-09-30"));
        let answer = &parsed.answers[0];
        assert_eq!(answer.kind, "choice");
        assert_eq!(answer.probabilities.len(), 2);
        let probabilities = parsed.answers.into_iter().next().unwrap().probabilities;
        let probabilities: BTreeMap<_, _> = probabilities
            .into_iter()
            .map(|p| (p.value.as_str().unwrap().to_string(), p.probability))
            .collect();
        let decision = Decision::new(&options(), probabilities, Some("g1f3".into()), None).unwrap();
        assert_eq!(decision.choice, "e2e4");
    }

    #[test]
    fn parses_refusals() {
        let parsed: Response = serde_json::from_str(
            r#"{"model": "m", "answers": [{"type": "refusal", "name": "move"}]}"#,
        )
        .unwrap();
        assert_eq!(parsed.answers[0].kind, "refusal");
    }
}
