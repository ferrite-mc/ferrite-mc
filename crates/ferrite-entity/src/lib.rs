//! Ferrite's entity system.
//!
//! - [`EntityStore`]: per-region storage. Generational keys over dense structure-of-arrays
//!   columns for the hot fields (position, velocity, bounding box, flags) plus a dense column
//!   of everything else, and a chunk-section index ([`SectionIndex`]) for range and collision
//!   queries.
//! - [`tracker`]: the vanilla entity tracker, run per region against the region's players.
//! - [`Metadata`]: synched entity data with dirty tracking.
//! - [`item`]: item entities (vanilla physics against block shapes, merging, pickup into a
//!   player inventory, despawning), with [`physics`] underneath.
//! - [`experience`]: experience orbs (award split, merge scan, attraction to players, pickup).
//! - [`effect`]: mob effects (instances, vanilla tick timings, attribute modifiers).
//! - [`allocate_id`] / [`random_uuid`]: the process-wide network id counter (shared with
//!   players) and entity UUIDs.
//!
//! Threading follows Folia: an entity is only touched by the region that owns its chunk.
//! Stores split and merge with their regions, and an entity that walks into another region
//! section leaves its store by value ([`EntityStore::take_crossing`]) to be handed to the new
//! owner through that region's task queue.

pub mod ai;
pub mod arrow;
pub mod bobber;
pub mod breed;
pub mod combat;
pub mod creeper;
pub mod dragon;
pub mod effect;
pub mod enderman;
pub mod experience;
pub mod eye;
pub mod falling;
pub mod firework;
pub mod horse;
mod id;
mod index;
pub mod item;
pub mod metadata;
pub mod mob;
pub mod path;
pub mod physics;
pub mod potion;
mod rng;
pub mod sheep;
pub mod shoulder;
pub mod spawn;
mod store;
pub mod sun;
pub mod tame;
pub mod tnt;
pub mod tracker;
pub mod variant;
pub mod villager;

pub use id::{EntityKey, allocate_id, random_uuid};
pub use index::{Cell, QUERY_MARGIN, SectionIndex};
pub use metadata::Metadata;
pub use physics::BlockGetter;
pub use rng::Rng;
pub use store::{DEFAULT_REGION_SHIFT, EntityData, EntityRecord, EntityStore, Flags, chunk_of};
