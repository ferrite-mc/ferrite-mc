//! Candidate selection for vanilla random-spread structure placements.

use crate::{ResourceId, WorldgenData};
use ferrite_noise::{LegacyRandomSource, RandomSource};

impl WorldgenData {
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
