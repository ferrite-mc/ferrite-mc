//! Vanilla player survival state.
//!
//! Mirrors the 26.3 server classes:
//!
//! * `net.minecraft.world.entity.LivingEntity`: `health`, `absorptionAmount`,
//!   `invulnerableTime` and `hurt` (absorption first, 20-tick invulnerability, death at zero).
//! * `net.minecraft.world.food.FoodData`: `tick` (exhaustion, natural regeneration, starvation),
//!   `eat` and `addExhaustion`; saturation restored is `FoodConstants.saturationByModifier`
//!   (`nutrition * saturationModifier * 2`, capped by the food level).
//! * `net.minecraft.world.entity.player.Player`: `tick` (fire ticks, water air supply,
//!   `lavaHurt`, `belowWorld`), `causeFallDamage` (`ceil(fallDistance - 3)`), and the
//!   movement/action exhaustion causes.
//! * `net.minecraft.server.level.ServerPlayer`: `die`/`respawn` state.
//!
//! The region owns all of this; nothing here touches another player or another region. The
//! packet side (`set_health`, `player_combat_kill`, `respawn`) lives in `region.rs`.

use ferrite_command::gamerule::GameRules;

/// Vanilla `LivingEntity.getMaxHealth()` for a player without attribute modifiers.
pub const MAX_HEALTH: f32 = 20.0;
/// Vanilla `FoodData.MAX_FOOD` / `MAX_SATURATION`.
pub const MAX_FOOD: i32 = 20;
pub const MAX_SATURATION: f32 = 20.0;
/// Vanilla `Player.getMaxAirSupply()` (`300` ticks = 15 s underwater).
pub const MAX_AIR: i32 = 300;
/// Vanilla `LivingEntity.invulnerableDuration` (1 s).
pub const INVULNERABLE_TICKS: i32 = 20;
/// `Entity.setSecondsOnFire(15)` from `Entity.lavaHurt`.
pub const LAVA_FIRE_TICKS: i32 = 300;
/// `Entity.setSecondsOnFire(8)` while standing in a fire block.
pub const FIRE_BLOCK_TICKS: i32 = 160;
/// `FoodData.tick` damage every 80 ticks when the food bar is empty.
pub const STARVE_INTERVAL: i32 = 80;
/// `Player.causeFallDamage`: `safe_fall_distance` attribute default.
pub const SAFE_FALL_DISTANCE: f64 = 3.0;
/// `Player.isBelowWorld`: `level.getMinBuildHeight() - 64` (min height -64 in the overworld).
pub const VOID_Y: f64 = -128.0;
/// `minecraft:consumable` default `consume_seconds` (1.6 s) in ticks.
pub const EAT_TICKS: i32 = 32;
/// A hard cap mirroring vanilla's `FoodData.MAX_EXHAUSTION`-style clamp; vanilla absorbs
/// exhaustion in steps of 4, so this only prevents unbounded growth from a stuck client.
const MAX_EXHAUSTION: f32 = 40.0;

/// Where a point of damage came from. Vanilla models this with `DamageSource`; the server only
/// needs the type tags, the death message and the invulnerability bypass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageKind {
    /// `DamageTypes.FALL`: `ceil(fallDistance - safeFallDistance)`.
    Fall,
    /// `DamageTypes.DROWN`: 2 damage when the air supply reaches -20.
    Drown,
    /// `DamageTypes.LAVA`: 4 damage, `Entity.lavaHurt` (also sets 15 s of fire).
    Lava,
    /// `DamageTypes.ON_FIRE`: 1 damage every 20 fire ticks.
    Fire,
    /// `DamageTypes.FELL_OUT_OF_WORLD`: 4 damage per tick below the world, bypasses invulnerability.
    Void,
    /// `DamageTypes.STARVE`: 1 damage on the starvation timer.
    Starve,
    /// `DamageTypes.MAGIC`: poison and instant damage.
    Magic,
    /// `DamageTypes.WITHER`: the wither effect.
    Wither,
    /// `DamageTypes.EXPLOSION`: blasts and `World#createExplosion`. Bypasses armor, but not
    /// invulnerability; plugins see it as `ENTITY_EXPLOSION` and may cancel or change it.
    Explosion,
    /// `DamageTypes.MOB_ATTACK`: melee damage from a hostile mob.
    Mob,
    /// `DamageTypes.ARROW`: projectile damage.
    Arrow,
    /// `DamageTypes.GENERIC_KILL` (`/kill`): bypasses invulnerability and creative mode.
    GenericKill,
    /// `DamageTypes.GENERIC`: damage a plugin deals without a vanilla cause; armor applies.
    Generic,
}

