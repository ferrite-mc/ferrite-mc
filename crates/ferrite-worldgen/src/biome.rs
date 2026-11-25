//! Multi-noise biome source in 6D climate parameter space with R-tree acceleration.

use crate::density::{DensityFunction, EvalCache, FunctionContext, NoiseMap};

/// A 1D parameter range `[min, max]` in climate space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parameter {
    pub min: f32,
    pub max: f32,
}

impl Parameter {
    #[inline]
    pub const fn point(v: f32) -> Self {
        Self { min: v, max: v }
    }

    #[inline]
    pub const fn span(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    #[inline]
    pub fn distance(self, v: f32) -> f32 {
        if v > self.max {
            v - self.max
        } else if v < self.min {
            self.min - v
        } else {
            0.0
        }
    }

    #[inline]
    pub fn union(self, other: Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }
}

/// 6D climate parameter hypercube for a biome entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParameterPoint {
    pub temperature: Parameter,
    pub humidity: Parameter,
    pub continentalness: Parameter,
    pub erosion: Parameter,
    pub depth: Parameter,
    pub weirdness: Parameter,
    pub offset: f32,
}

impl ParameterPoint {
    /// Squared Euclidean distance from `target` to this hypercube, including `offset^2`.
    #[inline]
    pub fn fitness(&self, target: [f32; 6]) -> f32 {
        let d0 = self.temperature.distance(target[0]);
        let d1 = self.humidity.distance(target[1]);
        let d2 = self.continentalness.distance(target[2]);
        let d3 = self.erosion.distance(target[3]);
        let d4 = self.depth.distance(target[4]);
        let d5 = self.weirdness.distance(target[5]);
        d0 * d0 + d1 * d1 + d2 * d2 + d3 * d3 + d4 * d4 + d5 * d5 + self.offset * self.offset
    }

    #[inline]
    fn as_array(&self) -> [Parameter; 7] {
        [
            self.temperature,
            self.humidity,
            self.continentalness,
            self.erosion,
            self.depth,
            self.weirdness,
            Parameter::point(self.offset),
        ]
    }
}

/// Climate sampler evaluating the 6 climate density functions.
#[derive(Debug, Clone)]
pub struct ClimateSampler {
    pub temperature: DensityFunction,
    pub humidity: DensityFunction,
    pub continentalness: DensityFunction,
    pub erosion: DensityFunction,
    pub depth: DensityFunction,
    pub weirdness: DensityFunction,
}

impl ClimateSampler {
    /// Pre-binds noise pointers on all 6 climate density functions.
    pub fn bind_noises(&mut self, noises: &NoiseMap) {
        self.temperature.bind_noises(noises);
        self.humidity.bind_noises(noises);
        self.continentalness.bind_noises(noises);
        self.erosion.bind_noises(noises);
        self.depth.bind_noises(noises);
        self.weirdness.bind_noises(noises);
    }

    /// Returns `true` if any climate density function depends on `ctx.y`.
    pub fn depends_on_y(&self) -> bool {
        self.temperature.depends_on_y()
            || self.humidity.depends_on_y()
            || self.continentalness.depends_on_y()
            || self.erosion.depends_on_y()
            || self.depth.depends_on_y()
            || self.weirdness.depends_on_y()
    }

    pub fn sample(&self, ctx: FunctionContext, noises: &NoiseMap) -> [f32; 6] {
        [
            self.temperature.compute(ctx, noises) as f32,
            self.humidity.compute(ctx, noises) as f32,
            self.continentalness.compute(ctx, noises) as f32,
            self.erosion.compute(ctx, noises) as f32,
            self.depth.compute(ctx, noises) as f32,
            self.weirdness.compute(ctx, noises) as f32,
        ]
    }

    pub fn sample_cached(
        &self,
        ctx: FunctionContext,
        noises: &NoiseMap,
        cache: &mut EvalCache,
    ) -> [f32; 6] {
        [
            self.temperature.compute_cached(ctx, noises, cache) as f32,
            self.humidity.compute_cached(ctx, noises, cache) as f32,
            self.continentalness.compute_cached(ctx, noises, cache) as f32,
            self.erosion.compute_cached(ctx, noises, cache) as f32,
            self.depth.compute_cached(ctx, noises, cache) as f32,
            self.weirdness.compute_cached(ctx, noises, cache) as f32,
        ]
    }

