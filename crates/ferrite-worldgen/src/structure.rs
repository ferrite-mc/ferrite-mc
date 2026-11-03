//! Candidate selection for vanilla random-spread structure placements.

use crate::{ResourceId, WorldgenData};
use ferrite_noise::{LegacyRandomSource, RandomSource};

impl WorldgenData {
    /// Applies only the structure set's frequency reduction. Other placement checks are separate.
    pub fn structure_set_frequency_allows(
        &self,
        id: &ResourceId,
        seed: i64,
        x: i32,
        z: i32,
    ) -> Option<bool> {
        let placement = self.structure_sets.get(id)?.get("placement")?;
        let frequency = placement
            .get("frequency")
            .map_or(Some(1.0), |v| v.as_f64().map(|n| n as f32))?;
        if !(0.0..=1.0).contains(&frequency) {
            return None;
        }
        if frequency >= 1.0 {
            return Some(true);
        }
        let method = placement
            .get("frequency_reduction_method")
            .map_or(Some("default"), |v| v.as_str())?;
        let salt = placement
            .get("salt")
            .and_then(|v| v.as_i64())
            .and_then(|n| i32::try_from(n).ok())?;
        let with_salt = |salt: i32| {
            i64::from(x)
                .wrapping_mul(341_873_128_712)
                .wrapping_add(i64::from(z).wrapping_mul(132_897_987_541))
                .wrapping_add(seed)
                .wrapping_add(i64::from(salt))
        };
        Some(match method {
            "default" => LegacyRandomSource::new(with_salt(salt)).next_float() < frequency,
            "legacy_type_1" => {
                let region_x = x >> 4;
                let region_z = z >> 4;
                let mut random =
                    LegacyRandomSource::new(i64::from(region_x ^ region_z.wrapping_shl(4)) ^ seed);
                random.next_int();
                random.next_int_bound((1.0_f32 / frequency) as i32) == 0
            }
            "legacy_type_2" => {
                LegacyRandomSource::new(with_salt(10_387_320)).next_float() < frequency
            }
            "legacy_type_3" => {
                let mut initial = LegacyRandomSource::new(seed);
                let a = initial.next_long();
                let b = initial.next_long();
                let mixed = i64::from(x).wrapping_mul(a) ^ i64::from(z).wrapping_mul(b) ^ seed;
                LegacyRandomSource::new(mixed).next_double() < f64::from(frequency)
            }
            _ => return None,
        })
    }
    /// Returns the random-spread candidate from a loaded structure set.
    /// This does not apply frequency, exclusion, biome or terrain checks.
    pub fn structure_set_candidate(
        &self,
        id: &ResourceId,
        seed: i64,
        chunk_x: i32,
        chunk_z: i32,
    ) -> Option<(i32, i32)> {
        let placement = self.structure_sets.get(id)?.get("placement")?;
        if placement.get("type")?.as_str()? != "minecraft:random_spread" {
            return None;
        }
        let number = |name| i32::try_from(placement.get(name)?.as_i64()?).ok();
        let triangular = match placement
            .get("spread_type")
            .and_then(|value| value.as_str())
        {
            None | Some("linear") => false,
            Some("triangular") => true,
            Some(_) => return None,
        };
        random_spread_candidate(
            seed,
            chunk_x,
            chunk_z,
            number("spacing")?,
            number("separation")?,
            number("salt")?,
            triangular,
        )
    }
}

/// Returns the potential structure chunk for the region containing `(chunk_x, chunk_z)`.
/// Biome checks, frequency reduction, exclusion zones and structure generation are separate.
pub fn random_spread_candidate(
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    spacing: i32,
    separation: i32,
    salt: i32,
    triangular: bool,
) -> Option<(i32, i32)> {
    if !(1..=4096).contains(&spacing) || separation < 0 || separation >= spacing {
        return None;
    }
    let region_x = chunk_x.div_euclid(spacing);
    let region_z = chunk_z.div_euclid(spacing);
    let random_seed = i64::from(region_x)
        .wrapping_mul(341_873_128_712)
        .wrapping_add(i64::from(region_z).wrapping_mul(132_897_987_541))
        .wrapping_add(seed)
        .wrapping_add(i64::from(salt));
    let mut random = LegacyRandomSource::new(random_seed);
    let bound = spacing - separation;
    let mut sample = || {
        let first = random.next_int_bound(bound);
        if triangular {
            (first + random.next_int_bound(bound)) / 2
        } else {
            first
        }
    };
    Some((
        region_x.wrapping_mul(spacing).wrapping_add(sample()),
        region_z.wrapping_mul(spacing).wrapping_add(sample()),
    ))
}
