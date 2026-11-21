//! Vanilla-compatible `DensityFunction` AST and evaluator (Minecraft 26.3).

use std::collections::HashMap;
use std::sync::Arc;

use crate::noise::{BlendedNoise, NormalNoise, SimplexNoise};

/// Block position passed to `DensityFunction::compute`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionContext {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl FunctionContext {
    #[inline]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

/// Named noise instances keyed by resource location (e.g. `"minecraft:temperature"`).
pub type NoiseMap = HashMap<String, Arc<NormalNoise>>;

/// Coordinate axis for `gradient` and `slice`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// A cubic spline node used by `DensityFunction::Spline`.
#[derive(Debug, Clone)]
pub struct SplinePoint {
    pub location: f64,
    pub value: SplineValue,
    pub derivative: f64,
}

#[derive(Debug, Clone)]
pub enum SplineValue {
    Constant(f64),
    Spline(Box<CubicSpline>),
}

#[derive(Debug, Clone)]
pub struct CubicSpline {
    pub coordinate: Box<DensityFunction>,
    pub points: Vec<SplinePoint>,
}

impl CubicSpline {
    pub fn evaluate(&self, ctx: FunctionContext, noises: &NoiseMap) -> f64 {
        let coord = self.coordinate.compute(ctx, noises);
        if self.points.is_empty() {
            return 0.0;
        }
        if self.points.len() == 1 {
            return self.points[0].value.eval(ctx, noises);
        }
        let first = &self.points[0];
        if coord <= first.location {
            let v0 = first.value.eval(ctx, noises);
            return v0 + first.derivative * (coord - first.location);
        }
        let last = &self.points[self.points.len() - 1];
        if coord >= last.location {
            let vn = last.value.eval(ctx, noises);
            return vn + last.derivative * (coord - last.location);
        }
        let idx = self
            .points
            .partition_point(|p| p.location <= coord)
            .saturating_sub(1);
        let p0 = &self.points[idx];
        let p1 = &self.points[idx + 1];
        let dx = p1.location - p0.location;
        if dx == 0.0 {
            return p0.value.eval(ctx, noises);
        }
        let t = (coord - p0.location) / dx;
        let v0 = p0.value.eval(ctx, noises);
        let v1 = p1.value.eval(ctx, noises);
        let a = p0.derivative * dx - (v1 - v0);
        let b = -p1.derivative * dx + (v1 - v0);
        let one_minus_t = 1.0 - t;
        one_minus_t * v0 + t * v1 + t * one_minus_t * (a * one_minus_t + b * t)
    }
}

