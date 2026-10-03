//! Prompt templates: how a position and its legal moves are phrased for Jev.

use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const BASELINE: &str = include_str!("../prompts/baseline.toml");

pub const STATE_VARS: &[&str] = &[
    "side", "opponent", "fen", "board", "moves", "history", "check", "balance",
];
pub const OPTION_VARS: &[&str] = &[
    "uci",
    "san",
    "san_plain",
    "piece",
    "from",
    "to",
    "details",
    "capture",
    "promotion",
    "castling",
    "gives_check",
    "safety",
    "hanging",
    "attacks",
    "material",
    "repetition",
];

pub type Vars = BTreeMap<&'static str, String>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prompt {
    pub state: String,
    pub instructions: String,
    pub option: String,
}

impl Prompt {
    pub fn baseline() -> Self {
        Self::parse(BASELINE).expect("built-in baseline prompt is valid")
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("cannot read prompt file {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("invalid prompt file {}", path.display()))
    }

    fn parse(text: &str) -> Result<Self> {
        let p: Prompt = toml::from_str(text)?;
        check_placeholders("state", &p.state, STATE_VARS)?;
        check_placeholders("instructions", &p.instructions, STATE_VARS)?;
        check_placeholders("option", &p.option, OPTION_VARS)?;
        Ok(p)
    }

    pub fn state(&self, vars: &Vars) -> String {
        render(&self.state, vars)
    }

    pub fn instructions(&self, vars: &Vars) -> String {
        render(&self.instructions, vars)
    }

    pub fn option(&self, vars: &Vars) -> String {
        render(&self.option, vars)
    }
}

fn render(template: &str, vars: &Vars) -> String {
    vars.iter().fold(template.to_string(), |s, (k, v)| {
        s.replace(&format!("{{{k}}}"), v)
    })
}

/// Rejects `{name}` placeholders that are not in `allowed`, to catch typos.
fn check_placeholders(field: &str, template: &str, allowed: &[&str]) -> Result<()> {
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('}') else { break };
        let name = &rest[..end];
        if !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !allowed.contains(&name)
        {
            bail!(
                "unknown placeholder {{{name}}} in `{field}` (available: {})",
                allowed.join(", ")
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_parses() {
        Prompt::baseline();
    }

    #[test]
    fn rejects_unknown_placeholder() {
        let err = Prompt::parse("state = '{sid}'\ninstructions = ''\noption = ''")
            .err()
            .unwrap();
        assert!(err.to_string().contains("{sid}"));
    }

    #[test]
    fn renders() {
        let p = Prompt::parse("state = '{side} vs {opponent}'\ninstructions = ''\noption = ''")
            .unwrap();
        let vars = Vars::from([("side", "White".into()), ("opponent", "Black".into())]);
        assert_eq!(p.state(&vars), "White vs Black");
    }
}