impl DamageKind {
    /// Whether armour points are ignored (explosions in this damage model).
    pub fn bypasses_armor(self) -> bool {
        matches!(self, Self::Explosion)
    }

    /// `DamageTypeTags.BYPASSES_INVULNERABILITY`: void damage and `/kill`.
    pub fn bypasses_invulnerability(self) -> bool {
        matches!(self, Self::Void | Self::GenericKill)
    }

    /// Whether survival mode should be the only game mode that takes this damage. Creative is
    /// invulnerable to everything except the bypassing sources; spectators take nothing.
    pub fn ignores_creative(self) -> bool {
        !matches!(self, Self::Void | Self::GenericKill)
    }

    /// `Player.deathMessage` for the vanilla `death.attack.*` keys.
    pub fn death_message(self, name: &str) -> String {
        match self {
            Self::Fall => format!("{name} fell from a high place"),
            Self::Drown => format!("{name} drowned"),
            Self::Lava => format!("{name} tried to swim in lava"),
            Self::Fire => format!("{name} burned to death"),
            Self::Void => format!("{name} fell out of the world"),
            Self::Starve => format!("{name} starved to death"),
            Self::Magic => format!("{name} was killed by magic"),
            Self::Wither => format!("{name} withered away"),
            Self::Explosion => format!("{name} blew up"),
            Self::Mob => format!("{name} was slain"),
            Self::Arrow => format!("{name} was shot"),
            Self::GenericKill => format!("{name} was killed"),
            Self::Generic => format!("{name} died"),
        }
    }
}

/// `net.minecraft.world.food.FoodData`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoodData {
    /// `foodLevel`, 0..=20.
    pub food: i32,
    /// `saturationLevel`, 0..=food.
    pub saturation: f32,
    /// `exhaustionLevel`; every 4 points consume one saturation or food point.
    pub exhaustion: f32,
    /// `tickTimer`, shared by regeneration and starvation, like vanilla.
    tick_timer: i32,
}

impl Default for FoodData {
    fn default() -> Self {
        // `ServerPlayer` starts with a full bar and 5 saturation.
        Self {
            food: MAX_FOOD,
            saturation: 5.0,
            exhaustion: 0.0,
            tick_timer: 0,
        }
    }
}

/// What `FoodData.tick` asks the player to do this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FoodTick {
    None,
    /// `LivingEntity.heal(amount)` from natural regeneration.
    Heal(f32),
    /// `hurt(damageSources().starve(), 1)`.
    Starve,
}

impl FoodData {
    /// `FoodData.addExhaustion`: clamped to the vanilla maximum (40).
    pub fn add_exhaustion(&mut self, amount: f32) {
        if amount > 0.0 {
            self.exhaustion = (self.exhaustion + amount).min(MAX_EXHAUSTION);
        }
    }

    /// `FoodData.eat(int food, float saturationModifier)`; saturation is
    /// `FoodConstants.saturationByModifier(food, modifier)` (= nutrition * modifier * 2) capped
    /// by the new food level.
    pub fn eat(&mut self, nutrition: i32, saturation_modifier: f32) {
        self.food = (self.food + nutrition).min(MAX_FOOD);
        let saturation = nutrition as f32 * saturation_modifier * 2.0;
        self.saturation = (self.saturation + saturation)
            .min(MAX_SATURATION)
            .min(self.food as f32);
    }