impl SplineValue {
    fn eval(&self, ctx: FunctionContext, noises: &NoiseMap) -> f64 {
        match self {
            SplineValue::Constant(v) => *v,
            SplineValue::Spline(s) => s.evaluate(ctx, noises),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappedOp {
    Abs,
    Square,
    Cube,
    HalfNegative,
    QuarterNegative,
    Squeeze,
    Negate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Min,
    Max,
}

/// Per-chunk local evaluation cache for [`DensityFunction::Cached`] nodes.
///
/// Allocated per `generate_chunk` call so [`NoiseChunkGenerator`] remains completely
/// lock-free and `Send + Sync` across parallel region threads.
#[derive(Debug, Clone)]
pub struct EvalCache {
    slots: Vec<(i32, i32, i32, f64)>,
}

impl EvalCache {
    pub fn new(count: usize) -> Self {
        Self {
            slots: vec![(i32::MIN, i32::MIN, i32::MIN, 0.0); count],
        }
    }
}

/// Core `DensityFunction` AST covering all 33 vanilla 26.3 density function types.
#[derive(Debug, Clone)]
pub enum DensityFunction {
    Constant(f64),
    Noise {
        noise_id: String,
        noise: Option<Arc<NormalNoise>>,
        xz_scale: f64,
        y_scale: f64,
        shift_x: Option<Box<DensityFunction>>,
        shift_z: Option<Box<DensityFunction>>,
    },
    ShiftedNoise {
        noise_id: String,
        noise: Option<Arc<NormalNoise>>,
        xz_scale: f64,
        y_scale: f64,
        shift_x: Box<DensityFunction>,
        shift_y: Box<DensityFunction>,
        shift_z: Box<DensityFunction>,
    },
    ShiftA {
        noise_id: String,
        noise: Option<Arc<NormalNoise>>,
    },
    ShiftB {
        noise_id: String,
        noise: Option<Arc<NormalNoise>>,
    },
    OldBlendedNoise {
        blended: Option<Arc<BlendedNoise>>,
    },
    Mapped {
        op: MappedOp,
        input: Box<DensityFunction>,
    },
    Binary {
        op: BinaryOp,
        arg1: Box<DensityFunction>,
        arg2: Box<DensityFunction>,
    },
    Lerp {
        alpha: Box<DensityFunction>,
        first: Box<DensityFunction>,
        second: Box<DensityFunction>,
    },
    Clamp {
        input: Box<DensityFunction>,
        min: f64,
        max: f64,
    },
    Gradient {
        axis: Axis,
        from_coordinate: i32,
        to_coordinate: i32,
        from_value: f64,
        to_value: f64,
    },
    Slice {
        axis: Axis,
        coordinate: i32,
        input: Box<DensityFunction>,
    },
    RangeChoice {
        input: Box<DensityFunction>,
        min_inclusive: f64,
        max_exclusive: f64,
        when_in_range: Box<DensityFunction>,
        when_out_of_range: Box<DensityFunction>,
    },
    IntervalSelect {
        input: Box<DensityFunction>,
        thresholds: Vec<f64>,
        functions: Vec<DensityFunction>,
    },
    DistanceToPoint {
        point: [f64; 3],
    },
    FindTopSurface {
        density: Box<DensityFunction>,
        upper_bound: Box<DensityFunction>,
        lower_bound: i32,
        cell_height: i32,
    },
    Spline(Box<CubicSpline>),
    EndOuterIslands(Arc<SimplexNoise>),
    Cached {
        id: usize,
        is_2d: bool,
        input: Box<DensityFunction>,
    },
    Wrapper(Box<DensityFunction>),
}

impl CubicSpline {
    pub fn bind_noises(&mut self, noises: &NoiseMap) {
        self.coordinate.bind_noises(noises);
        for p in &mut self.points {
            if let SplineValue::Spline(s) = &mut p.value {
                s.bind_noises(noises);
            }
        }
    }

    fn depends_on_y(&self) -> bool {
        self.coordinate.depends_on_y()
            || self.points.iter().any(|p| match &p.value {
                SplineValue::Constant(_) => false,
                SplineValue::Spline(s) => s.depends_on_y(),
            })
    }

    pub fn evaluate_cached(
        &self,
        ctx: FunctionContext,
        noises: &NoiseMap,
        cache: &mut EvalCache,
    ) -> f64 {
        let coord = self.coordinate.compute_cached(ctx, noises, cache);
        if self.points.is_empty() {
            return 0.0;
        }
        if self.points.len() == 1 {
            return self.points[0].value.eval_cached(ctx, noises, cache);
        }
        let first = &self.points[0];
        if coord <= first.location {
            let v0 = first.value.eval_cached(ctx, noises, cache);
            return v0 + first.derivative * (coord - first.location);
        }
        let last = &self.points[self.points.len() - 1];
        if coord >= last.location {
            let vn = last.value.eval_cached(ctx, noises, cache);
            return vn + last.derivative * (coord - last.location);
        }
        let idx = self
            .points
            .partition_point(|p| p.location <= coord)
            .saturating_sub(1);
        let p0 = &self.points[idx];
        let p1 = &self.points[idx + 1];
        let dx = p1.location - p0.location;
        if dx == 0.0 {
            return p0.value.eval_cached(ctx, noises, cache);
        }
        let t = (coord - p0.location) / dx;
        let v0 = p0.value.eval_cached(ctx, noises, cache);
        let v1 = p1.value.eval_cached(ctx, noises, cache);
        let a = p0.derivative * dx - (v1 - v0);
        let b = -p1.derivative * dx + (v1 - v0);
        let one_minus_t = 1.0 - t;
        one_minus_t * v0 + t * v1 + t * one_minus_t * (a * one_minus_t + b * t)
    }
}

impl SplineValue {
    fn eval_cached(&self, ctx: FunctionContext, noises: &NoiseMap, cache: &mut EvalCache) -> f64 {
        match self {
            SplineValue::Constant(v) => *v,
            SplineValue::Spline(s) => s.evaluate_cached(ctx, noises, cache),
        }
    }
}

impl DensityFunction {
    /// Pre-binds `Arc<NormalNoise>` pointers on all noise nodes so evaluation avoids string
    /// `HashMap` lookups in the inner worldgen loop.
    pub fn bind_noises(&mut self, noises: &NoiseMap) {
        match self {
            DensityFunction::Constant(_)
            | DensityFunction::OldBlendedNoise { .. }
            | DensityFunction::Gradient { .. }
            | DensityFunction::DistanceToPoint { .. }
            | DensityFunction::EndOuterIslands(_) => {}
            DensityFunction::Noise {
                noise_id,
                noise,
                shift_x,
                shift_z,
                ..
            } => {
                if noise.is_none() {
                    *noise = noises.get(noise_id).cloned();
                }
                if let Some(sx) = shift_x {
                    sx.bind_noises(noises);
                }
                if let Some(sz) = shift_z {
                    sz.bind_noises(noises);
                }
            }
            DensityFunction::ShiftedNoise {
                noise_id,
                noise,
                shift_x,
                shift_y,
                shift_z,
                ..
            } => {
                if noise.is_none() {
                    *noise = noises.get(noise_id).cloned();
                }
                shift_x.bind_noises(noises);
                shift_y.bind_noises(noises);
                shift_z.bind_noises(noises);
            }
            DensityFunction::ShiftA { noise_id, noise }
            | DensityFunction::ShiftB { noise_id, noise } => {
                if noise.is_none() {
                    *noise = noises.get(noise_id).cloned();
                }
            }
            DensityFunction::Mapped { input, .. }
            | DensityFunction::Clamp { input, .. }
            | DensityFunction::Slice { input, .. }
            | DensityFunction::Cached { input, .. }
            | DensityFunction::Wrapper(input) => input.bind_noises(noises),
            DensityFunction::Binary { arg1, arg2, .. } => {
                arg1.bind_noises(noises);
                arg2.bind_noises(noises);
            }
            DensityFunction::Lerp {
                alpha,
                first,
                second,
            } => {
                alpha.bind_noises(noises);
                first.bind_noises(noises);
                second.bind_noises(noises);
            }
            DensityFunction::RangeChoice {
                input,
                when_in_range,
                when_out_of_range,
                ..
            } => {
                input.bind_noises(noises);
                when_in_range.bind_noises(noises);
                when_out_of_range.bind_noises(noises);
            }
            DensityFunction::IntervalSelect {
                input, functions, ..
            } => {
                input.bind_noises(noises);
                for f in functions {
                    f.bind_noises(noises);
                }
            }
            DensityFunction::FindTopSurface {
                density,
                upper_bound,
                ..
            } => {
                density.bind_noises(noises);
                upper_bound.bind_noises(noises);
            }
            DensityFunction::Spline(s) => s.bind_noises(noises),
        }
    }

    /// Returns `true` if the value of this density function can vary with `ctx.y`.
    pub fn depends_on_y(&self) -> bool {
        match self {
            DensityFunction::Constant(_)
            | DensityFunction::ShiftA { .. }
            | DensityFunction::ShiftB { .. }
            | DensityFunction::EndOuterIslands(_) => false,
            DensityFunction::Noise { y_scale, .. } => *y_scale != 0.0,
            DensityFunction::ShiftedNoise {
                y_scale, shift_y, ..
            } => *y_scale != 0.0 || shift_y.depends_on_y(),
            DensityFunction::OldBlendedNoise { .. } | DensityFunction::DistanceToPoint { .. } => {
                true
            }
            DensityFunction::Mapped { input, .. }
            | DensityFunction::Clamp { input, .. }
            | DensityFunction::Cached { input, .. }
            | DensityFunction::Wrapper(input) => input.depends_on_y(),
            DensityFunction::Binary { arg1, arg2, .. } => {
                arg1.depends_on_y() || arg2.depends_on_y()
            }
            DensityFunction::Lerp {
                alpha,
                first,
                second,
            } => alpha.depends_on_y() || first.depends_on_y() || second.depends_on_y(),
            DensityFunction::Gradient { axis, .. } => *axis == Axis::Y,
            DensityFunction::Slice { axis, input, .. } => *axis != Axis::Y && input.depends_on_y(),
            DensityFunction::RangeChoice {
                input,
                when_in_range,
                when_out_of_range,
                ..
            } => {
                input.depends_on_y()
                    || when_in_range.depends_on_y()
                    || when_out_of_range.depends_on_y()
            }
            DensityFunction::IntervalSelect {
                input, functions, ..
            } => input.depends_on_y() || functions.iter().any(|f| f.depends_on_y()),
            DensityFunction::FindTopSurface { .. } => false,
            DensityFunction::Spline(s) => s.depends_on_y(),
        }
    }

    pub fn compute_cached(
        &self,
        ctx: FunctionContext,
        noises: &NoiseMap,
        cache: &mut EvalCache,
    ) -> f64 {
        match self {
            DensityFunction::Cached { id, is_2d, input } => {
                let key_y = if *is_2d { 0 } else { ctx.y };
                if let Some(&(cx, cy, cz, val)) = cache.slots.get(*id)
                    && cx == ctx.x
                    && cy == key_y
                    && cz == ctx.z
                {
                    return val;
                }
                let val = input.compute_cached(ctx, noises, cache);
                if let Some(slot) = cache.slots.get_mut(*id) {
                    *slot = (ctx.x, key_y, ctx.z, val);
                }
                val
            }
            DensityFunction::Wrapper(inner) => inner.compute_cached(ctx, noises, cache),
            DensityFunction::Spline(spline) => spline.evaluate_cached(ctx, noises, cache),
            DensityFunction::Mapped { op, input } => {
                let v = input.compute_cached(ctx, noises, cache);
                match op {
                    MappedOp::Abs => v.abs(),
                    MappedOp::Square => v * v,
                    MappedOp::Cube => v * v * v,
                    MappedOp::HalfNegative => {
                        if v > 0.0 {
                            v
                        } else {
                            v * 0.5
                        }
                    }
                    MappedOp::QuarterNegative => {
                        if v > 0.0 {
                            v
                        } else {
                            v * 0.25
                        }
                    }
                    MappedOp::Squeeze => {
                        let c = v.clamp(-1.0, 1.0);
                        c / 2.0 - c * c * c / 24.0
                    }
                    MappedOp::Negate => -v,
                }
            }
            DensityFunction::Binary { op, arg1, arg2 } => {
                let a = arg1.compute_cached(ctx, noises, cache);
                let b = arg2.compute_cached(ctx, noises, cache);
                match op {
                    BinaryOp::Add => a + b,
                    BinaryOp::Sub => a - b,
                    BinaryOp::Mul => a * b,
                    BinaryOp::Div => {
                        if b == 0.0 {
                            0.0
                        } else {
                            a / b
                        }
                    }
                    BinaryOp::Min => a.min(b),
                    BinaryOp::Max => a.max(b),
                }
            }
            DensityFunction::Lerp {
                alpha,
                first,
                second,
            } => {
                let t = alpha.compute_cached(ctx, noises, cache);
                let a = first.compute_cached(ctx, noises, cache);
                let b = second.compute_cached(ctx, noises, cache);
                a + t * (b - a)
            }
            DensityFunction::Clamp { input, min, max } => {
                input.compute_cached(ctx, noises, cache).clamp(*min, *max)
            }
            DensityFunction::Noise {
                noise_id,
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_z,
            } => {
                let sx = shift_x
                    .as_ref()
                    .map(|f| f.compute_cached(ctx, noises, cache))
                    .unwrap_or(0.0);
                let sz = shift_z
                    .as_ref()
                    .map(|f| f.compute_cached(ctx, noises, cache))
                    .unwrap_or(0.0);
                let resolved = noise
                    .as_deref()
                    .or_else(|| noises.get(noise_id).map(|n| &**n));
                resolved
                    .map(|n| {
                        n.get_value(
                            ctx.x as f64 * xz_scale + sx,
                            ctx.y as f64 * y_scale,
                            ctx.z as f64 * xz_scale + sz,
                        )
                    })
                    .unwrap_or(0.0)
            }
            DensityFunction::ShiftedNoise {
                noise_id,
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                let x = ctx.x as f64 * xz_scale + shift_x.compute_cached(ctx, noises, cache);
                let y = ctx.y as f64 * y_scale + shift_y.compute_cached(ctx, noises, cache);
                let z = ctx.z as f64 * xz_scale + shift_z.compute_cached(ctx, noises, cache);
                let resolved = noise
                    .as_deref()
                    .or_else(|| noises.get(noise_id).map(|n| &**n));
                resolved.map(|n| n.get_value(x, y, z)).unwrap_or(0.0)
            }
            DensityFunction::Slice {
                axis,
                coordinate,
                input,
            } => {
                let sliced = match axis {
                    Axis::X => FunctionContext::new(*coordinate, ctx.y, ctx.z),
                    Axis::Y => FunctionContext::new(ctx.x, *coordinate, ctx.z),
                    Axis::Z => FunctionContext::new(ctx.x, ctx.y, *coordinate),
                };
                input.compute_cached(sliced, noises, cache)
            }
            DensityFunction::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let v = input.compute_cached(ctx, noises, cache);
                if v >= *min_inclusive && v < *max_exclusive {
                    when_in_range.compute_cached(ctx, noises, cache)
                } else {
                    when_out_of_range.compute_cached(ctx, noises, cache)
                }
            }
            DensityFunction::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                if functions.is_empty() {
                    return 0.0;
                }
                let v = input.compute_cached(ctx, noises, cache);
                let idx = thresholds.partition_point(|&th| th <= v);
                let f = &functions[idx.min(functions.len() - 1)];
                f.compute_cached(ctx, noises, cache)
            }
            _ => self.compute(ctx, noises),
        }
    }

    pub fn compute(&self, ctx: FunctionContext, noises: &NoiseMap) -> f64 {
        match self {
            DensityFunction::Constant(v) => *v,
            DensityFunction::Noise {
                noise_id,
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_z,
            } => {
                let sx = shift_x
                    .as_ref()
                    .map(|f| f.compute(ctx, noises))
                    .unwrap_or(0.0);
                let sz = shift_z
                    .as_ref()
                    .map(|f| f.compute(ctx, noises))
                    .unwrap_or(0.0);
                let resolved = noise
                    .as_deref()
                    .or_else(|| noises.get(noise_id).map(|n| &**n));
                resolved
                    .map(|n| {
                        n.get_value(
                            ctx.x as f64 * xz_scale + sx,
                            ctx.y as f64 * y_scale,
                            ctx.z as f64 * xz_scale + sz,
                        )
                    })
                    .unwrap_or(0.0)
            }
            DensityFunction::ShiftedNoise {
                noise_id,
                noise,
                xz_scale,
                y_scale,
                shift_x,
                shift_y,
                shift_z,
            } => {
                let x = ctx.x as f64 * xz_scale + shift_x.compute(ctx, noises);
                let y = ctx.y as f64 * y_scale + shift_y.compute(ctx, noises);
                let z = ctx.z as f64 * xz_scale + shift_z.compute(ctx, noises);
                let resolved = noise
                    .as_deref()
                    .or_else(|| noises.get(noise_id).map(|n| &**n));
                resolved.map(|n| n.get_value(x, y, z)).unwrap_or(0.0)
            }
            DensityFunction::ShiftA { noise_id, noise } => {
                let resolved = noise
                    .as_deref()
                    .or_else(|| noises.get(noise_id).map(|n| &**n));
                resolved
                    .map(|n| n.get_value(ctx.x as f64 * 0.25, 0.0, ctx.z as f64 * 0.25) * 4.0)
                    .unwrap_or(0.0)
            }
            DensityFunction::ShiftB { noise_id, noise } => {
                let resolved = noise
                    .as_deref()
                    .or_else(|| noises.get(noise_id).map(|n| &**n));
                resolved
                    .map(|n| n.get_value(ctx.z as f64 * 0.25, ctx.x as f64 * 0.25, 0.0) * 4.0)
                    .unwrap_or(0.0)
            }
            DensityFunction::OldBlendedNoise { blended } => blended
                .as_ref()
                .map(|b| b.sample(ctx.x, ctx.y, ctx.z))
                .unwrap_or(0.0),
            DensityFunction::Mapped { op, input } => {
                let v = input.compute(ctx, noises);
                match op {
                    MappedOp::Abs => v.abs(),
                    MappedOp::Square => v * v,
                    MappedOp::Cube => v * v * v,
                    MappedOp::HalfNegative => {
                        if v > 0.0 {
                            v
                        } else {
                            v * 0.5
                        }
                    }
                    MappedOp::QuarterNegative => {
                        if v > 0.0 {
                            v
                        } else {
                            v * 0.25
                        }
                    }
                    MappedOp::Squeeze => {
                        let c = v.clamp(-1.0, 1.0);
                        c / 2.0 - c * c * c / 24.0
                    }
                    MappedOp::Negate => -v,
                }
            }
            DensityFunction::Binary { op, arg1, arg2 } => {
                let a = arg1.compute(ctx, noises);
                let b = arg2.compute(ctx, noises);
                match op {
                    BinaryOp::Add => a + b,
                    BinaryOp::Sub => a - b,
                    BinaryOp::Mul => a * b,
                    BinaryOp::Div => {
                        if b == 0.0 {
                            0.0
                        } else {
                            a / b
                        }
                    }
                    BinaryOp::Min => a.min(b),
                    BinaryOp::Max => a.max(b),
                }
            }
            DensityFunction::Lerp {
                alpha,
                first,
                second,
            } => {
                let t = alpha.compute(ctx, noises);
                let a = first.compute(ctx, noises);
                let b = second.compute(ctx, noises);
                a + t * (b - a)
            }
            DensityFunction::Clamp { input, min, max } => {
                input.compute(ctx, noises).clamp(*min, *max)
            }
            DensityFunction::Gradient {
                axis,
                from_coordinate,
                to_coordinate,
                from_value,
                to_value,
            } => {
                let coord = match axis {
                    Axis::X => ctx.x,
                    Axis::Y => ctx.y,
                    Axis::Z => ctx.z,
                };
                if coord <= *from_coordinate {
                    *from_value
                } else if coord >= *to_coordinate {
                    *to_value
                } else {
                    let t = (coord - *from_coordinate) as f64
                        / (*to_coordinate - *from_coordinate) as f64;
                    from_value + t * (to_value - from_value)
                }
            }
            DensityFunction::Slice {
                axis,
                coordinate,
                input,
            } => {
                let sliced = match axis {
                    Axis::X => FunctionContext::new(*coordinate, ctx.y, ctx.z),
                    Axis::Y => FunctionContext::new(ctx.x, *coordinate, ctx.z),
                    Axis::Z => FunctionContext::new(ctx.x, ctx.y, *coordinate),
                };
                input.compute(sliced, noises)
            }
            DensityFunction::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let v = input.compute(ctx, noises);
                if v >= *min_inclusive && v < *max_exclusive {
                    when_in_range.compute(ctx, noises)
                } else {
                    when_out_of_range.compute(ctx, noises)
                }
            }
            DensityFunction::IntervalSelect {
                input,
                thresholds,
                functions,
            } => {
                if functions.is_empty() {
                    return 0.0;
                }
                let v = input.compute(ctx, noises);
                let idx = thresholds.partition_point(|&th| th <= v);
                let f = &functions[idx.min(functions.len() - 1)];
                f.compute(ctx, noises)
            }
            DensityFunction::DistanceToPoint { point } => {
                let dx = ctx.x as f64 - point[0];
                let dy = ctx.y as f64 - point[1];
                let dz = ctx.z as f64 - point[2];
                (dx * dx + dy * dy + dz * dz).sqrt()
            }
            DensityFunction::FindTopSurface {
                density,
                upper_bound,
                lower_bound,
                cell_height,
            } => {
                let step = (*cell_height).max(1);
                let top = upper_bound.compute(ctx, noises).floor() as i32;
                let top_aligned = top - top.rem_euclid(step);
                let mut y = top_aligned;
                while y >= *lower_bound {
                    let v = density.compute(FunctionContext::new(ctx.x, y, ctx.z), noises);
                    if v > 0.0 {
                        return y as f64;
                    }
                    y -= step;
                }
                *lower_bound as f64
            }
            DensityFunction::Spline(spline) => spline.evaluate(ctx, noises),
            DensityFunction::EndOuterIslands(simplex) => simplex.end_island_density(ctx.x, ctx.z),
            DensityFunction::Cached { input, .. } | DensityFunction::Wrapper(input) => {
                input.compute(ctx, noises)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::random::XoroshiroRandom;

    #[test]
    fn gradient_interpolates_and_clamps() {
        let df = DensityFunction::Gradient {
            axis: Axis::Y,
            from_coordinate: 0,
            to_coordinate: 100,
            from_value: 1.0,
            to_value: -1.0,
        };
        let noises = NoiseMap::new();
        assert_eq!(df.compute(FunctionContext::new(0, -10, 0), &noises), 1.0);
        assert_eq!(df.compute(FunctionContext::new(0, 50, 0), &noises), 0.0);
        assert_eq!(df.compute(FunctionContext::new(0, 150, 0), &noises), -1.0);
    }

    #[test]
    fn noise_and_spline_evaluate_cleanly() {
        let mut noises = NoiseMap::new();
        let mut rng = XoroshiroRandom::from_seed(99);
        noises.insert(
            "minecraft:continents".to_string(),
            Arc::new(NormalNoise::new(&mut rng, -9, &[1.0, 1.0])),
        );
        let spline = DensityFunction::Spline(Box::new(CubicSpline {
            coordinate: Box::new(DensityFunction::Noise {
                noise_id: "minecraft:continents".to_string(),
                noise: None,
                xz_scale: 0.25,
                y_scale: 0.0,
                shift_x: None,
                shift_z: None,
            }),
            points: vec![
                SplinePoint {
                    location: -1.0,
                    value: SplineValue::Constant(-0.5),
                    derivative: 0.0,
                },
                SplinePoint {
                    location: 1.0,
                    value: SplineValue::Constant(0.5),
                    derivative: 0.0,
                },
            ],
        }));
        let val = spline.compute(FunctionContext::new(16, 64, -32), &noises);
        assert!(val.is_finite());
    }
}
