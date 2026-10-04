//! Prompt templates: how a position and its legal moves are phrased for Jev.

use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The prompt used when no `--prompt` is given: the best variant so far.
const DEFAULT: &str = include_str!("../prompts/threat-loses.toml");
#[cfg(test)]
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
    "mate",
    "threat",
    "exchange",
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
    #[serde(default)]
    pub labels: Labels,
}

/// Wording of the fixed move labels. Each is shown as ", <text>" in its
/// placeholder; an empty text suppresses the label. May use state placeholders
/// such as {side} and {opponent}.
#[derive(Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct Labels {
    pub check: String,
    pub checkmate: String,
    pub stalemate: String,
    pub repetition: String,
    pub repetition_draw: String,
    pub threat: String,
}

impl Default for Labels {
    fn default() -> Self {
        Self {
            check: "gives check".into(),
            checkmate: "delivers checkmate".into(),
            stalemate: "stalemates (draw)".into(),
            repetition: "repeats an earlier position".into(),
            repetition_draw: "repeats the position a third time (draw by repetition)".into(),
            threat: "allows checkmate in one".into(),
        }
    }
}

impl Labels {
    fn fields_mut(&mut self) -> [(&'static str, &mut String); 6] {
        [
            ("check", &mut self.check),
            ("checkmate", &mut self.checkmate),
            ("stalemate", &mut self.stalemate),
            ("repetition", &mut self.repetition),
            ("repetition_draw", &mut self.repetition_draw),
            ("threat", &mut self.threat),
        ]
    }

    /// Renders state placeholders and adds the ", " separator.
    pub fn render(&self, vars: &Vars) -> Labels {
        let mut out = self.clone();
        for (_, text) in out.fields_mut() {
            if !text.is_empty() {
                *text = format!(", {}", render(text, vars));
            }
        }
        out
    }
}

impl Prompt {
    pub fn builtin() -> Self {
        Self::parse(DEFAULT).expect("built-in default prompt is valid")
    }

    #[cfg(test)]
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
        let mut labels = p.labels.clone();
        for (name, text) in labels.fields_mut() {
            check_placeholders(&format!("labels.{name}"), text, STATE_VARS)?;
        }
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
    fn labels_override_and_render() {
        let p = Prompt::parse(
            "state = \"s\"\ninstructions = \"i\"\noption = \"{uci}{threat}\"\n\
             [labels]\nthreat = \"blunder: lets {opponent} win\"\ncheck = \"\"\n",
        )
        .unwrap();
        let vars = Vars::from([("opponent", "Black".to_string())]);
        let labels = p.labels.render(&vars);
        assert_eq!(labels.threat, ", blunder: lets Black win");
        assert_eq!(labels.check, "");
        assert_eq!(labels.checkmate, ", delivers checkmate");
        assert_eq!(Prompt::baseline().labels, Labels::default());
        assert!(
            Prompt::parse("state=\"\"\ninstructions=\"\"\noption=\"\"\n[labels]\nthreat=\"{uci}\"")
                .is_err()
        );
        assert!(
            Prompt::parse("state=\"\"\ninstructions=\"\"\noption=\"\"\n[labels]\nbogus=\"x\"")
                .is_err()
        );
    }

    #[test]
    fn baseline_parses() {
        Prompt::baseline();
        Prompt::builtin();
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