    /// `FoodData.tick(ServerPlayer)`: drains exhaustion, then natural regeneration or
    /// starvation. `health` is the current health for the `isHurt()` checks.
    ///
    /// Vanilla's difficulty checks live at the call site: on Normal starvation only hurts above
    /// 1 health, so this reports starvation and the caller applies the damage.
    pub fn tick(&mut self, health: f32, natural_regeneration: bool) -> FoodTick {
        if self.exhaustion > 4.0 {
            self.exhaustion -= 4.0;
            if self.saturation > 0.0 {
                self.saturation = (self.saturation - 1.0).max(0.0);
            } else {
                self.food = (self.food - 1).max(0);
            }
        }
        if natural_regeneration && self.saturation > 0.0 && self.food >= 20 && health < MAX_HEALTH {
            self.tick_timer += 1;
            if self.tick_timer >= 10 {
                let heal = self.saturation.min(6.0);
                self.add_exhaustion(heal);
                self.tick_timer = 0;
                return FoodTick::Heal(heal / 6.0);
            }
        } else if natural_regeneration && self.food >= 18 && health < MAX_HEALTH {
            self.tick_timer += 1;
            if self.tick_timer >= 80 {
                self.add_exhaustion(6.0);
                self.tick_timer = 0;
                return FoodTick::Heal(1.0);
            }
        } else if self.food <= 0 {
            self.tick_timer += 1;
            if self.tick_timer >= STARVE_INTERVAL {
                self.tick_timer = 0;
                return FoodTick::Starve;
            }
        } else {
            self.tick_timer = 0;
        }
        FoodTick::None
    }
}

/// The four damage sources one environment tick can produce.
pub type EnvironmentDamage = [Option<(DamageKind, f32)>; 4];

fn queue_damage(queue: &mut EnvironmentDamage, kind: DamageKind, amount: f32) {
    if let Some(slot) = queue.iter_mut().find(|slot| slot.is_none()) {
        *slot = Some((kind, amount));
    }
}

/// Health, hunger and the per-tick environmental trackers of one player.
#[derive(Debug, Clone)]
pub struct Survival {
    pub health: f32,
    /// Vanilla `Attributes.MAX_HEALTH` (health boost raises it above 20).
    pub max_health: f32,
    /// The base value a plugin set; effects add to it.
    pub base_max_health: f32,
    /// The `max_health` the client was last told, so it is resent only when it changes.
    pub sent_max_health: f32,
    /// `LivingEntity.absorptionAmount`.
    pub absorption: f32,
    pub food: FoodData,
    /// `invulnerableTime`; damage is ignored while positive unless the source bypasses it.
    pub invulnerable: i32,
    /// `airSupply`; starts full and drops below zero underwater.
    pub air: i32,
    /// `remainingFireTicks` (`Entity.setRemainingFireTicks`).
    pub fire_ticks: i32,
    /// Distance fallen since leaving the ground (`Entity.fallDistance`).
    pub fall_distance: f64,
    /// The position used to measure movement, jumps and fall distance.
    pub last_pos: [f64; 3],
    pub was_ground: bool,
    pub sprinting: bool,
    /// Below this height the void hurts: the dimension's minimum build height - 64.
    pub void_y: f64,
    /// Damage per tick below `void_y`; `None` once a plugin switched the void off
    /// (`World#setVoidDamageEnabled(false)`).
    pub void_damage: Option<f32>,
    /// `true` once health reached zero (`LivingEntity.isDeadOrDying`).
    pub dead: bool,
    /// Set by the region once the death drops, message and packets ran.
    pub death_processed: bool,
    pub death_cause: Option<DamageKind>,
    /// The finished death message of a `/damage` kill (names the attacker and the damage type's
    /// `message_id`); `None` uses the cause's own text.
    pub death_message: Option<String>,
    /// The held item currently being eaten (`LivingEntity.useItem`).
    pub eating: Option<Eating>,
    last_synced: (f32, i32, f32),
}

/// `LivingEntity.useItem` progress for a food stack in an `Inventory` slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Eating {
    pub slot: usize,
    /// `useItemRemainingTicks`; reaches zero when the item is consumed.
    pub remaining: i32,
}

