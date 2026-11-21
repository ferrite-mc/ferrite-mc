//! Vanilla-compatible Rust world generation for Ferrite.
//!
//! Implements Minecraft 26.3's noise, density functions, multi-noise biome source,
//! and noise chunk generator.

pub mod biome;
pub mod chunk_gen;
pub mod density;
pub mod noise;
pub mod random;
pub mod vanilla_loader;
