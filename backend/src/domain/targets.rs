//! The scoring targets and thresholds, as reported by the agent's get_targets
//! tool. Built from the same constants the scoring uses. Pure, no I/O.
//!
//! `sources` name the studies behind each target (full citations and what each
//! supports: docs/REFERENCES.md). Only author-year labels go to the model; the
//! years become "known" numbers for the grounding check, which is acceptable.

use serde_json::{json, Value};

use super::reading::WARM_UP_SECS;
use super::scoring::{
    ECO2_FULL_PPM, ECO2_ZERO_PPM, HUMIDITY_POINTS_PER_PCT, HUMIDITY_TARGET_PCT,
    TEMP_POINTS_PER_F, TEMP_TARGET_F,
};
use super::sleep::{INCOMPLETE_BELOW_PCT, SHORT_SESSION_SECS};

pub fn targets() -> Value {
    let (temp_min, temp_max) = TEMP_TARGET_F;
    let (rh_min, rh_max) = HUMIDITY_TARGET_PCT;
    json!({
        "eco2_ppm": {
            "note": "estimated CO2 (eCO2) from a VOC sensor",
            "full_points_at_or_below": ECO2_FULL_PPM,
            "zero_points_at_or_above": ECO2_ZERO_PPM,
            "sources": ["Fan et al., 2022 (window/door opening)", "Fan et al., 2022 (ventilation and temperature)", "Kang et al., 2024", "Yan et al., 2024"],
        },
        "temp_f": {
            "target_min": temp_min,
            "target_max": temp_max,
            "points_lost_per_degree_outside": TEMP_POINTS_PER_F,
            "zero_points_at_or_below": temp_min - 100.0 / TEMP_POINTS_PER_F,
            "zero_points_at_or_above": temp_max + 100.0 / TEMP_POINTS_PER_F,
            "sources": ["Fan et al., 2022 (ventilation and temperature)", "Okamoto-Mizuno et al., 1999"],
        },
        "humidity_pct": {
            "target_min": rh_min,
            "target_max": rh_max,
            "points_lost_per_percent_outside": HUMIDITY_POINTS_PER_PCT,
            "zero_points_at_or_below": rh_min - 100.0 / HUMIDITY_POINTS_PER_PCT,
            "zero_points_at_or_above": rh_max + 100.0 / HUMIDITY_POINTS_PER_PCT,
            "sources": ["Arundel et al., 1986", "US EPA mold and moisture guide", "Okamoto-Mizuno et al., 1999"],
        },
        "score": "average of the three 0-100 sub-scores",
        "bands": { "great_min": 90, "good_min": 80, "fair_min": 70, "poor": "below 70" },
        "incomplete_night_below_pct": INCOMPLETE_BELOW_PCT,
        "short_session_under_minutes": SHORT_SESSION_SECS / 60,
        "sensor_warm_up_minutes": WARM_UP_SECS / 60,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_scoring_spec() {
        let t = targets();
        assert_eq!(t["eco2_ppm"]["full_points_at_or_below"], 800.0);
        assert_eq!(t["eco2_ppm"]["zero_points_at_or_above"], 2000.0);
        assert_eq!(t["temp_f"]["target_min"], 65.0);
        assert_eq!(t["temp_f"]["target_max"], 70.0);
        assert_eq!(t["temp_f"]["zero_points_at_or_below"], 55.0);
        assert_eq!(t["temp_f"]["zero_points_at_or_above"], 80.0);
        assert_eq!(t["humidity_pct"]["zero_points_at_or_below"], 20.0);
        assert_eq!(t["humidity_pct"]["target_min"], 40.0);
        assert_eq!(t["humidity_pct"]["target_max"], 60.0);
        assert_eq!(t["humidity_pct"]["zero_points_at_or_above"], 80.0);
        assert_eq!(t["eco2_ppm"]["sources"][2], "Kang et al., 2024");
        assert_eq!(t["humidity_pct"]["sources"][0], "Arundel et al., 1986");
        assert_eq!(t["incomplete_night_below_pct"], 60.0);
        assert_eq!(t["short_session_under_minutes"], 60);
        assert_eq!(t["sensor_warm_up_minutes"], 20);
    }
}