impl Default for Survival {
    fn default() -> Self {
        Self {
            health: MAX_HEALTH,
            max_health: MAX_HEALTH,
            base_max_health: MAX_HEALTH,
            sent_max_health: MAX_HEALTH,
            absorption: 0.0,
            food: FoodData::default(),
            invulnerable: 0,
            air: MAX_AIR,
            fire_ticks: 0,
            fall_distance: 0.0,
            last_pos: [0.0; 3],
            was_ground: true,
            sprinting: false,
            void_y: VOID_Y,
            void_damage: Some(4.0),
            dead: false,
            death_processed: false,
            death_cause: None,
            death_message: None,
            eating: None,
            last_synced: (MAX_HEALTH, MAX_FOOD, 5.0),
        }
    }
}

impl Survival {
    pub fn new() -> Self {
        Self::default()
    }

    /// `LivingEntity.heal`: clamps at the current maximum health.
    pub fn heal(&mut self, amount: f32) -> bool {
        if self.dead || amount <= 0.0 || self.health >= self.max_health {
            return false;
        }
        self.health = (self.health + amount).min(self.max_health);
        true
    }

    /// `Attributes.MAX_HEALTH` from health boost; clamps the current health down on removal.
    pub fn set_max_health(&mut self, max: f32) {
        self.max_health = max.max(1.0);
        self.health = self.health.min(self.max_health);
    }

    /// `LivingEntity.hurt` after damage-type filtering: absorption absorbs first, the rest is
    /// taken from health, and any accepted hit starts the 20-tick invulnerability window.
    ///
    /// Returns whether the hit landed (absorption still counts as a landed hit).
    pub fn damage(&mut self, kind: DamageKind, amount: f32) -> bool {
        if self.dead || amount <= 0.0 {
            return false;
        }
        if self.invulnerable > 0 && !kind.bypasses_invulnerability() {
            return false;
        }
        let mut remaining = amount;
        if self.absorption > 0.0 {
            let absorbed = remaining.min(self.absorption);
            self.absorption -= absorbed;
            remaining -= absorbed;
        }
        self.invulnerable = INVULNERABLE_TICKS;
        if remaining > 0.0 {
            self.health = (self.health - remaining).max(0.0);
        }
        if self.health <= 0.0 {
            self.health = 0.0;
            self.dead = true;
            self.death_cause = Some(kind);
            self.eating = None;
        }
        true
    }

    /// Marks the current values as sent (`ClientboundSetHealthPacket`).
    pub fn mark_synced(&mut self) {
        self.last_synced = (self.health, self.food.food, self.food.saturation);
    }

    /// Whether the client's health bar is stale.
    pub fn needs_sync(&self) -> bool {
        let current = (self.health, self.food.food, self.food.saturation);
        current.0.to_bits() != self.last_synced.0.to_bits()
            || current.1 != self.last_synced.1
            || current.2.to_bits() != self.last_synced.2.to_bits()
    }

