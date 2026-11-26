//! Multi-layer `ServerHooks` dispatcher combining zero-overhead Rust native plugins with the
//! optional embedded JVM plugin host.

use std::sync::Arc;

use crate::game::plugin_types::{
    BlockBreak, BlockInteract, BlockPosition, CommandSender, EntityTask, Error, InteractAction,
    ItemSnapshot, PlayerDeath, PlayerDeathOutcome, PlayerLocation, PlayerMove, PlayerSnapshot,
    PluginCommand, PluginCommandResult, RegionTask, ServerHooks, WorldSnapshot,
};

/// Dispatches `ServerHooks` calls across an ordered list of layers (for example: zero-overhead
/// Rust native plugins first, followed by the optional embedded JVM `HostHooks`).
///
/// Cancellable hooks short-circuit as soon as an earlier layer cancels the action, so a native
/// Rust region-protection or mute plugin can reject an event without ever crossing JNI.
pub struct CompositeHooks {
    layers: Vec<Arc<dyn ServerHooks>>,
}

impl CompositeHooks {
    /// Builds a composite hook stack from ordered layers.
    pub fn new(layers: Vec<Arc<dyn ServerHooks>>) -> Self {
        Self { layers }
    }

    /// Combines optional layers, returning `None` when no layer is active or a direct `Arc` when
    /// only a single layer is active (avoiding any extra indirection).
    pub fn combine(layers: Vec<Arc<dyn ServerHooks>>) -> Option<Arc<dyn ServerHooks>> {
        match layers.len() {
            0 => None,
            1 => layers.into_iter().next(),
            _ => Some(Arc::new(Self::new(layers))),
        }
    }
}

impl ServerHooks for CompositeHooks {
    fn player_joined(&self, id: uuid::Uuid, name: &str) -> Result<(), Error> {
        for layer in &self.layers {
            layer.player_joined(id, name)?;
        }
        Ok(())
    }

    fn player_left(&self, id: uuid::Uuid, name: &str) -> Result<(), Error> {
        for layer in &self.layers {
            layer.player_left(id, name)?;
        }
        Ok(())
    }

    fn chat(&self, id: uuid::Uuid, name: &str, message: &str) -> Result<Option<String>, Error> {
        let mut current = message.to_owned();
        for layer in &self.layers {
            match layer.chat(id, name, &current)? {
                Some(next) => current = next,
                None => return Ok(None),
            }
        }
        Ok(Some(current))
    }

