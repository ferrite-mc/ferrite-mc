//! Server-owned v2 block state seam.
//!
//! Reads use the dimension's real [`AnvilChunkSource`]; they never generate a chunk. Writes are
//! validated and queued here, then applied by [`WorldStateProvider::tick`] from the game-loop
//! tick. `world.block_set_async` returning `queued: true` therefore means exactly that: the
//! write was accepted, not applied.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::game::{AnvilChunkSource, Dimension, DimensionRegistry};
use ferrite_plugin_host::{
    BlockGetRequest, BlockSetAccepted, BlockSetRequest, BlockSnapshot, HostStateProvider,
    StateError,
};

/// Maximum accepted-but-not-yet-applied block writes. Overflow is a hard `queue_full` error.
pub const MAX_PENDING_BLOCK_WRITES: usize = 4096;

struct QueuedWrite {
    source: Arc<AnvilChunkSource>,
    x: i32,
    y: i32,
    z: i32,
    state_id: u32,
    expected_state_id: Option<u32>,
}

/// Adapter from the contract-v2 provider seam to Ferrite's actual dimension sources.
pub struct WorldStateProvider {
    dimensions: Arc<DimensionRegistry>,
    pending: Mutex<VecDeque<QueuedWrite>>,
    next_request: AtomicU64,
}

impl WorldStateProvider {
    /// Wraps the world's real dimension registry.
    pub fn new(dimensions: Arc<DimensionRegistry>) -> Self {
        Self {
            dimensions,
            pending: Mutex::new(VecDeque::new()),
            next_request: AtomicU64::new(0),
        }
    }

    fn source(&self, world: &str) -> Result<Arc<AnvilChunkSource>, StateError> {
        let dimension = match world {
            "minecraft:overworld" => Dimension::Overworld,
            "minecraft:the_nether" => Dimension::Nether,
            "minecraft:the_end" => Dimension::End,
            _ => return Err(StateError::unknown_world(world)),
        };
        Ok(Arc::clone(self.dimensions.source(dimension)))
    }

    fn snapshot(
        &self,
        world: &str,
        id: u16,
        x: i32,
        y: i32,
        z: i32,
    ) -> Result<BlockSnapshot, StateError> {
        let (material, properties) = ferrite_data::blocks::state(id)
            .ok_or_else(|| StateError::unknown_block_state(u32::from(id)))?;
        Ok(BlockSnapshot {
            world: world.to_owned(),
            x,
            y,
            z,
            state_id: u32::from(id),
            material: material.to_owned(),
            properties: properties
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect::<BTreeMap<_, _>>(),
            loaded: true,
        })
    }

    /// Applies the queue on the game-loop thread. Every failure is logged; it never panics the
    /// tick loop. A failed write is not silently retried.
    pub fn drain(&self) {
        let writes: Vec<QueuedWrite> = {
            let mut pending = self
                .pending
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            pending.drain(..).collect()
        };
        for write in writes {
            match write.source.compare_set_block(
                write.x,
                write.y,
                write.z,
                write.expected_state_id,
                write.state_id,
            ) {
                Ok(()) => tracing::debug!(
                    x = write.x,
                    y = write.y,
                    z = write.z,
                    state = write.state_id,
                    "applied a plugin block write"
                ),
                Err(error) => tracing::warn!(
                    x = write.x,
                    y = write.y,
                    z = write.z,
                    state = write.state_id,
                    %error,
                    "a queued plugin block write failed"
                ),
            }
        }
    }
}

impl HostStateProvider for WorldStateProvider {
    fn block_get(&self, request: &BlockGetRequest) -> Result<BlockSnapshot, StateError> {
        let source = self.source(&request.world)?;
        let state = source
            .block(request.x, request.y, request.z)
            .ok_or_else(|| {
                StateError::chunk_not_loaded(request.x.div_euclid(16), request.z.div_euclid(16))
            })?;
        let id = u16::try_from(state).map_err(|_| StateError::unknown_block_state(state))?;
        self.snapshot(&request.world, id, request.x, request.y, request.z)
    }