    /// `Player.tick` environmental hazards. `eye_in_water` and `feet_in_lava` come from the
    /// block fluids at the eye and feet; `fire_block` is a fire/soul-fire block at the feet.
    ///
    /// Returns the damage to apply through the player's `hurt`, after this call updated the
    /// air supply, fire ticks and fall distance.
    #[allow(clippy::too_many_arguments)]
    pub fn environment_tick(
        &mut self,
        pos: [f64; 3],
        on_ground: bool,
        eye_in_water: bool,
        feet_in_lava: bool,
        fire_block: bool,
        gamemode: i32,
        sprinting: bool,
    ) -> EnvironmentDamage {
        let mut queue = [None; 4];
        if self.dead || gamemode == 3 {
            self.last_pos = pos;
            self.was_ground = on_ground;
            return queue;
        }
        let survival = gamemode == 0 || gamemode == 2;
        let creative = gamemode == 1;

        // Movement exhaustion (`Player.travel`): sprinting costs 0.1 per metre; a jump costs
        // 0.05, or 0.2 when sprint-jumping (`Player.jumpFromGround`).
        if survival {
            let dx = pos[0] - self.last_pos[0];
            let dz = pos[2] - self.last_pos[2];
            if sprinting && on_ground {
                self.food
                    .add_exhaustion(0.1 * (dx * dx + dz * dz).sqrt() as f32);
            }
            if self.was_ground && pos[1] > self.last_pos[1] + 0.1 {
                self.food.add_exhaustion(if sprinting { 0.2 } else { 0.05 });
            }
        }
        // Fall distance accumulates while airborne and resets on landing or in a fluid
        // (`Entity.checkFallDamage`).
        if pos[1] < self.last_pos[1] {
            self.fall_distance += self.last_pos[1] - pos[1];
        }
        if eye_in_water || feet_in_lava {
            self.fall_distance = 0.0;
        }
        if on_ground {
            if self.fall_distance > SAFE_FALL_DISTANCE && survival {
                let damage = (self.fall_distance - SAFE_FALL_DISTANCE).ceil() as f32;
                queue_damage(&mut queue, DamageKind::Fall, damage);
            }
            self.fall_distance = 0.0;
        }
        self.last_pos = pos;
        self.was_ground = on_ground;

        // `Player.tick`: fire ticks burn for 1 damage every second.
        if self.fire_ticks > 0 {
            if self.fire_ticks % 20 == 0 {
                queue_damage(&mut queue, DamageKind::Fire, 1.0);
            }
            self.fire_ticks -= 1;
        }
        if fire_block {
            self.fire_ticks = self.fire_ticks.max(FIRE_BLOCK_TICKS);
        }
        if feet_in_lava {
            // `Entity.lavaHurt`: 4 damage and 15 s of fire.
            self.fire_ticks = self.fire_ticks.max(LAVA_FIRE_TICKS);
            queue_damage(&mut queue, DamageKind::Lava, 4.0);
        }

        // `Player.tick` water breathing: `decreaseAirSupply` one per tick, at -20 reset and
        // take 2 drowning damage; otherwise refill by 4 per tick.
        if eye_in_water {
            self.air -= 1;
            if self.air <= -20 {
                self.air = 0;
                queue_damage(&mut queue, DamageKind::Drown, 2.0);
            }
        } else if self.air < MAX_AIR {
            self.air = (self.air + 4).min(MAX_AIR);
        }

        // `Player.isBelowWorld`: 4 damage per tick below min build height - 64.
        if pos[1] < self.void_y
            && let Some(amount) = self.void_damage
        {
            queue_damage(&mut queue, DamageKind::Void, amount);
        }

        if creative {
            // Creative ignores everything except the bypassing sources (void, /kill).
            for slot in queue.iter_mut() {
                if slot.is_some_and(|(kind, _)| kind.ignores_creative()) {
                    *slot = None;
                }
            }
        }
        queue
    }

    /// `FoodData.tick` through the game rule and game mode; starvation is only applied in
    /// survival/adventure, and only above 1 health on Normal difficulty.
    pub fn food_tick(
        &mut self,
        natural_regeneration: bool,
        gamemode: i32,
        difficulty_hard: bool,
    ) -> FoodTick {
        if self.dead || (gamemode != 0 && gamemode != 2) {
            return FoodTick::None;
        }
        match self.food.tick(self.health, natural_regeneration) {
            FoodTick::Starve => {
                if self.health > 10.0 || difficulty_hard || self.health > 1.0 {
                    FoodTick::Starve
                } else {
                    FoodTick::None
                }
            }
            other => other,
        }
    }
}

/// Whether a block state is `minecraft:fire` or `minecraft:soul_fire`; standing in one refreshes
/// the 8 s fire timer (`Entity.baseTick` -> `setRemainingFireTicks(160)`).
pub fn is_fire_block(state: u16) -> bool {
    ferrite_data::blocks::state(state)
        .is_some_and(|(name, _)| matches!(name, "minecraft:fire" | "minecraft:soul_fire"))
}

