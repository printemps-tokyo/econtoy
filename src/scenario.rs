//! Scenario files: a run's parameters, shocks, and policy changes in one
//! plain-text file.
//!
//! The format is line based. `#` starts a comment, blank lines are ignored,
//! and every other line is one of:
//!
//! ```text
//! name = Stimulus after a demand slump
//! description = Demand falls, then the government spends more.
//! ticks = 240
//! interest-rate = 3
//! shock = demand:60:-40:24
//! at 60: gov-spending = 35
//! ```
//!
//! `KEY = VALUE` takes any setting accepted by [`Params::set`] (the flag
//! names without `--`). `shock` and `set` may repeat; any other key may
//! appear once. `at TICK: KNOB = VALUE` schedules a policy change, the same
//! as `--shock set:TICK:KNOB:VALUE`.

use crate::{Knob, Params, Shock};

/// The descriptive part of a scenario file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scenario {
    /// The `name` line, if any.
    pub name: Option<String>,
    /// The `description` line, if any.
    pub description: Option<String>,
}

/// Scenarios built into the binary: `(name, file contents)`.
pub const BUILTIN_SCENARIOS: [(&str, &str); 5] = [
    ("balanced", include_str!("../scenarios/balanced.scenario")),
    ("stimulus", include_str!("../scenarios/stimulus.scenario")),
    ("austerity", include_str!("../scenarios/austerity.scenario")),
    ("rate-hike", include_str!("../scenarios/rate-hike.scenario")),
    (
        "deficit-money",
        include_str!("../scenarios/deficit-money.scenario"),
    ),
];

/// Look up a built-in scenario by name.
#[must_use]
pub fn builtin(name: &str) -> Option<&'static str> {
    BUILTIN_SCENARIOS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, text)| *text)
}

/// Apply a scenario file to `params`, returning its name and description.
/// Settings the file does not mention keep their current values; shocks and
/// policy changes are appended.
///
/// # Errors
/// Returns `line N: ...` for the first line that cannot be applied.
pub fn apply(text: &str, params: &mut Params) -> Result<Scenario, String> {
    let mut scenario = Scenario::default();
    let mut seen: Vec<String> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = raw.split('#').next().unwrap_or_default().trim();
        if line.is_empty() {
            continue;
        }
        apply_line(line, params, &mut scenario, &mut seen)
            .map_err(|e| format!("line {line_no}: {e}"))?;
    }
    Ok(scenario)
}

fn apply_line(
    line: &str,
    params: &mut Params,
    scenario: &mut Scenario,
    seen: &mut Vec<String>,
) -> Result<(), String> {
    if let Some(rest) = strip_at(line) {
        let (tick, assignment) = rest
            .split_once(':')
            .ok_or_else(|| "expected 'at TICK: KNOB = VALUE'".to_string())?;
        let tick = tick
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("invalid tick '{}'", tick.trim()))?;
        let (knob, value) = split_assignment(assignment)?;
        let knob = Knob::from_name(knob).ok_or_else(|| {
            format!(
                "'{knob}' cannot change mid-run (policy knobs: {})",
                Knob::ALL.map(Knob::name).join(", ")
            )
        })?;
        let value = value
            .parse::<f64>()
            .map_err(|_| format!("invalid value for {}: '{value}'", knob.name()))?;
        params.shocks.push(Shock::Policy { tick, knob, value });
        return Ok(());
    }

    let (key, value) = split_assignment(line)?;
    let key = key.to_ascii_lowercase().replace('_', "-");
    if key != "shock" && key != "set" {
        if seen.contains(&key) {
            return Err(format!("'{key}' is set more than once"));
        }
        seen.push(key.clone());
    }
    match key.as_str() {
        "name" => scenario.name = Some(value.to_string()),
        "description" => scenario.description = Some(value.to_string()),
        _ => params.set(&key, value)?,
    }
    Ok(())
}

/// `at 60: ...` -> `60: ...`; any other line -> `None`.
fn strip_at(line: &str) -> Option<&str> {
    let (word, rest) = line.split_once(char::is_whitespace)?;
    word.eq_ignore_ascii_case("at").then_some(rest)
}

fn split_assignment(text: &str) -> Result<(&str, &str), String> {
    let (key, value) = text
        .split_once('=')
        .ok_or_else(|| format!("expected 'KEY = VALUE', got '{}'", text.trim()))?;
    let (key, value) = (key.trim(), value.trim());
    if key.is_empty() {
        return Err("missing key before '='".to_string());
    }
    if value.is_empty() {
        return Err(format!("missing value for '{key}'"));
    }
    Ok((key, value))
}