    fn block_set_async(&self, request: &BlockSetRequest) -> Result<BlockSetAccepted, StateError> {
        let source = self.source(&request.world)?;
        let state = u16::try_from(request.state_id)
            .map_err(|_| StateError::unknown_block_state(request.state_id))?;
        if ferrite_data::blocks::state(state).is_none() {
            return Err(StateError::unknown_block_state(request.state_id));
        }
        if !source.chunk_resident(request.x.div_euclid(16), request.z.div_euclid(16)) {
            return Err(StateError::chunk_not_loaded(
                request.x.div_euclid(16),
                request.z.div_euclid(16),
            ));
        }
        if let Some(expected) = request.expected_state_id
            && source.block(request.x, request.y, request.z) != Some(expected)
        {
            return Err(StateError::stale_request(
                expected,
                source.block(request.x, request.y, request.z).unwrap_or(0),
            ));
        }
        let mut pending = self
            .pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if pending.len() >= MAX_PENDING_BLOCK_WRITES {
            return Err(StateError::queue_full());
        }
        pending.push_back(QueuedWrite {
            source,
            x: request.x,
            y: request.y,
            z: request.z,
            state_id: request.state_id,
            expected_state_id: request.expected_state_id,
        });
        Ok(BlockSetAccepted {
            queued: true,
            request_id: self.next_request.fetch_add(1, Ordering::SeqCst) + 1,
        })
    }

    fn chunk_loaded(&self, world: &str, x: i32, z: i32) -> Result<bool, StateError> {
        Ok(self.source(world)?.chunk_resident(x, z))
    }

    fn supported_ops(&self) -> Vec<&'static str> {
        vec![
            "world.block_get",
            "world.block_set_async",
            "world.chunk_loaded",
        ]
    }

    fn tick(&self) {
        self.drain();
    }

    fn shutdown(&self) {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn provider() -> (WorldStateProvider, Arc<AnvilChunkSource>) {
        let source = AnvilChunkSource::new_memory();
        assert!(source.ensure_chunk_loaded(0, 0, Duration::from_secs(10)));
        let dimensions =
            DimensionRegistry::with_source(Dimension::Overworld, Arc::clone(&source), [0, 65, 0]);
        (WorldStateProvider::new(dimensions), source)
    }

    #[tokio::test]
    async fn reads_real_source_and_applies_async_write_only_on_tick() {
        let (provider, source) = provider();
        let before = provider
            .block_get(&BlockGetRequest {
                world: "minecraft:overworld".into(),
                x: 0,
                y: 64,
                z: 0,
            })
            .expect("loaded block read");
        assert!(before.loaded);

        let stone = u32::from(ferrite_data::blocks::default_state("minecraft:stone").unwrap());
        let accepted = provider
            .block_set_async(&BlockSetRequest {
                world: "minecraft:overworld".into(),
                x: 0,
                y: 64,
                z: 0,
                state_id: stone,
                expected_state_id: Some(before.state_id),
            })
            .expect("write accepted");
        assert!(accepted.queued);
        assert_eq!(accepted.request_id, 1);
        assert_ne!(
            source.block(0, 64, 0),
            Some(stone),
            "acceptance is not application"
        );

        provider.tick();
        assert_eq!(source.block(0, 64, 0), Some(stone));
    }

    #[tokio::test]
    async fn unknown_world_state_and_stale_guard_are_explicit() {
        let (provider, _) = provider();
        assert_eq!(
            provider
                .block_get(&BlockGetRequest {
                    world: "minecraft:nowhere".into(),
                    x: 0,
                    y: 64,
                    z: 0,
                })
                .unwrap_err()
                .code,
            "unknown_world"
        );
        assert_eq!(
            provider
                .block_set_async(&BlockSetRequest {
                    world: "minecraft:overworld".into(),
                    x: 0,
                    y: 64,
                    z: 0,
                    state_id: 65535,
                    expected_state_id: None,
                })
                .unwrap_err()
                .code,
            "unknown_block_state"
        );
        assert_eq!(
            provider
                .block_set_async(&BlockSetRequest {
                    world: "minecraft:overworld".into(),
                    x: 0,
                    y: 64,
                    z: 0,
                    state_id: 1,
                    expected_state_id: Some(0),
                })
                .unwrap_err()
                .code,
            "stale_request"
        );
    }
}