    fn player_moved(
        &self,
        id: uuid::Uuid,
        name: &str,
        movement: &PlayerMove,
    ) -> Result<bool, Error> {
        for layer in &self.layers {
            if !layer.player_moved(id, name, movement)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn player_interacted(
        &self,
        id: uuid::Uuid,
        name: &str,
        action: InteractAction,
        event: BlockInteract<'_>,
    ) -> Result<bool, Error> {
        for layer in &self.layers {
            if !layer.player_interacted(id, name, action, event)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn block_broken(
        &self,
        id: uuid::Uuid,
        name: &str,
        event: BlockBreak<'_>,
    ) -> Result<bool, Error> {
        for layer in &self.layers {
            if !layer.block_broken(id, name, event)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn block_placed(
        &self,
        id: uuid::Uuid,
        name: &str,
        block: BlockPosition,
        face: u8,
        placed: &str,
        replaced: &str,
    ) -> Result<bool, Error> {
        for layer in &self.layers {
            if !layer.block_placed(id, name, block, face, placed, replaced)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn entity_changed_block(
        &self,
        id: uuid::Uuid,
        kind: &str,
        world: uuid::Uuid,
        block: BlockPosition,
        from: &str,
        to: &str,
    ) -> Result<bool, Error> {
        for layer in &self.layers {
            if !layer.entity_changed_block(id, kind, world, block, from, to)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn blocks_exploded(
        &self,
        world: uuid::Uuid,
        center: BlockPosition,
        blocks: &[BlockPosition],
        yield_ratio: f32,
    ) -> Result<Option<Vec<BlockPosition>>, Error> {
        let mut modified: Option<Vec<BlockPosition>> = None;
        for layer in &self.layers {
            let input = modified.as_deref().unwrap_or(blocks);
            if let Some(next) = layer.blocks_exploded(world, center, input, yield_ratio)? {
                modified = Some(next);
            }
        }
        Ok(modified)
    }

    fn mob_spawn_allowed(&self, world: &str, x: i32, y: i32, z: i32) -> Result<bool, Error> {
        for layer in &self.layers {
            if !layer.mob_spawn_allowed(world, x, y, z)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn fire_spread_allowed(&self, world: &str, x: i32, y: i32, z: i32) -> Result<bool, Error> {
        for layer in &self.layers {
            if !layer.fire_spread_allowed(world, x, y, z)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn player_command(&self, id: uuid::Uuid, name: &str, message: &str) -> Result<bool, Error> {
        for layer in &self.layers {
            if !layer.player_command(id, name, message)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn chunk_loaded(&self, world: uuid::Uuid, x: i32, z: i32, new: bool) -> Result<(), Error> {
        for layer in &self.layers {
            layer.chunk_loaded(world, x, z, new)?;
        }
        Ok(())
    }

    fn chunk_unloaded(&self, world: uuid::Uuid, x: i32, z: i32) -> Result<(), Error> {
        for layer in &self.layers {
            layer.chunk_unloaded(world, x, z)?;
        }
        Ok(())
    }

    fn player_died(
        &self,
        id: uuid::Uuid,
        name: &str,
        cause: &str,
        death: &PlayerDeath,
    ) -> Result<PlayerDeathOutcome, Error> {
        let mut state = death.clone();
        let mut outcome = PlayerDeathOutcome {
            drops: death.drops.clone(),
            dropped_exp: death.dropped_exp,
            keep_inventory: death.keep_inventory,
        };
        for layer in &self.layers {
            outcome = layer.player_died(id, name, cause, &state)?;
            state.drops = outcome.drops.clone();
            state.dropped_exp = outcome.dropped_exp;
            state.keep_inventory = outcome.keep_inventory;
        }
        Ok(outcome)
    }

    fn player_respawned(
        &self,
        id: uuid::Uuid,
        name: &str,
        spawn: [f64; 3],
        yaw: f32,
        pitch: f32,
    ) -> Result<[f64; 3], Error> {
        let mut current = spawn;
        for layer in &self.layers {
            current = layer.player_respawned(id, name, current, yaw, pitch)?;
        }
        Ok(current)
    }

    fn entity_damaged(
        &self,
        id: uuid::Uuid,
        name: &str,
        cause: &str,
        amount: f64,
    ) -> Result<f64, Error> {
        let mut current = amount;
        for layer in &self.layers {
            current = layer.entity_damaged(id, name, cause, current)?;
            if current <= 0.0 {
                return Ok(0.0);
            }
        }
        Ok(current)
    }

    fn entity_died(
        &self,
        id: uuid::Uuid,
        kind: &str,
        position: [f64; 3],
        dropped_exp: i32,
    ) -> Result<(), Error> {
        for layer in &self.layers {
            layer.entity_died(id, kind, position, dropped_exp)?;
        }
        Ok(())
    }

    fn weather_changing(&self, world: uuid::Uuid, to_weather_state: bool) -> Result<bool, Error> {
        for layer in &self.layers {
            if !layer.weather_changing(world, to_weather_state)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn world_loaded(&self, world: uuid::Uuid) -> Result<(), Error> {
        for layer in &self.layers {
            layer.world_loaded(world)?;
        }
        Ok(())
    }

    fn player_level_changed(
        &self,
        id: uuid::Uuid,
        name: &str,
        old_level: i32,
        new_level: i32,
    ) -> Result<i32, Error> {
        let mut current = new_level;
        for layer in &self.layers {
            current = layer.player_level_changed(id, name, old_level, current)?;
        }
        Ok(current)
    }

    fn player_exp_changed(&self, id: uuid::Uuid, name: &str, amount: i32) -> Result<i32, Error> {
        let mut current = amount;
        for layer in &self.layers {
            current = layer.player_exp_changed(id, name, current)?;
        }
        Ok(current)
    }

    fn inventory_clicked(
        &self,
        id: uuid::Uuid,
        name: &str,
        slot: i32,
        click: &str,
        action: &str,
    ) -> Result<bool, Error> {
        for layer in &self.layers {
            if !layer.inventory_clicked(id, name, slot, click, action)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn inventory_dragged(
        &self,
        id: uuid::Uuid,
        name: &str,
        old_cursor: &ItemSnapshot,
        new_cursor: &ItemSnapshot,
        items: &[(i32, ItemSnapshot)],
    ) -> Result<bool, Error> {
        for layer in &self.layers {
            if !layer.inventory_dragged(id, name, old_cursor, new_cursor, items)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn inventory_closed(&self, id: uuid::Uuid, name: &str) -> Result<(), Error> {
        for layer in &self.layers {
            layer.inventory_closed(id, name)?;
        }
        Ok(())
    }

    fn resource_pack_status(
        &self,
        player: uuid::Uuid,
        name: &str,
        pack: uuid::Uuid,
        action: i32,
    ) -> Result<(), Error> {
        for layer in &self.layers {
            layer.resource_pack_status(player, name, pack, action)?;
        }
        Ok(())
    }

    fn plugin_message_received(
        &self,
        player: uuid::Uuid,
        name: &str,
        channel: &str,
        data: &[u8],
    ) -> Result<bool, Error> {
        let mut accepted = false;
        for layer in &self.layers {
            if layer.plugin_message_received(player, name, channel, data)? {
                accepted = true;
            }
        }
        Ok(accepted)
    }

    fn plugin_commands(&self) -> Result<Vec<PluginCommand>, Error> {
        let mut merged = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for layer in &self.layers {
            for command in layer.plugin_commands()? {
                if seen.insert(command.name.to_ascii_lowercase()) {
                    merged.push(command);
                }
            }
        }
        Ok(merged)
    }

    fn tick(&self) -> Result<(), Error> {
        for layer in &self.layers {
            layer.tick()?;
        }
        Ok(())
    }

    fn register_world(&self, world: &WorldSnapshot) -> Result<(), Error> {
        for layer in &self.layers {
            layer.register_world(world)?;
        }
        Ok(())
    }

    fn unregister_world(&self, id: uuid::Uuid) -> Result<bool, Error> {
        for layer in &self.layers {
            if !layer.unregister_world(id)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn register_player(&self, player: &PlayerSnapshot) -> Result<(), Error> {
        for layer in &self.layers {
            layer.register_player(player)?;
        }
        Ok(())
    }

    fn inventory_slot_changed(
        &self,
        id: uuid::Uuid,
        slot: u8,
        item: Option<&ItemSnapshot>,
        data: Option<&str>,
    ) -> Result<(), Error> {
        for layer in &self.layers {
            layer.inventory_slot_changed(id, slot, item, data)?;
        }
        Ok(())
    }

    fn run_plugin_command(
        &self,
        sender: CommandSender,
        line: &str,
    ) -> Result<PluginCommandResult, Error> {
        for layer in &self.layers {
            let outcome = layer.run_plugin_command(sender, line)?;
            if outcome.handled {
                return Ok(outcome);
            }
        }
        Ok(PluginCommandResult {
            handled: false,
            messages: Vec::new(),
        })
    }

    fn take_region_tasks(&self) -> Vec<RegionTask> {
        self.layers
            .iter()
            .flat_map(|layer| layer.take_region_tasks())
            .collect()
    }

    fn run_region_task(&self, task: RegionTask) -> Result<(), Error> {
        for layer in &self.layers {
            layer.run_region_task(task.clone())?;
        }
        Ok(())
    }

    fn take_entity_tasks(&self) -> Vec<EntityTask> {
        self.layers
            .iter()
            .flat_map(|layer| layer.take_entity_tasks())
            .collect()
    }

    fn run_entity_task(&self, task: EntityTask) -> Result<(), Error> {
        for layer in &self.layers {
            layer.run_entity_task(task.clone())?;
        }
        Ok(())
    }

    fn update_player_location(
        &self,
        id: uuid::Uuid,
        location: &PlayerLocation,
    ) -> Result<(), Error> {
        for layer in &self.layers {
            layer.update_player_location(id, location)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::plugin_types::{MessageContent, PluginMessage};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct StubLayer {
        cancel_break: bool,
        command: &'static str,
        calls: Arc<AtomicUsize>,
    }

    impl ServerHooks for StubLayer {
        fn player_joined(&self, _: uuid::Uuid, _: &str) -> Result<(), Error> {
            Ok(())
        }
        fn player_left(&self, _: uuid::Uuid, _: &str) -> Result<(), Error> {
            Ok(())
        }
        fn chat(&self, _: uuid::Uuid, _: &str, msg: &str) -> Result<Option<String>, Error> {
            Ok(Some(msg.to_owned()))
        }
        fn player_moved(&self, _: uuid::Uuid, _: &str, _: &PlayerMove) -> Result<bool, Error> {
            Ok(true)
        }
        fn player_interacted(
            &self,
            _: uuid::Uuid,
            _: &str,
            _: InteractAction,
            _: BlockInteract<'_>,
        ) -> Result<bool, Error> {
            Ok(true)
        }
        fn block_broken(&self, _: uuid::Uuid, _: &str, _: BlockBreak<'_>) -> Result<bool, Error> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(!self.cancel_break)
        }
        fn block_placed(
            &self,
            _: uuid::Uuid,
            _: &str,
            _: BlockPosition,
            _: u8,
            _: &str,
            _: &str,
        ) -> Result<bool, Error> {
            Ok(true)
        }
        fn player_command(&self, _: uuid::Uuid, _: &str, _: &str) -> Result<bool, Error> {
            Ok(true)
        }
        fn plugin_commands(&self) -> Result<Vec<PluginCommand>, Error> {
            Ok(vec![PluginCommand {
                name: self.command.to_owned(),
                aliases: Vec::new(),
                permission: None,
                usage: None,
            }])
        }
        fn run_plugin_command(
            &self,
            sender: CommandSender,
            line: &str,
        ) -> Result<PluginCommandResult, Error> {
            if line.trim_start_matches('/').starts_with(self.command) {
                let player = match sender {
                    CommandSender::Console => uuid::Uuid::nil(),
                    CommandSender::Player(id) => id,
                };
                return Ok(PluginCommandResult {
                    handled: true,
                    messages: vec![PluginMessage {
                        player,
                        content: MessageContent::Plain(format!("handled:{}", self.command)),
                    }],
                });
            }
            Ok(PluginCommandResult {
                handled: false,
                messages: Vec::new(),
            })
        }
    }

    #[test]
    fn earlier_layer_cancel_short_circuits_later_layers_and_merges_commands() {
        let first_calls = Arc::new(AtomicUsize::new(0));
        let second_calls = Arc::new(AtomicUsize::new(0));
        let combined = CompositeHooks::combine(vec![
            Arc::new(StubLayer {
                cancel_break: true,
                command: "rg",
                calls: Arc::clone(&first_calls),
            }),
            Arc::new(StubLayer {
                cancel_break: false,
                command: "hello",
                calls: Arc::clone(&second_calls),
            }),
        ])
        .expect("composite");

        let allowed = combined
            .block_broken(
                uuid::Uuid::from_u128(1),
                "Steve",
                BlockBreak {
                    world: "world",
                    block: BlockPosition { x: 0, y: 64, z: 0 },
                    material: "minecraft:stone",
                },
            )
            .unwrap();
        assert!(!allowed);
        assert_eq!(first_calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            second_calls.load(Ordering::SeqCst),
            0,
            "second layer must not run after first layer cancels"
        );

        let commands: Vec<String> = combined
            .plugin_commands()
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(commands, vec!["rg".to_owned(), "hello".to_owned()]);

        let res = combined
            .run_plugin_command(CommandSender::Console, "hello world")
            .unwrap();
        assert!(res.handled);
        assert_eq!(res.messages.len(), 1);
    }
}