    /// Evaluates the five 2D climate channels (`temperature`, `humidity`, `continentalness`,
    /// `erosion`, `weirdness`) once per `(x, z)` column so vertical slices only need to update `depth`.
    pub fn sample_2d_channels_cached(
        &self,
        ctx: FunctionContext,
        noises: &NoiseMap,
        cache: &mut EvalCache,
    ) -> [f32; 6] {
        [
            self.temperature.compute_cached(ctx, noises, cache) as f32,
            self.humidity.compute_cached(ctx, noises, cache) as f32,
            self.continentalness.compute_cached(ctx, noises, cache) as f32,
            self.erosion.compute_cached(ctx, noises, cache) as f32,
            0.0,
            self.weirdness.compute_cached(ctx, noises, cache) as f32,
        ]
    }

    /// Evaluates `depth` at `ctx` and combines it with precomputed 2D channels.
    #[inline]
    pub fn sample_depth_with_2d(
        &self,
        ctx: FunctionContext,
        noises: &NoiseMap,
        cache: &mut EvalCache,
        mut base_2d: [f32; 6],
    ) -> [f32; 6] {
        base_2d[4] = self.depth.compute_cached(ctx, noises, cache) as f32;
        base_2d
    }

    /// Returns `true` when any channel other than `depth` depends on `Y`.
    pub fn horizontal_channels_depend_on_y(&self) -> bool {
        self.temperature.depends_on_y()
            || self.humidity.depends_on_y()
            || self.continentalness.depends_on_y()
            || self.erosion.depends_on_y()
            || self.weirdness.depends_on_y()
    }
}

#[derive(Debug, Clone)]
enum RTreeNode {
    Leaf {
        bounds: [Parameter; 7],
        entries: Vec<(ParameterPoint, u32)>,
    },
    Branch {
        bounds: [Parameter; 7],
        children: Vec<RTreeNode>,
    },
}

impl RTreeNode {
    fn bounds(&self) -> &[Parameter; 7] {
        match self {
            RTreeNode::Leaf { bounds, .. } => bounds,
            RTreeNode::Branch { bounds, .. } => bounds,
        }
    }

    fn min_dist(&self, target: [f32; 6]) -> f32 {
        let b = self.bounds();
        let d0 = b[0].distance(target[0]);
        let d1 = b[1].distance(target[1]);
        let d2 = b[2].distance(target[2]);
        let d3 = b[3].distance(target[3]);
        let d4 = b[4].distance(target[4]);
        let d5 = b[5].distance(target[5]);
        let d6 = b[6].distance(0.0);
        d0 * d0 + d1 * d1 + d2 * d2 + d3 * d3 + d4 * d4 + d5 * d5 + d6 * d6
    }

    fn search(&self, target: [f32; 6], best: &mut (u32, f32, bool)) {
        if self.min_dist(target) >= best.1 {
            return;
        }
        match self {
            RTreeNode::Leaf { entries, .. } => {
                for &(point, id) in entries {
                    let d = point.fitness(target);
                    if d < best.1 {
                        let is_zero_depth = point.depth.min == 0.0 && point.depth.max == 0.0;
                        *best = (id, d, is_zero_depth);
                    }
                }
            }
            RTreeNode::Branch { children, .. } => {
                for child in children {
                    child.search(target, best);
                }
            }
        }
    }
}

fn union_bounds(items: &[[Parameter; 7]]) -> [Parameter; 7] {
    let mut acc = items[0];
    for b in &items[1..] {
        for i in 0..7 {
            acc[i] = acc[i].union(b[i]);
        }
    }
    acc
}

