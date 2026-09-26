//! Rounding for everything the API, SSE stream, and agent tools report:
//! eCO₂ and TVOC as whole numbers; temperature, humidity, and scores to 1 decimal.
//! Scores are rounded in the domain (so bands match the shown number); the
//! serde helpers here round sensor values and statistics as they are serialized.

use serde::{Serialize, Serializer};

pub fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Precision {
    /// Serialized as an integer.
    Whole,
    /// Serialized as a float with 1 decimal.
    Tenths,
}

/// A number serialized at a given precision.
pub struct Rounded(pub f64, pub Precision);

impl Serialize for Rounded {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.1 {
            Precision::Whole => s.serialize_i64(self.0.round() as i64),
            Precision::Tenths => s.serialize_f64(round1(self.0)),
        }
    }
}

pub fn whole<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    Rounded(*v, Precision::Whole).serialize(s)
}

pub fn tenths<S: Serializer>(v: &f64, s: S) -> Result<S::Ok, S::Error> {
    Rounded(*v, Precision::Tenths).serialize(s)
}

pub fn tenths_opt<S: Serializer>(v: &Option<f64>, s: S) -> Result<S::Ok, S::Error> {
    v.map(|v| Rounded(v, Precision::Tenths)).serialize(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_to_one_decimal() {
        assert_eq!(round1(33.599999999999994), 33.6);
        assert_eq!(round1(76.66666666666667), 76.7);
        assert_eq!(round1(49.800000000000004), 49.8);
        assert_eq!(round1(23.0), 23.0);
    }

    #[test]
    fn serializes_at_precision() {
        let json = |r: Rounded| serde_json::to_string(&r).unwrap();
        assert_eq!(json(Rounded(477.4, Precision::Whole)), "477");
        assert_eq!(json(Rounded(477.5, Precision::Whole)), "478");
        assert_eq!(json(Rounded(76.64, Precision::Tenths)), "76.6");
        assert_eq!(json(Rounded(100.0, Precision::Tenths)), "100.0");
    }
}
