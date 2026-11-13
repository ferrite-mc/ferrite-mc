//! Number providers and integer ranges used by rolls, counts and limits.

use crate::Rng;
use serde_json::Value;

/// A loot number provider: a constant, a uniform integer range, or a binomial
/// draw.
///
/// Vanilla 26.3 only writes `constant` as a bare JSON number, but the explicit
/// `{"type": "minecraft:constant", "value": ...}` form is accepted too.
#[derive(Debug, Clone, PartialEq)]
pub enum NumberProvider {
    /// A fixed value.
    Constant(f64),
    /// A uniform integer in `min..=max`.
    Uniform {
        /// Inclusive lower bound.
        min: f64,
        /// Inclusive upper bound.
        max: f64,
    },
    /// The number of successes in `n` trials with probability `p`.
    Binomial {
        /// Number of trials.
        n: i32,
        /// Success probability in `[0, 1]`.
        p: f32,
    },
}

impl NumberProvider {
    /// A provider that always yields `value`.
    pub(crate) fn constant(value: f64) -> Self {
        Self::Constant(value)
    }

    /// Parses one provider. Missing or unrecognized providers become zero.
    pub(crate) fn parse(value: Option<&Value>) -> Self {
        let Some(value) = value else {
            return Self::Constant(0.0);
        };
        let Some(object) = value.as_object() else {
            return Self::Constant(value.as_f64().unwrap_or(0.0));
        };

        let kind = object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("minecraft:constant");
        match kind {
            "minecraft:uniform" => Self::Uniform {
                min: number(object.get("min")),
                max: number(object.get("max")),
            },
            "minecraft:binomial" => Self::Binomial {
                n: integer(object.get("n")),
                p: number(object.get("p")) as f32,
            },
            "minecraft:constant" => Self::Constant(number(object.get("value"))),
            _ => Self::Constant(0.0),
        }
    }

    /// Draws a float: constants as written, `uniform` as `min + nextFloat * (max - min)`
    /// (vanilla `UniformGenerator.getFloat`), `binomial` as its integer draw.
    pub fn get_float(&self, rng: &mut impl Rng) -> f32 {
        match self {
            Self::Constant(value) => *value as f32,
            Self::Uniform { min, max } => {
                let (min, max) = (*min as f32, *max as f32);
                if max <= min {
                    min
                } else {
                    min + rng.next_float() * (max - min)
                }
            }
            Self::Binomial { .. } => self.get_int(rng) as f32,
        }
    }

    /// Draws an integer from this provider.
    pub fn get_int(&self, rng: &mut impl Rng) -> i32 {
        match self {
            Self::Constant(value) => *value as i32,
            Self::Uniform { min, max } => {
                let min = *min as i32;
                let max = *max as i32;
                if max <= min {
                    return min;
                }
                let span = (i64::from(max) - i64::from(min) + 1).min(i64::from(i32::MAX)) as i32;
                min + rng.next_int(span)
            }
            Self::Binomial { n, p } => {
                let trials = (*n).max(0);
                let probability = p.clamp(0.0, 1.0);
                let mut successes = 0;
                for _ in 0..trials {
                    if rng.next_float() < probability {
                        successes += 1;
                    }
                }
                successes
            }
        }
    }
}

/// An inclusive integer range with either bound optional.
///
/// `limit_count` uses this to clamp a stack: `{"min": 0}` leaves counts alone,
/// `{"max": 9}` caps them, and `{"min": 1, "max": 4}` clamps both ends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IntRange {
    /// Inclusive lower bound, if any.
    pub min: Option<i32>,
    /// Inclusive upper bound, if any.
    pub max: Option<i32>,
}

impl IntRange {
    /// Parses a range. Bare numbers are exact values.
    pub(crate) fn parse(value: Option<&Value>) -> Self {
        let Some(value) = value else {
            return Self::default();
        };
        match value {
            Value::Number(number) => {
                let value = number.as_i64().map(|value| value as i32);
                Self {
                    min: value,
                    max: value,
                }
            }
            Value::Object(object) => Self {
                min: object
                    .get("min")
                    .and_then(Value::as_i64)
                    .map(|value| value as i32),
                max: object
                    .get("max")
                    .and_then(Value::as_i64)
                    .map(|value| value as i32),
            },
            _ => Self::default(),
        }
    }

    /// Clamps `value` to this range.
    pub fn clamp(&self, value: i32) -> i32 {
        let value = match self.min {
            Some(min) => value.max(min),
            None => value,
        };
        match self.max {
            Some(max) => value.min(max),
            None => value,
        }
    }
}

fn number(value: Option<&Value>) -> f64 {
    value.and_then(Value::as_f64).unwrap_or(0.0)
}

fn integer(value: Option<&Value>) -> i32 {
    value.and_then(Value::as_i64).unwrap_or(0) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(u64);

    impl Rng for Fixed {
        fn next_int(&mut self, bound: i32) -> i32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            ((self.0 >> 33) % bound.max(1) as u64) as i32
        }

        fn next_float(&mut self) -> f32 {
            self.next_int(1 << 24) as f32 / (1 << 24) as f32
        }
    }

    #[test]
    fn bare_numbers_are_constants() {
        let provider = NumberProvider::parse(Some(&serde_json::json!(4)));
        assert_eq!(provider.get_int(&mut Fixed(1)), 4);
    }

    #[test]
    fn uniform_stays_in_range() {
        let provider = NumberProvider::parse(Some(&serde_json::json!({
            "type": "minecraft:uniform",
            "min": 2,
            "max": 5
        })));
        let mut rng = Fixed(3);
        for _ in 0..100 {
            let value = provider.get_int(&mut rng);
            assert!((2..=5).contains(&value));
        }
    }

    #[test]
    fn binomial_counts_trials() {
        let provider = NumberProvider::parse(Some(&serde_json::json!({
            "type": "minecraft:binomial",
            "n": 10,
            "p": 0.5
        })));
        let mut rng = Fixed(7);
        for _ in 0..100 {
            let value = provider.get_int(&mut rng);
            assert!((0..=10).contains(&value));
        }
    }

    #[test]
    fn ranges_clamp_either_bound() {
        let range = IntRange::parse(Some(&serde_json::json!({"min": 1, "max": 4})));
        assert_eq!(range.clamp(0), 1);
        assert_eq!(range.clamp(3), 3);
        assert_eq!(range.clamp(9), 4);

        let range = IntRange::parse(Some(&serde_json::json!({"max": 2})));
        assert_eq!(range.clamp(9), 2);
        assert_eq!(range.clamp(0), 0);
    }
}