fn build_rtree(mut entries: Vec<(ParameterPoint, u32)>) -> Option<RTreeNode> {
    if entries.is_empty() {
        return None;
    }
    const MAX_CHILDREN: usize = 8;
    if entries.len() <= MAX_CHILDREN {
        let boxes: Vec<[Parameter; 7]> = entries.iter().map(|(p, _)| p.as_array()).collect();
        return Some(RTreeNode::Leaf {
            bounds: union_bounds(&boxes),
            entries,
        });
    }

    // Split along the dimension with largest total span.
    let boxes: Vec<[Parameter; 7]> = entries.iter().map(|(p, _)| p.as_array()).collect();
    let total = union_bounds(&boxes);
    let mut split_dim = 0usize;
    let mut max_span = -1.0f32;
    for (i, p) in total.iter().enumerate() {
        let span = p.max - p.min;
        if span > max_span {
            max_span = span;
            split_dim = i;
        }
    }
    entries.sort_by(|(a, _), (b, _)| {
        let pa = a.as_array()[split_dim];
        let pb = b.as_array()[split_dim];
        let ca = pa.min + pa.max;
        let cb = pb.min + pb.max;
        ca.total_cmp(&cb)
    });

    let chunk_size = entries.len().div_ceil(MAX_CHILDREN);
    let mut children = Vec::new();
    while !entries.is_empty() {
        let take = chunk_size.min(entries.len());
        let slice: Vec<_> = entries.drain(..take).collect();
        if let Some(node) = build_rtree(slice) {
            children.push(node);
        }
    }
    let child_boxes: Vec<[Parameter; 7]> = children.iter().map(|c| *c.bounds()).collect();
    Some(RTreeNode::Branch {
        bounds: union_bounds(&child_boxes),
        children,
    })
}

/// Nearest-neighbor biome lookup in 6D climate space.
#[derive(Debug, Clone)]
pub struct MultiNoiseBiomeSource {
    pub entries: Vec<(ParameterPoint, u32)>,
    tree: Option<Box<RTreeNode>>,
    zero_depth_tree: Option<Box<RTreeNode>>,
    non_zero_depth_tree: Option<Box<RTreeNode>>,
    zero_depth_invariant: bool,
}

impl MultiNoiseBiomeSource {
    pub fn new(entries: Vec<(ParameterPoint, u32)>) -> Self {
        let zero_depth_invariant = !entries.is_empty()
            && entries
                .iter()
                .all(|(p, _)| (p.depth.min == 0.0 && p.depth.max == 0.0) || p.depth.min > 0.0);
        let (zero_depth_tree, non_zero_depth_tree) = if zero_depth_invariant {
            let zero_entries: Vec<_> = entries
                .iter()
                .copied()
                .filter(|(p, _)| p.depth.min == 0.0 && p.depth.max == 0.0)
                .collect();
            let non_zero_entries: Vec<_> = entries
                .iter()
                .copied()
                .filter(|(p, _)| p.depth.min > 0.0)
                .collect();
            (
                build_rtree(zero_entries).map(Box::new),
                build_rtree(non_zero_entries).map(Box::new),
            )
        } else {
            (None, None)
        };
        let tree = build_rtree(entries.clone()).map(Box::new);
        Self {
            entries,
            tree,
            zero_depth_tree,
            non_zero_depth_tree,
            zero_depth_invariant,
        }
    }

    pub fn find_biome(&self, target: [f32; 6]) -> u32 {
        self.find_biome_with_zero_depth_flag(target).0
    }

    /// Evaluates the closest `depth == [0.0, 0.0]` surface biome for a 2D column `base_2d`
    /// (with `base_2d[4] == 0.0`), returning `Some((biome_id, dist_sq_5d))` when
    /// `zero_depth_invariant` holds.
    ///
    /// Because every `depth == [0.0, 0.0]` biome's 6D squared fitness at any depth `d` is
    /// `dist_sq_5d + d * d`, this single column lookup determines the best surface biome across
    /// all 64 vertical biome cells in the column; vertical queries only need to test the tiny
    /// [`Self::non_zero_depth_tree`] (cave biomes).
    #[inline]
    pub fn find_zero_depth_column_biome(&self, mut base_2d: [f32; 6]) -> Option<(u32, f32)> {
        if !self.zero_depth_invariant {
            return None;
        }
        base_2d[4] = 0.0;
        let tree = self.zero_depth_tree.as_ref()?;
        let mut best = (0u32, f32::INFINITY, true);
        tree.search(base_2d, &mut best);
        Some((best.0, best.1))
    }