/// `GameRules.NATURAL_HEALTH_REGENERATION` and `GameRules.KEEP_INVENTORY`.
pub struct SurvivalRules {
    pub natural_regeneration: bool,
    pub keep_inventory: bool,
    pub show_death_messages: bool,
    pub immediate_respawn: bool,
    /// The server has no difficulty command wiring yet; Normal is vanilla's default.
    pub hard_difficulty: bool,
}

impl SurvivalRules {
    pub fn from(rules: &GameRules) -> Self {
        Self {
            natural_regeneration: rules
                .get_bool("natural_health_regeneration")
                .unwrap_or(true),
            keep_inventory: rules.get_bool("keep_inventory").unwrap_or(false),
            show_death_messages: rules.get_bool("show_death_messages").unwrap_or(true),
            immediate_respawn: rules.get_bool("immediate_respawn").unwrap_or(false),
            hard_difficulty: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn damage(queue: EnvironmentDamage) -> Vec<(DamageKind, f32)> {
        queue.into_iter().flatten().collect()
    }

    #[test]
    fn absorption_absorbs_before_health_and_invulnerability_blocks_repeats() {
        let mut s = Survival {
            absorption: 5.0,
            ..Default::default()
        };
        assert!(s.damage(DamageKind::Fall, 3.0));
        assert_eq!(s.absorption, 2.0);
        assert_eq!(s.health, 20.0);
        // The accepted hit starts invulnerability, even though absorption ate it all.
        assert_eq!(s.invulnerable, INVULNERABLE_TICKS);
        assert!(!s.damage(DamageKind::Fall, 3.0));
        // Bypassing sources ignore the window.
        assert!(s.damage(DamageKind::Void, 1.0));
        assert_eq!(s.absorption, 1.0);
    }

    #[test]
    fn health_reaches_zero_once_and_heals_clamp_at_max() {
        let mut s = Survival::new();
        s.health = 2.0;
        assert!(s.damage(DamageKind::Lava, 4.0));
        assert_eq!(s.health, 0.0);
        assert!(s.dead);
        assert_eq!(s.death_cause, Some(DamageKind::Lava));
        assert!(!s.damage(DamageKind::Fire, 1.0));
        assert!(!s.heal(5.0));
        let mut s = Survival {
            health: 19.0,
            ..Default::default()
        };
        assert!(s.heal(5.0));
        assert_eq!(s.health, 20.0);
        assert!(!s.heal(1.0));
    }

    #[test]
    fn food_eat_matches_vanilla_saturation_by_modifier() {
        let mut food = FoodData {
            food: 10,
            saturation: 0.0,
            ..Default::default()
        };
        food.eat(4, 2.4); // apple: 4 * 2.4 * 2 = 19.2, capped at food 14
        assert_eq!(food.food, 14);
        assert!((food.saturation - 14.0).abs() < 1e-6);
        let mut food = FoodData {
            food: 20,
            saturation: 0.0,
            ..Default::default()
        };
        food.eat(8, 12.8); // cooked beef bottoms out at the 20-point cap
        assert_eq!(food.food, 20);
        assert!((food.saturation - 20.0).abs() < 1e-6);
    }

    #[test]
    fn exhaustion_eats_saturation_then_food() {
        let mut food = FoodData {
            saturation: 6.0,
            ..Default::default()
        };
        food.add_exhaustion(4.5);
        assert!(matches!(food.tick(19.0, false), FoodTick::None));
        assert!((food.exhaustion - 0.5).abs() < 1e-6);
        assert!((food.saturation - 5.0).abs() < 1e-6);
        food.saturation = 0.0;
        food.add_exhaustion(4.1);
        food.tick(19.0, false);
        assert_eq!(food.food, 19);
    }

    #[test]
    fn natural_regeneration_uses_the_ten_and_eighty_tick_timers() {
        let mut food = FoodData {
            saturation: 3.0,
            ..Default::default()
        };
        for _ in 0..9 {
            assert!(matches!(food.tick(19.0, true), FoodTick::None));
        }
        assert!(
            matches!(food.tick(19.0, true), FoodTick::Heal(amount) if (amount - 0.5).abs() < 1e-6)
        );
        // Healing only adds exhaustion; it does not drain saturation until the next tick.
        assert!((food.saturation - 3.0).abs() < 1e-6);
        // With low saturation, the slower food>=18 branch heals 1 every 80 ticks.
        let mut food = FoodData {
            saturation: 0.0,
            ..Default::default()
        };
        for _ in 0..79 {
            assert!(matches!(food.tick(19.0, true), FoodTick::None));
        }
        assert_eq!(food.tick(19.0, true), FoodTick::Heal(1.0));
        // The gamerule disables both regeneration branches.
        let mut food = FoodData {
            saturation: 6.0,
            ..Default::default()
        };
        for _ in 0..100 {
            assert!(matches!(food.tick(19.0, false), FoodTick::None));
        }
    }

    #[test]
    fn starvation_fires_after_eighty_ticks_and_respects_normal_health_floor() {
        let mut s = Survival {
            health: 1.0,
            food: FoodData {
                food: 0,
                saturation: 0.0,
                ..Default::default()
            },
            ..Default::default()
        };
        for _ in 0..80 {
            assert_eq!(s.food_tick(true, 0, false), FoodTick::None);
        }
        assert_eq!(s.food_tick(true, 0, false), FoodTick::None);
        s.health = 5.0;
        s.food.tick_timer = STARVE_INTERVAL;
        assert_eq!(s.food_tick(true, 0, false), FoodTick::Starve);
        assert_eq!(s.food_tick(true, 1, false), FoodTick::None);
    }

    #[test]
    fn fall_damage_is_ceil_of_fall_distance_minus_three() {
        let mut s = Survival {
            last_pos: [0.5, 70.0, 0.5],
            was_ground: false,
            ..Default::default()
        };
        // 3.0 blocks of fall: no damage, but the distance resets on landing.
        let queue = s.environment_tick([0.5, 67.0, 0.5], true, false, false, false, 0, false);
        assert!(damage(queue).is_empty());
        assert_eq!(s.fall_distance, 0.0);
        // 6.2 blocks: ceil(3.2) = 4 damage.
        s.last_pos = [0.5, 70.0, 0.5];
        s.was_ground = false;
        let queue = s.environment_tick([0.5, 63.8, 0.5], true, false, false, false, 0, false);
        assert_eq!(damage(queue), [(DamageKind::Fall, 4.0)]);
        // Falling into water takes no fall damage.
        s.last_pos = [0.5, 70.0, 0.5];
        s.was_ground = false;
        let queue = s.environment_tick([0.5, 60.0, 0.5], true, true, false, false, 0, false);
        assert!(damage(queue).is_empty());
    }

    #[test]
    fn water_fire_lava_and_void_follow_vanilla_timings() {
        // Drowning at -20 air, then the bar refills out of water.
        let mut s = Survival {
            air: -19,
            ..Default::default()
        };
        let queue = s.environment_tick([0.0; 3], true, true, false, false, 0, false);
        assert_eq!(damage(queue), [(DamageKind::Drown, 2.0)]);
        assert_eq!(s.air, 0);
        let queue = s.environment_tick([0.0; 3], true, false, false, false, 0, false);
        assert!(damage(queue).is_empty());
        assert_eq!(s.air, 4);

        // Fire ticks burn for 1 damage every 20 ticks.
        let mut s = Survival {
            fire_ticks: 20,
            ..Default::default()
        };
        let queue = s.environment_tick([0.0; 3], true, false, false, false, 0, false);
        assert_eq!(damage(queue), [(DamageKind::Fire, 1.0)]);
        assert_eq!(s.fire_ticks, 19);

        // Lava deals 4 and sets 15 s of fire.
        let mut s = Survival::new();
        let queue = s.environment_tick([0.0; 3], true, false, true, false, 0, false);
        assert_eq!(damage(queue), [(DamageKind::Lava, 4.0)]);
        assert_eq!(s.fire_ticks, LAVA_FIRE_TICKS);

        // Standing in fire refreshes 8 s.
        let mut s = Survival::new();
        let queue = s.environment_tick([0.0; 3], true, false, false, true, 0, false);
        assert!(damage(queue).is_empty());
        assert_eq!(s.fire_ticks, FIRE_BLOCK_TICKS);

        // Below the world hurts every tick.
        let mut s = Survival::new();
        let queue = s.environment_tick(
            [0.0, VOID_Y - 1.0, 0.0],
            false,
            false,
            false,
            false,
            0,
            false,
        );
        assert_eq!(damage(queue), [(DamageKind::Void, 4.0)]);
    }

    #[test]
    fn a_plugin_can_change_or_switch_off_the_void_damage() {
        let mut state = Survival::new();
        state.void_damage = Some(9.0);
        let queue = state.environment_tick(
            [0.0, VOID_Y - 1.0, 0.0],
            false,
            false,
            false,
            false,
            0,
            false,
        );
        assert_eq!(damage(queue), [(DamageKind::Void, 9.0)]);
        let mut state = Survival::new();
        state.void_damage = None;
        let queue = state.environment_tick(
            [0.0, VOID_Y - 1.0, 0.0],
            false,
            false,
            false,
            false,
            0,
            false,
        );
        assert!(damage(queue).is_empty());
    }

    #[test]
    fn the_void_starts_sixty_four_blocks_below_the_dimensions_floor() {
        // The end and the nether start at y 0, so falling to y -65 is already the void there.
        let mut state = Survival::new();
        state.void_y = -64.0;
        let queue = state.environment_tick([0.0, -65.0, 0.0], false, false, false, false, 0, false);
        assert!(
            queue
                .iter()
                .flatten()
                .any(|(kind, _)| *kind == DamageKind::Void)
        );
        let mut state = Survival::new();
        let queue = state.environment_tick([0.0, -65.0, 0.0], false, false, false, false, 0, false);
        assert!(
            !queue
                .iter()
                .flatten()
                .any(|(kind, _)| *kind == DamageKind::Void),
            "the overworld void is still at -128"
        );
    }

    #[test]
    fn creative_ignores_hazards_but_not_void_and_spectators_take_nothing() {
        let mut s = Survival::new();
        let queue = s.environment_tick([0.0, VOID_Y - 1.0, 0.0], true, true, true, true, 1, false);
        assert_eq!(damage(queue), [(DamageKind::Void, 4.0)]);
        let mut s = Survival::new();
        let queue = s.environment_tick([0.0, VOID_Y - 1.0, 0.0], true, true, true, true, 3, false);
        assert!(damage(queue).is_empty());
    }

    #[test]
    fn sprinting_and_jumping_add_exhaustion() {
        let mut s = Survival {
            last_pos: [0.0, 64.0, 0.0],
            was_ground: true,
            ..Default::default()
        };
        // One tick of sprinting at 0.25 blocks horizontally.
        s.environment_tick([0.25, 64.0, 0.0], true, false, false, false, 0, true);
        assert!((s.food.exhaustion - 0.025).abs() < 1e-6);
        // Jumping costs 0.05 (non-sprint), sprint-jumping 0.2.
        s.environment_tick([0.25, 65.0, 0.0], false, false, false, false, 0, false);
        assert!((s.food.exhaustion - 0.075).abs() < 1e-6);
        s.was_ground = true;
        s.last_pos = [0.25, 64.0, 0.0];
        s.environment_tick([0.5, 65.0, 0.0], false, false, false, false, 0, true);
        assert!((s.food.exhaustion - 0.275).abs() < 1e-6);
    }

    #[test]
    fn sync_tracks_health_food_and_saturation_bits() {
        let mut s = Survival {
            health: 19.0,
            ..Default::default()
        };
        assert!(s.needs_sync());
        s.mark_synced();
        assert!(!s.needs_sync());
        s.food.saturation = 4.5;
        assert!(s.needs_sync());
    }
}
