//! Transactional runtime-world unload and the shutdown flush for every world.
//!
//! A scheduler can only be stopped by consuming it, so unload is a transaction in three steps:
//! stop the scheduler, persist entities and chunks, and only then forget the world. When
//! persistence fails the scheduler is rebuilt (held chunks, entities and queued tasks are put
//! back) and the world stays catalogued and usable, so nothing is lost silently.

use super::dimension::{DimensionRegistry, RuntimeWorldCatalog, RuntimeWorldDescriptor};
use super::entity_save::{self, ChunkEntities};
use super::region::{Region, TickContext};
use super::{GlobalRegion, WorldId, Worlds};
use ferrite_region::{ChunkPos, RegionTask, Shutdown};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

/// Everything needed to rebuild a world that was stopped for an unload that then failed.
struct Salvage {
    holds: HashMap<ChunkPos, usize>,
    entities: ChunkEntities,
    pending: Vec<(ChunkPos, RegionTask<Region>)>,
}

/// Unloads a runtime world atomically. `Ok(true)` means the world is gone, `Ok(false)` that it
/// was not loaded. On `Err` the world is still catalogued with a running scheduler, unless the
/// error says the scheduler could not be restored.
pub(super) async fn unload_runtime_world(
    worlds: &mut Worlds,
    catalog: &mut RuntimeWorldCatalog,
    id: Uuid,
    save: bool,
    players_inside: bool,
    make_ctx: impl Fn(&RuntimeWorldDescriptor) -> TickContext,
    view: Option<&Arc<crate::plugins::SourceWorldView>>,
) -> anyhow::Result<bool> {
    let Some(descriptor) = catalog.get(id).cloned() else {
        return Ok(false);
    };
    if players_inside {
        anyhow::bail!("cannot unload a runtime world while players are inside it");
    }
    // Resolve the storage root before stopping anything, so a missing path cannot strand the world.
    let root = if save {
        Some(
            descriptor
                .source
                .path()
                .ok_or_else(|| anyhow::anyhow!("runtime world has no persistent path"))?
                .to_path_buf(),
        )
    } else {
        None
    };
    let Some(mut stopped) = worlds.remove_runtime_world(id) else {
        return Ok(false);
    };
    let mut holds: HashMap<ChunkPos, usize> = HashMap::new();
    for region in &stopped.regions {
        for (chunk, count) in region.held_chunk_counts() {
            *holds.entry(chunk).or_default() += count;
        }
    }
    let entities = entity_save::drain_regions(&mut stopped.regions);
    let persisted = async {
        if let Some(root) = &root {
            let entity_root = descriptor.dimension.storage_root(root);
            entity_save::save_chunks(&entity_root, &entities)?;
            descriptor.source.flush_quiesced().await?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if let Err(error) = persisted {
        let salvage = Salvage {
            holds,
            entities,
            pending: stopped.pending_tasks,
        };
        return Err(if restore(worlds, id, make_ctx(&descriptor), salvage) {
            error.context("runtime world unload aborted; the world was restored")
        } else {
            error.context("runtime world unload failed and its scheduler could not be restored")
        });
    }
    catalog.unload(id);
    if let Some(view) = view {
        view.unregister_world(id);
    }
    Ok(true)
}

fn restore(worlds: &mut Worlds, id: Uuid, ctx: TickContext, salvage: Salvage) -> bool {
    if !worlds.insert_runtime_scheduler(id, ctx) {
        return false;
    }
    let Salvage {
        mut holds,
        mut entities,
        pending,
    } = salvage;
    // An entity whose chunk was not held still needs an owner to live in.
    for &(x, z) in entities.keys() {
        holds.entry(ChunkPos(x, z)).or_insert(1);
    }
    let scheduler = worlds.scheduler_world(WorldId::Runtime(id));
    scheduler.add_chunks(holds.keys().copied());
    let tasks = holds.into_iter().map(|(chunk, count)| {
        let records = entities.remove(&(chunk.0, chunk.1)).unwrap_or_default();
        let task: RegionTask<Region> = Box::new(move |region| {
            for _ in 0..count {
                region.hold_chunk(chunk);
            }
            region.add_entities(records);
        });
        (chunk, task)
    });
    scheduler.schedule_at_chunks(tasks);
    scheduler.schedule_at_chunks(pending);
    true
}

/// Saves every stopped world: vanilla dimensions and runtime worlds each get their own entity
/// directory and chunk source. Every world is attempted even when another fails; all failures
/// are reported together.
pub(super) async fn persist_shutdown(
    stopped: HashMap<WorldId, Shutdown<Region, GlobalRegion>>,
    entity_world: Option<&Path>,
    dimensions: &DimensionRegistry,
    catalog: &RuntimeWorldCatalog,
) -> Result<(), String> {
    let mut errors = Vec::new();
    let mut sources = Vec::new();
    for (world, mut shutdown) in stopped {
        let (root, source) = match world {
            WorldId::Vanilla(dimension) => (
                entity_world.map(|dir| dimension.storage_root(dir)),
                Some(Arc::clone(dimensions.source(dimension))),
            ),
            WorldId::Runtime(id) => match catalog.get(id) {
                Some(descriptor) => (
                    descriptor
                        .source
                        .path()
                        .map(|path| descriptor.dimension.storage_root(path)),
                    Some(Arc::clone(&descriptor.source)),
                ),
                None => {
                    errors.push(format!(
                        "runtime world {id} is not catalogued; its entities were not saved"
                    ));
                    (None, None)
                }
            },
        };
        if let Some(root) = root
            && let Err(error) = entity_save::save_regions(&root, &mut shutdown.regions)
        {
            errors.push(format!(
                "failed to save entities of {world:?} during shutdown: {error:#}"
            ));
        }
        sources.extend(source);
    }
    // Sources can be shared (the primary source is also the overworld's); flush each once.
    let mut flushed: BTreeSet<usize> = BTreeSet::new();
    for source in sources {
        if flushed.insert(Arc::as_ptr(&source) as usize)
            && let Err(error) = source.flush_all().await
        {
            errors.push(format!(
                "failed to save world chunks during shutdown: {error:#}"
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Dimension;
    use crate::game::anvil::AnvilChunkSource;
    use crate::game::dimension::RuntimeWorldRequest;
    use ferrite_entity::{
        Rng,
        mob::{self, MobKind},
    };
    use ferrite_math::Vec3;
    use ferrite_region::{Regionizer, Scheduler};
    use ferrite_world::RuntimeRegistry;

    fn vanilla_root() -> Option<std::path::PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3/generated");
        root.join("reports/blocks.json").exists().then_some(root)
    }

    fn scheduler() -> Scheduler<Region, GlobalRegion> {
        Scheduler::new(
            Regionizer::<Region>::new(0, 1),
            GlobalRegion::default(),
            1,
            |_| {},
            |_| {},
        )
    }

    fn worlds() -> Worlds {
        Worlds {
            schedulers: [(WorldId::Vanilla(Dimension::Overworld), scheduler())]
                .into_iter()
                .collect(),
            scheduler_factory: Arc::new(|_, _| scheduler()),
            transient_entities: None,
            persistent_entities: false,
            tracking: Default::default(),
        }
    }

    fn ctx(descriptor: &RuntimeWorldDescriptor) -> TickContext {
        let (signals, _rx) = tokio::sync::mpsc::unbounded_channel();
        TickContext {
            signals,
            tracking: Default::default(),
            positions: Default::default(),
            source: Some(Arc::clone(&descriptor.source)),
            dimension: descriptor.dimension,
            world_id: WorldId::Runtime(descriptor.id),
            raining: false,
            rules: Default::default(),
            spawn: [0.0, 65.0, 0.0],
            command_entities: Default::default(),
            state: Default::default(),
            trader_spawner: Default::default(),
            profiler: None,
            tree: None,
            hooks: None,
        }
    }

    async fn await_chunk(source: &Arc<AnvilChunkSource>, x: i32, z: i32) {
        loop {
            if let Some(result) = source.get(x, z, 0) {
                result.expect("chunk load");
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    fn run_in_chunk<T: Send + 'static>(
        worlds: &Worlds,
        id: Uuid,
        task: impl FnOnce(&mut Region) -> T + Send + 'static,
    ) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        worlds
            .scheduler_world(WorldId::Runtime(id))
            .schedule_at_chunk(ChunkPos(0, 0), move |region| {
                let _ = tx.send(task(region));
            });
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("region scheduler ran the task")
    }

    /// Opens the runtime world "extra", starts its scheduler, and puts one cow in chunk (0, 0).
    async fn setup(
        root: &Path,
        registry: &Arc<RuntimeRegistry>,
        worlds: &mut Worlds,
        catalog: &mut RuntimeWorldCatalog,
    ) -> (RuntimeWorldDescriptor, Uuid) {
        let descriptor = catalog
            .open_persistent(
                "extra",
                root,
                Arc::clone(registry),
                None,
                RuntimeWorldRequest {
                    dimension: Dimension::Overworld,
                    spawn: [0, 65, 0],
                    seed: 0,
                },
            )
            .unwrap();
        assert!(worlds.insert_runtime_scheduler(descriptor.id, ctx(&descriptor)));
        worlds
            .scheduler_world(WorldId::Runtime(descriptor.id))
            .add_chunk(ChunkPos(0, 0));
        let mut rng = Rng::default();
        let cow = mob::new_mob(MobKind::Cow, Vec3::new(8.5, 64.0, 8.5), &mut rng);
        let cow_uuid = cow.data.uuid;
        run_in_chunk(worlds, descriptor.id, move |region| {
            region.hold_chunk(ChunkPos(0, 0));
            region.entities.insert(cow);
        });
        await_chunk(&descriptor.source, 0, 0).await;
        assert_ne!(
            descriptor.source.block(1, 64, 1),
            Some(1),
            "the edit below must differ from generated terrain"
        );
        descriptor.source.set_block(1, 64, 1, 1).unwrap();
        (descriptor, cow_uuid)
    }

    #[tokio::test]
    async fn failed_flush_keeps_the_runtime_world_usable() {
        let Some(vanilla) = vanilla_root() else {
            return;
        };
        let registry = Arc::new(RuntimeRegistry::load(vanilla).unwrap());
        let dir = tempfile::tempdir().unwrap();
        let mut worlds = worlds();
        let mut catalog = RuntimeWorldCatalog::default();
        let (descriptor, cow) = setup(dir.path(), &registry, &mut worlds, &mut catalog).await;
        let id = descriptor.id;

        // A regular file where the entity directory belongs makes every entity write fail.
        let blocker = dir.path().join("extra").join(entity_save::DIRECTORY);
        std::fs::write(&blocker, b"not a directory").unwrap();

        let error = unload_runtime_world(&mut worlds, &mut catalog, id, true, false, &ctx, None)
            .await
            .expect_err("the flush must fail loudly");
        assert!(format!("{error:#}").contains("restored"), "{error:#}");
        assert!(catalog.get(id).is_some(), "world stays catalogued");
        assert!(
            worlds.schedulers.contains_key(&WorldId::Runtime(id)),
            "scheduler stays present"
        );

        // The restored world still runs tasks and still owns the cow it had before the attempt.
        let held = run_in_chunk(&worlds, id, |region| {
            (
                region.has_chunk_hold(ChunkPos(0, 0)),
                region
                    .drain_entities()
                    .iter()
                    .map(|record| record.data.uuid)
                    .collect::<Vec<_>>(),
            )
        });
        assert_eq!(held, (true, vec![cow]));

        // With the fault removed and the entity back in place, a retry succeeds.
        std::fs::remove_file(&blocker).unwrap();
        run_in_chunk(&worlds, id, |region| {
            let mut rng = Rng::default();
            region.entities.insert(mob::new_mob(
                MobKind::Cow,
                Vec3::new(8.5, 64.0, 8.5),
                &mut rng,
            ));
        });
        assert!(
            unload_runtime_world(&mut worlds, &mut catalog, id, true, false, &ctx, None)
                .await
                .unwrap()
        );
        assert!(catalog.get(id).is_none());
        assert_eq!(
            entity_save::load(&dir.path().join("extra"), (0, 0)).len(),
            1
        );
        worlds.shutdown();
    }

    /// The world is catalogued, has a running scheduler, still holds chunk (0, 0) and still owns
    /// exactly the given cow. The entities are put back, so the check does not change the world.
    fn assert_world_intact(worlds: &Worlds, catalog: &RuntimeWorldCatalog, id: Uuid, cow: Uuid) {
        assert!(catalog.get(id).is_some(), "world stays catalogued");
        assert!(
            worlds.schedulers.contains_key(&WorldId::Runtime(id)),
            "scheduler stays present"
        );
        let held = run_in_chunk(worlds, id, |region| {
            let records = region.drain_entities();
            let uuids: Vec<_> = records.iter().map(|record| record.data.uuid).collect();
            region.add_entities(records);
            (region.has_chunk_hold(ChunkPos(0, 0)), uuids)
        });
        assert_eq!(held, (true, vec![cow]));
    }

    #[tokio::test]
    async fn unload_with_a_player_inside_is_refused_and_changes_nothing() {
        let Some(vanilla) = vanilla_root() else {
            return;
        };
        let registry = Arc::new(RuntimeRegistry::load(vanilla).unwrap());
        let dir = tempfile::tempdir().unwrap();
        let mut worlds = worlds();
        let mut catalog = RuntimeWorldCatalog::default();
        let (descriptor, cow) = setup(dir.path(), &registry, &mut worlds, &mut catalog).await;
        let id = descriptor.id;

        let error = unload_runtime_world(&mut worlds, &mut catalog, id, true, true, &ctx, None)
            .await
            .expect_err("a world with a player inside must not unload");
        assert!(
            format!("{error:#}").contains("players are inside"),
            "{error:#}"
        );
        // Post-condition: the world is exactly as it was, and nothing was written for it.
        assert_world_intact(&worlds, &catalog, id, cow);
        assert!(
            entity_save::load(&dir.path().join("extra"), (0, 0)).is_empty(),
            "a refused unload writes no entity sidecar"
        );

        // Once the player has left, the same world unloads normally.
        assert!(
            unload_runtime_world(&mut worlds, &mut catalog, id, true, false, &ctx, None)
                .await
                .unwrap()
        );
        assert!(catalog.get(id).is_none());
        assert_eq!(
            entity_save::load(&dir.path().join("extra"), (0, 0)).len(),
            1
        );
        worlds.shutdown();
    }

    #[tokio::test]
    async fn unload_without_save_skips_persistence_and_leaves_no_half_removed_world() {
        let Some(vanilla) = vanilla_root() else {
            return;
        };
        let registry = Arc::new(RuntimeRegistry::load(vanilla).unwrap());
        let dir = tempfile::tempdir().unwrap();
        let mut worlds = worlds();
        let mut catalog = RuntimeWorldCatalog::default();
        let (descriptor, _cow) = setup(dir.path(), &registry, &mut worlds, &mut catalog).await;
        let id = descriptor.id;

        // Even a broken entity directory cannot matter: nothing is written when save is false.
        let blocker = dir.path().join("extra").join(entity_save::DIRECTORY);
        std::fs::write(&blocker, b"not a directory").unwrap();

        assert!(
            unload_runtime_world(&mut worlds, &mut catalog, id, false, false, &ctx, None)
                .await
                .expect("save=false does not touch the disk, so it cannot fail on it")
        );
        // Removed completely: catalog and scheduler agree, and the name is free again.
        assert!(catalog.get(id).is_none());
        assert!(catalog.by_name("extra").is_none());
        assert!(!worlds.schedulers.contains_key(&WorldId::Runtime(id)));
        // Nothing was persisted: the blocker is still the untouched file and no region exists.
        assert_eq!(std::fs::read(&blocker).unwrap(), b"not a directory");
        drop(descriptor);
        std::fs::remove_file(&blocker).unwrap();
        let reopened = catalog
            .open_persistent(
                "extra",
                dir.path(),
                Arc::clone(&registry),
                None,
                RuntimeWorldRequest {
                    dimension: Dimension::Overworld,
                    spawn: [0, 65, 0],
                    seed: 0,
                },
            )
            .expect("the unloaded name can be opened again");
        assert_eq!(reopened.id, id, "the world identity is kept");
        await_chunk(&reopened.source, 0, 0).await;
        assert_ne!(
            reopened.source.block(1, 64, 1),
            Some(1),
            "the unsaved block edit did not reach the disk"
        );
        assert!(
            entity_save::load(&dir.path().join("extra"), (0, 0)).is_empty(),
            "the unsaved cow did not reach the disk"
        );
        worlds.shutdown();
    }

    #[tokio::test]
    async fn failed_chunk_flush_restores_the_scheduler_and_catalog_entry() {
        let Some(vanilla) = vanilla_root() else {
            return;
        };
        let registry = Arc::new(RuntimeRegistry::load(vanilla).unwrap());
        let dir = tempfile::tempdir().unwrap();
        let mut worlds = worlds();
        let mut catalog = RuntimeWorldCatalog::default();
        let (descriptor, cow) = setup(dir.path(), &registry, &mut worlds, &mut catalog).await;
        let id = descriptor.id;

        // The entity sidecars write fine; only the region files cannot be created.
        let blocker = dir.path().join("extra").join("dimensions");
        std::fs::write(&blocker, b"not a directory").unwrap();

        let error = unload_runtime_world(&mut worlds, &mut catalog, id, true, false, &ctx, None)
            .await
            .expect_err("the chunk flush must fail loudly");
        assert!(format!("{error:#}").contains("restored"), "{error:#}");
        assert_world_intact(&worlds, &catalog, id, cow);
        // The unsaved edit is still in memory, ready for the retry.
        assert_eq!(descriptor.source.block(1, 64, 1), Some(1));

        std::fs::remove_file(&blocker).unwrap();
        run_in_chunk(&worlds, id, move |region| {
            let mut rng = Rng::default();
            let mut again = mob::new_mob(MobKind::Cow, Vec3::new(8.5, 64.0, 8.5), &mut rng);
            again.data.uuid = cow;
            region.entities.insert(again);
        });
        assert!(
            unload_runtime_world(&mut worlds, &mut catalog, id, true, false, &ctx, None)
                .await
                .unwrap()
        );
        assert!(catalog.get(id).is_none());
        worlds.shutdown();
    }

    #[tokio::test]
    async fn shutdown_then_reopen_preserves_runtime_chunks_and_entities() {
        let Some(vanilla) = vanilla_root() else {
            return;
        };
        let registry = Arc::new(RuntimeRegistry::load(vanilla).unwrap());
        let dir = tempfile::tempdir().unwrap();
        let mut worlds = worlds();
        let mut catalog = RuntimeWorldCatalog::default();
        let (descriptor, cow) = setup(dir.path(), &registry, &mut worlds, &mut catalog).await;
        let id = descriptor.id;

        let dimensions = DimensionRegistry::standard(None, Arc::clone(&registry), None, [0, 65, 0]);
        let stopped = worlds.shutdown();
        assert!(stopped.contains_key(&WorldId::Runtime(id)));
        persist_shutdown(stopped, None, &dimensions, &catalog)
            .await
            .expect("shutdown persists every runtime world");
        drop(descriptor);
        catalog.unload(id);

        let mut reopened_catalog = RuntimeWorldCatalog::default();
        let reopened = reopened_catalog
            .open_persistent(
                "extra",
                dir.path(),
                registry,
                None,
                RuntimeWorldRequest {
                    dimension: Dimension::Overworld,
                    spawn: [9, 9, 9],
                    seed: 0,
                },
            )
            .unwrap();
        assert_eq!(reopened.id, id);
        await_chunk(&reopened.source, 0, 0).await;
        assert_eq!(
            reopened.source.block(1, 64, 1),
            Some(1),
            "chunk edit survived shutdown"
        );
        let entities = entity_save::load(&dir.path().join("extra"), (0, 0));
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].data.uuid, cow);
    }
}