    /// Given the precomputed best `depth == [0.0, 0.0]` surface biome `(zero_biome, zero_dist_sq_5d)`
    /// for a column, returns the winning biome at `target` by checking only non-zero-depth (cave)
    /// biomes when `target[4] > 0.0`.
    #[inline]
    pub fn find_biome_with_column_zero_depth(
        &self,
        target: [f32; 6],
        zero_biome: u32,
        zero_dist_sq_5d: f32,
    ) -> u32 {
        let d = target[4];
        if d <= 0.0 {
            return zero_biome;
        }
        let Some(cave_tree) = &self.non_zero_depth_tree else {
            return zero_biome;
        };
        let mut best = (zero_biome, zero_dist_sq_5d + d * d, true);
        cave_tree.search(target, &mut best);
        best.0
    }

    /// Evaluates the winning biome at `target` and returns `(biome_id, holds_for_all_non_positive_depth)`.
    /// When `target[4] == 0.0` and the second element is `true`, any query with the same 5 horizontal
    /// climate channels and `depth <= 0.0` is mathematically guaranteed to return `biome_id`.
    #[inline]
    pub fn find_biome_with_zero_depth_flag(&self, target: [f32; 6]) -> (u32, bool) {
        if let Some(tree) = &self.tree {
            let mut best = (0u32, f32::INFINITY, false);
            tree.search(target, &mut best);
            return (best.0, self.zero_depth_invariant && best.2);
        }
        let mut best_id = 0u32;
        let mut best_dist = f32::INFINITY;
        let mut best_zero = false;
        for &(point, biome_id) in &self.entries {
            let d = point.fitness(target);
            if d < best_dist {
                best_dist = d;
                best_id = biome_id;
                best_zero = point.depth.min == 0.0 && point.depth.max == 0.0;
            }
        }
        (best_id, self.zero_depth_invariant && best_zero)
    }
}

/// Vanilla End biome selection (`TheEndBiomeSource` thresholds on `erosion` / `end/islands`).
#[derive(Debug, Clone, Copy)]
pub struct EndBiomeSource {
    pub the_end: u32,
    pub highlands: u32,
    pub midlands: u32,
    pub islands: u32,
    pub barrens: u32,
}

impl EndBiomeSource {
    #[inline]
    pub fn find_biome(&self, block_x: i32, block_z: i32, erosion: f32) -> u32 {
        let qx = i64::from(block_x.div_euclid(4));
        let qz = i64::from(block_z.div_euclid(4));
        if qx * qx + qz * qz <= 4096 {
            self.the_end
        } else if erosion > 0.25 {
            self.highlands
        } else if erosion >= -0.0625 {
            self.midlands
        } else if erosion < -0.21875 {
            self.islands
        } else {
            self.barrens
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_biome_matches_closest_climate_point() {
        let source = MultiNoiseBiomeSource::new(vec![
            (
                ParameterPoint {
                    temperature: Parameter::span(-1.0, -0.2),
                    humidity: Parameter::span(-1.0, 1.0),
                    continentalness: Parameter::span(-1.0, 1.0),
                    erosion: Parameter::span(-1.0, 1.0),
                    depth: Parameter::point(0.0),
                    weirdness: Parameter::span(-1.0, 1.0),
                    offset: 0.0,
                },
                1,
            ),
            (
                ParameterPoint {
                    temperature: Parameter::span(0.2, 1.0),
                    humidity: Parameter::span(-1.0, 1.0),
                    continentalness: Parameter::span(-1.0, 1.0),
                    erosion: Parameter::span(-1.0, 1.0),
                    depth: Parameter::point(0.0),
                    weirdness: Parameter::span(-1.0, 1.0),
                    offset: 0.0,
                },
                2,
            ),
        ]);
        assert_eq!(source.find_biome([-0.5, 0.0, 0.0, 0.0, 0.0, 0.0]), 1);
        assert_eq!(source.find_biome([0.6, 0.0, 0.0, 0.0, 0.0, 0.0]), 2);
    }
}
