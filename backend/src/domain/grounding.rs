//! Grounding check for agent replies: every number in the reply must match a
//! number in that turn's tool results. Pure functions, no I/O.
//!
//! Numbers are read as unsigned decimals ("1,738" = 1738, "−3" = 3), so signs,
//! ranges ("65–70"), and thousands separators don't matter. Digits attached to
//! letters ("eCO2", "PM2.5", "8h") are ignored when the letter comes first; a
//! number followed by a unit ("8h", "70°F") still counts.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

/// A number not preceded by a letter, digit, or dot.
static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[^A-Za-z0-9.])([0-9]{1,3}(?:,[0-9]{3})+(?:\.[0-9]+)?|[0-9]+(?:\.[0-9]+)?)")
        .unwrap()
});

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Grounding {
    /// True when every number in the reply appears in a tool result.
    pub verified: bool,
    /// How many numbers the reply contains.
    pub checked: usize,
    /// Numbers in the reply that no tool result contains, as written.
    pub unmatched: Vec<String>,
}

/// Unsigned numbers in `text`, as (written form, value).
pub fn extract_numbers(text: &str) -> Vec<(String, f64)> {
    NUMBER
        .captures_iter(text)
        .filter_map(|c| {
            let written = c.get(1)?.as_str();
            let value = written.replace(',', "").parse().ok()?;
            Some((written.to_string(), value))
        })
        .collect()
}

/// Checks `reply` against the numbers in `tool_results` (including numbers
/// inside strings, e.g. "8h 50m").
pub fn check(reply: &str, tool_results: &[Value]) -> Grounding {
    let known: Vec<f64> = tool_results
        .iter()
        .flat_map(|r| extract_numbers(&r.to_string()))
        .map(|(_, v)| v)
        .collect();
    let numbers = extract_numbers(reply);
    let unmatched: Vec<String> = numbers
        .iter()
        .filter(|(_, v)| !known.iter().any(|k| (k - v).abs() < 1e-9))
        .map(|(written, _)| written.clone())
        .collect();
    Grounding { verified: unmatched.is_empty(), checked: numbers.len(), unmatched }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn values(text: &str) -> Vec<f64> {
        extract_numbers(text).into_iter().map(|(_, v)| v).collect()
    }

    #[test]
    fn extracts_plain_decimal_and_grouped_numbers() {
        assert_eq!(values("Score 77.1, eCO2 1,738 ppm, 97.9%"), vec![77.1, 1738.0, 97.9]);
        assert_eq!(values("65–70 °F and -3 and −4"), vec![65.0, 70.0, 3.0, 4.0]);
        assert_eq!(values("8h 50m, 70°F"), vec![8.0, 50.0, 70.0]);
        assert_eq!(values("It ended at 70."), vec![70.0]);
    }

    #[test]
    fn ignores_digits_attached_to_leading_letters() {
        assert_eq!(values("eCO2 and CO₂ and PM2.5 and SCD41"), Vec::<f64>::new());
        assert_eq!(values("estimated (eCO₂) 450 ppm"), vec![450.0]);
    }

    #[test]
    fn verifies_numbers_found_in_tool_results() {
        let results = [
            json!({"temp_f": 75.9, "score": 77.1, "eco2_ppm": {"max": 726}}),
            json!({"time_in_sleep_mode": "8h 50m", "temp_f": {"target_min": 65, "target_max": 70}}),
        ];
        let reply = "Last night scored 77.1 over 8h 50m. Temperature averaged 75.9 °F \
                     (target 65–70 °F); estimated CO₂ (eCO₂) peaked at 726 ppm.";
        let g = check(reply, &results);
        assert_eq!(g, Grounding { verified: true, checked: 7, unmatched: vec![] });
    }

    #[test]
    fn flags_numbers_not_in_tool_results() {
        let results = [json!({"temp_f": 75.9})];
        let g = check("It was 76 °F, about 6 degrees warm.", &results);
        assert!(!g.verified);
        assert_eq!(g.unmatched, vec!["76", "6"]);
    }

    #[test]
    fn whole_and_decimal_forms_match() {
        let results = [json!({"score": 100.0, "eco2_ppm": 1738})];
        assert!(check("100 and 1,738 and 100.0", &results).verified);
    }

    #[test]
    fn reply_without_numbers_is_verified() {
        let g = check("No readings yet.", &[]);
        assert_eq!(g, Grounding { verified: true, checked: 0, unmatched: vec![] });
    }

    #[test]
    fn numbers_in_json_keys_are_not_known() {
        // "eco2_ppm" must not make "2" a known number.
        assert!(!check("2", &[json!({"eco2_ppm": 400})]).verified);
    }
}
