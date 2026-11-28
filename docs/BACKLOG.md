# Backlog — remaining work

Single list of open work, checked against source on **2026-11-19** at the vanilla-core pivot.
`README.md` holds the status table; this file holds the open work. Older coordinator and handoff
notes are historical; use this file and the current source instead of their "remaining" lists.
The open runtime-world sequence is captured below so it stays with the release branch.

## Contract for every task

Ferrite's Folia-style regionized concurrency is a correctness requirement, not a later optimization.

- Keep mutable world and entity state owned by its region thread. Never read or mutate another
  region's live state directly; use the region scheduler and the existing ownership-transfer paths
  (`Signal::EntityTransfer`, the per-player mailboxes, `RegionData::{merge, split}`).
- Cross-region interactions (entities, leads, mounts, projectiles, portals, block entities) need
  explicit handoff, cancellation, unload and reconnect behaviour, and tests for it.
- No global lock, shared mutable registry, or synchronous cross-region wait that can serialize
  ticking or deadlock. A global index needs a bounded, documented synchronization strategy and
  immutable or snapshot data where possible.
- Tests must cover same-region and cross-region cases, region split/merge, and persistence. Do not
  claim Folia compatibility from single-region tests.
- No Mojang code or assets. Real-client visual checks are excluded and must be reported as a
  limitation, not replaced with a claim.

## Landed since this file was last rewritten (do not re-implement)

- Redstone family including observers, hoppers, droppers/dispensers, command blocks, repeaters,
  comparators and torches — `game/{redstone,hopper,dispenser,command_block}.rs`.
- Pistons: resolver, entity pushes, two-tick overlay, region split/merge — `docs/pistons.md`.
- Nether and End dimensions, portals, the dragon fight, respawn stages, exit portal, gateways —
  `docs/end-dimension.md`.
- Containers: furnace/blast furnace/smoker, anvil, enchanting, grindstone, stonecutter, smithing,
  loom, brewing, beacon, chests/barrels, villager and trader menus —
  `game/{furnace,container,brewing,beacon}.rs`.
- Villagers and deterministic trades, wandering trader spawn/lifetime with the llama caravan,
  horse/llama riding and inventories — `game/{wandering_trader,ride,container}.rs`.
- Entity and block-entity persistence across restarts; the world is saved as Anvil under `world/`.
- All 94 vanilla command roots are registered, many only partly — `docs/COMMANDS-STATUS.md`.
- Signed chat, Velocity/Bungee forwarding, the per-connection writer task, the chunk packet cache.
- 2026-11-14: gateway landing radius 5; perched breath cloud 200 ticks at radius 5; sheep
  dyeing; `execute summon` hands its new entity to the chain; non-player slot predicates and
  `World#getFluidData`.
- 2026-11-19: sapling stage/tree growth through the worldgen feature host; turtle
  water/travel/nesting and hatching; beehive occupancy/pollination; carved-pumpkin golem
  construction; per-mob flags and sound variants.
- 2026-11-27: Wither T-pattern summoning, 220-tick invulnerable charge-up/explosion, purple BossBar
  (64-block range, sky darken), 3×4×3 `#wither_immune`-respecting block breaking on damage,
  Nether Star + 50 XP death drop, Wither skulls (Wither II) & half-health arrow armor; **83 official
  26.3 mobs** (`wirecheck ok=186 bad=0`) including `ZombieVillager` (vanilla 3600–6000 tick curing
  timer, iron bars/bed acceleration, profession-preserving `Villager` conversion + `record_cured`
  gossip), `Ghast` (charging metadata & explosive fireball), `Vex` (summoned by `Evoker`),
  `Silverfish`, `Endermite` (5% Ender Pearl spawn), aquatic/special mobs (`Squid`, `GlowSquid`,
  `Dolphin`, `Cod`, `Salmon`, `Pufferfish`, `TropicalFish`, `Tadpole`, `Bat`, `Ocelot`, `Panda`,
  `PolarBear`, `SkeletonHorse`, `ZombieHorse`, `Illusioner`, `Giant`); unique vanilla AI for Warden
  Sonic Boom, Breeze Wind Charge & arrow deflection, Creaking camera-FOV freeze, Shulker bullets,
  Guardian/Elder Guardian laser beam & Mining Fatigue III curse, Evoker Fangs & Vex summoning,
  Illusioner bow & Blindness spell, Ravager AoE roar, Hoglin/Zoglin upward fling, Phantom daylight
  burn, Pufferfish puff-up + Poison aura, PolarBear neutral retaliation, Armadillo roll-up, Sniffer
  ancient seed digging (`torchflower_seeds`/`pitcher_pod`), and Squid/GlowSquid ink squirt;
  dungeon `mob_spawner` runtime tick (`game/mob_spawner.rs`) & loot chest NBT; underground
  Mineshaft (cave spider spawner + `abandoned_mineshaft` chest), Nether Fortress Blaze spawner
  balcony (`nether_bridge` chest), End City Purpur Tower (`end_city_treasure` chest), desert pyramid
  treasure room, jungle temple, swamp hut, desert well & mushroom island features; expanded
  Paper/Folia Java APIs (`PermissionAttachment`, `AttributeInstance`, `FerriteBlock` 25+ methods,
  `FerritePlayer` EnderChest + 6 workstation menus, `FerriteServer` IP bans/MOTD/tick/spawnRadius,
  World PvP/Difficulty/Sound/Particle, `RegionScheduler`, `BossBar` + 5-plugin E2E suite); expanded
  C-ABI v2 native plugin hooks (`on_player_command`, `on_packet_inbound`/`outbound` with live
  encoded packet payloads, `on_mob_spawn`, `on_fire_spread`, `on_weather_change`, `on_chunk_load`),
  host callbacks (`set_health`, `set_food`, `set_world_time`, `give_item`, `has_permission`), and
  JVM bytecode `auto-convert-native` transpiler; `ferrite-convert` `snapshot`/`verify`/`recompress`;
  and engine hotpath optimizations (`RustGenerator` flat-buffer `initial_light_from_u16` &
  `grid == 15` skip, `rust_decorate` dead-work elimination & single-pass Nether scan,
  `PalettedContainer`/`chunk_nbt` zero-alloc packing & const-generic lane shift, `Chunk::set_block`
  heightmap bitmask skip, `LightEngine` zero-copy reset & identical-`LightProperties` skip,
  `noise.rs` inline `floor`, A* single-probe relaxation, `VarLong` 1–2 byte fast path, `TrackingInfo`
  `OnceLock` table, `.fworld` v3 LZ4 full save `2.98 ms`, Overworld `1T 352.6/s` / `12T 2,310.9/s`,
  Nether `12,726.1/s`, End `22,780.4/s`, 200-player fan-out `0.20 ms` / `192.4x`, `50,000x` raycast
  `1.24 ms`).
- 2026-11-27 (v2): **90/90 (100%) official 26.3 living mobs** (`CamelHusk`, `CopperGolem`,
  `HappyGhast`, `Nautilus`, `ZombieNautilus`, `Parched`, `SulfurCube` + 26.3 metadata serializers
  `32/37/38`, `wirecheck ok=200 bad=0`) and **40/40 (100%) status effects** (`Unluck`,
  `DolphinsGrace`, `BreathOfTheNautilus` + `Dolphin`/`Nautilus` aura AI and `SulfurCube` fuse
  explosion AI); vanilla 26.3 combat enchantments (`sharpness`, `smite`, `bane_of_arthropods` +
  Slowness IV, `fire_aspect`, `knockback`, `sweeping_edge`, mace smash + `density`, `breach`,
  `power`, `flame`, `punch`, `infinity`), vanilla armor defense/toughness + protection EPFs
  (`protection`, `fire_protection`, `blast_protection`, `projectile_protection`, `feather_falling`,
  `thorns` reflection), melee PvP, and gamerules (`fire_damage`, `drowning_damage`, `fall_damage`,
  `pvp`, `spawner_blocks_work`, `ender_pearls_vanish_on_death`); core block entities: per-player
  27-slot **Ender Chest** (`minecraft:ender_chest` + `playerdata` `ender_items` persistence),
  6-slot **Chiseled Bookshelf** (`game/bookshelf.rs`, block state `slot_0..5_occupied` + NBT +
  comparator), and **Conduit** (`game/conduit.rs`, 3×3×3 water shell + 5×5 orthogonal prismarine
  frame, 32–96 block `ConduitPower` aura, 42-block hostile mob attack); expanded Paper/Bukkit
  events (`PlayerTeleportEvent`, `FoodLevelChangeEvent`, `PlayerExpChangeEvent`,
  `EntityDamageEvent`, `PlayerDeathEvent`, `WeatherChangeEvent`, `CreatureSpawnEvent` +
  `FerriteInternalApiBridge.createDamageSourceBuilder` & 2-arg `FerriteWorld.spawnEntity`).
- 2026-11-27 (v3 — 100% 26.3 registry parity sweep): **43/43 (100%) enchantments** (added
  `efficiency` mining speedup, `respiration` underwater air drain, `frost_walker` surface water
  freezing to `frosted_ice`, `depth_strider`, `swift_sneak`, `binding_curse`, `impaling`,
  `quick_charge`, `multishot` 3-arrow fan, `piercing` arrow pass-through, `wind_burst` mace smash
  launch, `lunge` dash) + `powder_snow` freeze ticks & leather armor immunity + core gamerules
  (`freeze_damage`, `tnt_explodes`, `mob_drops`, `command_blocks_work`, `spawn_monsters`,
  `allow_entering_nether_using_portals`, `players_nether_portal_default_delay`/`creative_delay`,
  28/59 enforced); **161/161 (100%) entities** (22 boats/chest-boats/rafts, 7 minecarts including
  `tnt_minecart` activator detonation & `hopper_minecart` item pickup, real `lightning_bolt` entity
  with `Pig->ZombifiedPiglin`/`Villager->Witch`/`Creeper->charged`/`Mooshroom` conversions, and 10
  display/interaction/mannequin/decorative entities with `item_frame` insert/rotate); **49/49 (100%)
  block entities** (`game/sculk.rs` catalyst spread/shrieker Warden summon/calibrated sensor,
  `game/block_entities_extra.rs` archaeology brushing, lectern, decorated pot, creaking heart, bell
  raider glowing, plus banner/skull/copper_golem_statue/potent_sulfur/shelf/test_instance_block);
  special items & blocks (`game/special_items.rs`: Elytra firework boost, player Crossbow, Bone Meal,
  Sponge 65-block BFS/Nether dry, Respawn Anchor, Copper Bulb, Goat Horn); and **52/52 (100%)
  vanilla 26.3 worldgen structures** + natural Overworld thunderstorm lightning with `lightning_rod`
  redirection (`2,547 passed / 0 failed`, `wirecheck ok=200 bad=0`).
- 2026-11-27 (v4 — item/block runtime gaps): composter fill/harvest from the item's
  `minecraft:compostable.layers` component with vanilla 30/50/65/85/100 percentages and
  bone-meal extraction (`game/composter.rs`); frogspawn placement on a water surface and its
  `3600..12000`-tick scheduled hatch into 2–5 tadpoles (`game/block_ticks.rs`); furnace minecart
  3600-tick fuel (32000 cap) + continuous vanilla push, command-block minecart 4-tick powered
  activator execution, and spawner minecart `BaseSpawner` NBT ticking, with vanilla NBT
  persistence for all three. **Still open:** bundle insert/extract/select — the item model has no
  typed `minecraft:bundle_contents` component (its network form is raw bytes and its save form is
  `Component::Opaque`); the client's `BundleItemSelected` index is recorded in `SurvivalState`,
  but contents cannot be read or rewritten until a typed bundle-contents model exists. Spyglass
  needs no server state: vanilla only starts a 1200-tick use action, plays the sound and awards
  the stat; scoping and FOV are client-only.
- 2026-11-27 (v5 — gamerules): `entity_drops` now also gates mob loot, TNT and mob explosions use
  their own `tnt_explosion_drop_decay` / `mob_explosion_drop_decay` rules, and
  `reduced_debug_info` / `limited_crafting` are propagated in the login packet, `locator_bar`
  clears tracked waypoints, while `send_command_feedback` and `log_admin_commands` gate their
  command paths. Still unenforced at v5: patrol/phantom spawning, snow accumulation height,
  command fork/sequence limits, projectile block breaking and spectator chunk generation.
- 2026-11-27 (v6 — remaining priority rules and vehicles): `command_block_output` broadcasts
  command-block feedback to permission-level-2 players, `show_advancement_messages` gates the
  chat announcement (rewards still run), `respawn_radius` offsets respawns without a bed/anchor,
  and `max_minecart_speed` caps every minecart after powered-rail/furnace push. Bed and respawn
  anchor points are now actually used on death. `PaddleBoat` stores the client's left/right input
  on the ridden boat and drives the paddle metadata. Java's `FerriteServicePluginLoader` delegates
  to the runtime-owned `FerritePluginLoader` instead of failing every method. Still open at v6:
  `ContainerSlotStateChanged` (the crafter screen/menu was not modeled yet; closed by v7), the
  remaining gamerule list above, and the transitive `glib 0.18.5` advisory (Tauri 2.12.5 pins
  gtk 0.18).
- 2026-11-27 (v7 — four-worker parity sweep): **safety and crafting rules**: `/fill` and
  `/clone` honor `max_block_modifications` (default 32768); the dispatcher enforces
  `max_command_forks` and `max_command_sequence_length` across nested function/`execute` forks;
  and `limited_crafting` rejects recipes the player has not unlocked. **Crafter and bundles**:
  vanilla `minecraft:crafter_3x3` menu with nine one-item inputs, a result slot, per-slot
  disabled bits persisted in block-entity NBT, and a live `ContainerSlotStateChanged` handler;
  typed `minecraft:bundle_contents` with vanilla 64-weight insert/extract/select and
  `BundleItemSelected` handling. **Spawning/AI**: vanilla pillager patrols (`spawn_patrols`:
  shared per-dimension 12000–13200 tick window, 20% chance, five-day world-age gate,
  Overworld/Peaceful/block-light checks, 2–5 pillagers with a banner captain 24–48 blocks from
  the chosen player) and phantom insomnia (`spawn_phantoms`: per-player 72000-tick
  `TIME_SINCE_REST`, reset on sleeping and respawning, night/thunderstorm, sea-level and
  clear-sky checks, 1–2 minute attempts with the vanilla chance formula, 1–4 phantoms 20–34
  blocks above scaled by difficulty); arrows and tridents carry `universal_anger` to an angered
  neutral mob, retargeting it to the nearest player within 16 blocks; `player_movement_check`/
  `elytra_movement_check` reject moved-too-quickly packets using Spigot's
  `moved-too-quickly-multiplier` and `moved-wrongly-threshold` (teleport, flying, vehicle and
  creative/spectator moves exempt). **World/API**: random-tick precipitation reads each biome's
  `has_precipitation`/`temperature`/`temperature_modifier`, including the height adjustment,
  freezes water into ice and melts snow in temperate rain; `max_snow_accumulation_height` caps
  the layer; projectiles honor adventure mode and spawn protection before breaking chorus
  flowers, decorated pots or fast tridents; portal activation emits level event 1038 through
  `global_sound_events`; and Java PDC values support nested lists and arrays with recursive
  round trips. CI's `world_regression` chunk wait now matches the session read timeout so slow
  runners do not fail on wall-clock alone. Still open at v7 (closed by v8): patrol captain-follow
  AI and local-difficulty group sizing; spawn protection outside projectile block breaking;
  Paper lifecycle event registration. Real-client visual verification remains manual.
- 2026-11-28 (v8 — closing the v7 gaps): pillager patrol members follow the banner captain
  (`LongDistancePatrolGoal`: 8-block follow distance, 0.7 speed) and store the captain UUID on
  the mob state; patrol group size and phantom groups read a vanilla `DifficultyInstance` built
  from world age, per-chunk inhabited time and the Hard moon phase, so long-settled chunks form
  patrols of up to eight; spawn protection now blocks breaking, placing and using blocks (not
  only projectile breaks) for survival/adventure non-operators in the Overworld; and Paper
  lifecycle event registration is implemented with the `COMMANDS` registrar (`BasicCommand` and
  Brigadier literals), priority/monitor ordering, the enable-window check, handler isolation,
  INITIAL firing after the last plugin enables, RELOAD refiring and per-plugin cleanup.

## 1. Real 26.3 client validation (visual client check still manual)

Latest `python3 tools/wirecheck/run.py` against a live headless release `ferrite-server`
(`--storage fworld --lighting-pool`) passed `SUMMARY ok=1923 bad=0` with the real vanilla 26.3
codecs (`PacketCheck.java` updated to register `WORLD_CLOCK` alongside 26.3 mob variant / sound_variant
registries; `change_difficulty`, `player_abilities`, and `set_time` world clock updates all verified on wire).
Interactive visual checks with a real Minecraft client still need manual run: dimension travel,
arrival platform, void death, End boss bar/dragon/gateways, piston animation, animal interaction,
and vault and trading screens.

## 2. Entity and mob parity

- Entity replacement when a player leaves and returns to chunks is pinned by regression tests
  (`two_viewers_one_leaving_first_then_reentry_keeps_one_entity` in `game/mod.rs`,
  `a_chunk_stays_held_until_the_last_viewer_releases_it` in `game/region.rs`). A real-client
  reproduction is still outstanding.
- `MobKind` now covers 41 entity types (including `Slime` and `MagmaCube` with `ID_SIZE` metadata,
  `Size` NBT persistence, and split-on-death). The 2026-11-19 breadth slice adds Drowned, Husk,
  Stray, Bogged, Wither Skeleton, Blaze, Cave Spider, Witch, Piglin, Zombified Piglin, Iron Golem,
  Snow Golem, Goat, Fox, Rabbit, Frog, Turtle, Bee and Mooshroom, with shared family AI,
  equipment, biome spawn-table inclusion and zombie/husk drowning conversion. Passive/neutral
  special AI now covers Snow Golem snowball attacks (3 damage to Blazes), warm-biome/Nether
  melting (`melts_in_biome`), and region-owned snow trails; Iron Golem heavy melee/fling (`+0.4` Y,
  status `4`, cross-region preserved), `player_created` non-player-targeting gate, and non-creeper
  hostile defense; Goat ram/charge and long-jump; Fox prey pounce and player flee; Rabbit hop
  movement and player flee; Frog long-jump, `ShootTongue` live prey catching against tiny
  `Slime`/`MagmaCube`, and variant froglight drops; Bee sting with Poison (`10 s`/`18 s`,
  cross-region preserved) and post-sting death countdown; Mooshroom bowl-to-stew interaction and
  shearing into a cow + 5 mushrooms; Iron Golem iron-ingot repair; and cow/goat/mooshroom bucket
  milking (with `filled_bucket` and golem `summoned_entity` advancement triggers). Hostile ranged
  AI now covers Blaze fireballs/hover/melee, Witch splash potions and self-drinking, Piglin
  crossbows/gold-armor neutrality/anger, and Stray/Bogged tipped arrows. Bee hives/pollination,
  turtle home-beach/water/travel/egg-laying/hatching, carved-pumpkin golem construction, and the
  modelled per-mob flags/sound variants are implemented server-side (`docs/mob-ai-parity.md`).
  Piglin bartering, its eight-slot inventory, `#guarded_by_piglins` anger, and Overworld zombification
  (`300` ticks outside `piglin_safe` dimensions, `ZombifiedPiglin` conversion with 200-tick Nausea I
  and `IsImmuneToZombification`/`TimeInOverworld` persistence) are also implemented.
  Witch raider healing and Piglin Hoglin hunting/celebration are implemented. Raids are now
  modelled: Bad Omen from a raid captain starts a three-plus-wave raid at a nearby village,
  waves spawn pillagers/vindicators/evokers/ravagers/witches, clearing the last wave makes the
  survivors celebrate (`Raider.IS_CELEBRATING`, including Witch `CELEBRATING`) and grants
  Hero of the Village. Raid Omen starts the same raid and is consumed, and live raids are saved
  per dimension to `data/minecraft/raids.dat` and reloaded into the owning region. Exact
  goal-by-goal residuals remain.
- Vanilla mob AI parity is incomplete. Cross-region player snapshots, remote attack routing and
  the knockback of a mob whose melee hit was blocked by a shield (`Signal::MobKnockback`) are in.
  Hostile goal selection keeps a valid target, endermen stay neutral until provoked, creepers
  hold their fuse out to seven blocks and skeletons strafe. Block-grid A* now handles bounded
  128-waypoint / up to 4096-node weighted searches (`path::node_budget`), `Villager` / `Piglin`
  region-owned door opening & closing (`InteractWithDoor`), deep-water surface recovery for
  floaters, `WallClimberNavigation` vertical/overhang/descent paths for spiders, and skeleton
  `avoid_sun` daylight detours; full goal-by-goal vanilla parity is still open (`docs/mob-ai-parity.md`).
- Dragon: the strafe phase, the gateway creation beam, the 2400-tick automatic beam and the
  perched subphases are implemented. The perched timings were checked against the 26.3 jar
  (scan 20×10, 25/100 ticks, roar 40, one cloud at tick 10 per 200-tick flame, four flames then
  takeoff). The gateway landing radius now matches vanilla's 5. The charge uses the 3.0
  fly-speed slope clamp, the 10-tick recovery, and `HORIZONTAL_COLLISION` / `VERTICAL_COLLISION`
  exits (`DragonChargePlayerPhase.doServerTick`, evaluated before `Flags::NO_PHYSICS` clears
  collision flags on movement like `Entity.move`). `EnderDragon` spawns with `Flags::NO_PHYSICS`,
  and `entity_save.rs` persists `DragonChargeTarget`, `DragonChargeRecovery`, and `DragonYawRate`
  (plus strafe `Target` UUID) while still aborting targetless legacy/vanilla charge saves to
  `Holding`. Perched breath aims from vanilla's sitting `head` offset (6.5 forward, -1.0
  y-offset, 0.5 mid-head) plus 2.5 forward and spawns a client-visible `area_effect_cloud`
  (`dragon_breath` particle, `INSTANT_DAMAGE`, 0.25 duration scale, constant radius 5, 20-tick
  wait, 200-tick lifetime) that affects players and mobs (dragon immune) across region
  split/merge. The head, neck and three tail `part_boxes` now use the same 26.3 `aiStep`
  `tickPart` arguments as the body/wings: a 17-tick `DragonFlightHistory` supplies the five-tick
  head offset, the 5/10-sample flight pitch, and the 12/14/16-sample tail yaw and height.
  Flight yaw now uses vanilla's decaying yaw-rate steering and the non-charge phases accelerate
  toward their target instead of snapping, so phase changes carry momentum. Remaining residual:
  real-client visual/animation/sound verification (`docs/end-dimension.md`, `docs/mob-ai-parity.md`).
- Projectiles: arrow, trident, snowball, egg, ender pearl, fireballs, llama spit, experience
  bottle, wind charge, splash and lingering potion, and the fishing bobber are in
  (`ferrite-entity/src/{arrow,bobber,potion}.rs`). A lingering cloud inserts a visible
  `area_effect_cloud` entity and applies its effects to players **and** mobs; the cloud follows
  region split/merge. Bobber casting, biting, hooking and reeling have region tests.
- Shoulder parrot is a real state: the parrot entity is removed when picked up, its colour is
  synched through player metadata, it survives reconnects in a sidecar save, and releasing it
  recreates the parrot entity. Client rendering is unverified.
- Metadata, equipment, and drop-chance breadth per entity type. Default attributes, vehicle
  passengers, and all 8 `EquipmentSlot` positions (`MainHand`..`Head`, `Body` on
  Horse/Llama/TraderLlama/Wolf, `Saddle` on Pig/Horse/Donkey/Mule) pair on tracking start and
  persist in NBT (`equipment` + `drop_chances`). Equipment death drops now apply vanilla 26.3
  Looting (`+0.01`/level from `looting.json`) and Curse of Vanishing suppression
  (`drops::roll_equipment_drops`). `Metadata::define_mob` defines the verified 26.3 indices,
  defaults, and holder-registry serializers (22–31) across all 39 `MobKind`s, with state sync
  and NBT persistence for sheared Bogged/SnowGolem (`Pumpkin`), baby Piglin, Bee nectar/sting,
  Turtle egg state, Goat screaming/horns, Fox flags/trusted UUIDs, IronGolem player-created,
  Blaze on-fire, the Frog/Fox/Rabbit/Mooshroom/Cow/Pig/Chicken variants, and the 22–31 sound
  variants. Trial Omen now converts nearby trial spawners to ominous and drives the three-wave,
  reward-ejection and cooldown cycle (`game/trial_spawner.rs`); the remaining metadata gap is
  wider non-`MobKind` coverage, plus real-client visual verification (`docs/mob-ai-parity.md`).
  Boats and minecarts summon, float/fall, and can be ridden through `/ride`; boats get
  depth-scaled buoyancy and bubble-time metadata, and minecarts centre on straight and curved
  rails, follow ascending rails and stop against blocks. Client visuals remain unverified.

## 3. Plugin & API scope (Folia / Paper / Spigot / NMS)

Historically, the earlier Java plugin host and JVM worldgen host (`vanilla_gen.rs` / `ferrite-jvm`)
were removed on 2026-11-18 in favor of pure-Rust parallel worldgen (`crates/ferrite-worldgen`,
`game/rust_gen.rs`). Any future plugin loader, Paper/Folia/Spigot API facade, or NMS bridge should
run as an opt-in module/feature (`--features jvm-plugins` or separate runtime layer) so the pure-Rust
core keeps its low idle memory and Folia-style region ownership guarantees.

## 4. World interaction and gameplay

- Waterlogging, fluid/redstone interaction and fluid-wash drops are implemented and covered by
  the `world-interaction-audit.md` tests. `Region::wake_loaded_chunks` implements the border pass
  and `Region::tick` runs it; the join benchmark is in the audit. Vines now follow their 26.3
  random tick, including the `spread_vines` rule and face-aware slab/stairs support, bamboo
  `leaves` updates are in, and sapling tree growth is wired to the 26.3 worldgen feature host
  (`ChunkGenerator::place_tree`, `Signal::GrowSapling` -> `ApplyTree` -> `TreePlaceChunk`) under
  region ownership with split/merge state, replaceability/space validation, and block/neighbor/fluid
  updates (`region::sapling_growth_tests::*`). Remaining audit blockers are real-client checks
  (and worlds with no worldgen host attached keep stage-1 saplings rather than hand-built trees).
- Neighbour-update ordering and placement rules match the audited six-face order; sugar cane, cactus,
  bamboo, kelp, melon/pumpkin stems, pitcher crop and sapling stage/tree growth now have random-tick
  handlers. Gravity-block chains were audited earlier.
- Enchanting edge cases. Anvil enchantment combining and shulker boxes are in. The cartography table is implemented (`game/map_menu.rs` over `game/map.rs` `MapStore`; zoom out, lock, clone, restart persistence are unit-tested).
- Maps: styled captions are sent as component NBT. Hidden cursors are omitted because the 26.3
  decoration record has no visibility bit (`FerriteMapView`, `PlayerExtrasTest`).
- `Structure#getEntities` and non-empty `EntityTransformer` lists are applied at the transformed
  position (`CapturedStructure.placeEntities`, `StructureEntitiesTest`).

## 5. Protocol and networking

- Protocol negotiation and bounded version boundary (`ferrite-protocol` `ProtocolVersion`,
  `SUPPORTED_PROTOCOLS`, `NegotiationOutcome`, `negotiate_protocol`, wired into `ferrite-net`
  `handle_connection`): accepts `26.3` (`777`) and returns explicit `OutdatedClient` /
  `OutdatedServer` disconnect reasons on mismatch. Older-protocol packet translation (started in
  `wip/day2-2026-10-28`) remains deferred because only `vanilla/26.3` data exists in this
  workspace.
- Outbound compression and CFB8 encryption offload is implemented in `ferrite-net` (`write_loop` /
  `flush_bodies`, batches `>= 4096` raw bytes via `spawn_blocking`, smaller batches inline; reader
  remains inline). Measurements and caveats are in `docs/NET-WRITER-OFFLOAD.md` (bench:
  `ferrite-net/benches/writer_stage.rs`).
- Chat and command rate limits are in (`ferrite-net/src/rate_limit.rs`, `ServerConfig::rate_limits`,
  vanilla spam-threshold defaults), configured by the `[rate-limits]` table of `ferrite.toml`.
  The 30 s read timeout is in (`ferrite-net/src/lib.rs`, `DEFAULT_READ_TIMEOUT`).
- Connection/storage hardening: `ServerConfig::max_connections` (`--max-connections`, default 4096),
  per-IP TCP connect throttling (`IpConnectionLimiter` in `rate_limit.rs`), `max_players` login
  capacity enforcement (`GameMessage::Joined` in `game/mod.rs`), `.fworld` decompression-bomb,
  `xxh32` checksum, and forged chunk-count bounds (`fworld::MAX_FWORLD_UNCOMPRESSED_BYTES`), and Tauri WebView CSP
  (`tauri.conf.json`) are implemented and unit-tested.
- Fine-grained traffic compression & batching controls (`[network]` table in `ferrite.toml`,
  `--compression-threshold -1..`, `--compression-level`, `--skip-incompressible`,
  `--writer-offload-bytes`, `--batch-linger-ms`) and matching Windows Tauri UI controls are wired.
- Pure-Rust worldgen decoration & vanilla Jigsaw assembly (`game/rust_decorate.rs`,
  `game/rust_jigsaw.rs`) add deterministic 3D cave carving, vanilla-band Overworld/Nether ores,
  biome-aware trees/flora, outer End chorus plants, and clean-room loading/BFS Jigsaw assembly
  over local `vanilla/26.3/generated/data/minecraft/{structure,worldgen/template_pool}` `.nbt`/JSON
  templates for `/place structure`, `/place jigsaw`, and generated structure chunks.
- `ferrite-entity/src/dragon.rs`: `fly_turn_step` drives vanilla smooth yaw-rate flight steering
  and non-charge phases accelerate toward their target instead of snapping; `scan_turn` and
  `clamp_slope` cover the perched and charge cases.

## 6. Performance and operations

- Player chunk stream and `/forceload` holder changes are batched into one scheduler topology
  lock/wake per packet or command batch. The 2026-11-19 40-cycle `/forceload` run measured 5,078
  vs 5,112 changes/s (0.99x, i.e. no throughput gain) but 50.9 vs 426.8 ms total holder-signal
  time (8.39x lower). The player chunk-stream harness is experimental and opt-in; the 800-bot
  rerun is still open. See `docs/perf/2026-11-19/holder-batching.md`.
- `/tick rate 10000` coordinator event-branch starvation is fixed (`events.recv()` now precedes
  `clock.tick()` in `game/mod.rs`, keeping `signals.recv()` first; pinned by
  `unpaced_tick_rate_does_not_starve_console_commands_or_shutdown`). The unpaced holder suites in
  `holders_e2e` remain opt-in (`HOLDERS_UNPACED=1`).
- `Signal::Update` is sent when player state changes. Do not skip coordinator updates based on the
  online snapshot: command teleports can make that copy stale. Profile before changing ownership.
- Re-run the 800-bot measurement after gameplay merges and record MSPT/RSS:
  - 2026-11-21 (`BOTS_STAGGER_MS=5 BOTS_FORCELOAD=20 BOTS_VIEW=2 target/release/examples/bots 800`, 12 CPUs):
    - `superflat`: `800/800` joined/walked (`34.9 s`), `avg_mspt=60.783`, `p50=59.104`, `p99_mspt=120.154`, `rss_mb=1334.4`, `cache_hit_rate=99.9%`, `43.14 MiB/s`.
    - `fixture` (`lighting-pool=false`): `800/800` joined/walked (`34.8 s`), `avg_mspt=11.007`, `p50=8.525`, `p99_mspt=34.818`, `max=124.785`, `rss_mb=2266.7`, `cache_hit_rate=99.9%`, `46.14 MiB/s`.
    - `fixture` (`BOTS_LIGHTING_POOL=1`): `800/800` joined/walked (`34.8 s`), `avg_mspt=10.417`, `p50=8.400`, `p99_mspt=33.639`, `max=94.272`, `rss_mb=1620.4`, `cache_hit_rate=99.9%`, `47.30 MiB/s`.
- The ScalableLux-style lighting worker pool is implemented in `ferrite-light`
  (`514dc4fa`, branch `yxshadxw/lighting-pool`): per-chunk `parking_lot` lock tokens in one
  total `(chunk_x, chunk_z)` order, worker-local engines, panic recovery, and centre-only
  publication. Evidence: 201,326,592 fixture block/kind comparisons at 1/2/4/8 workers with
  0 mismatches, 5 adversarial concurrency tests, and a 3-worker benchmark of 2.65–2.94x
  (88.5–98.0% efficiency) on a shared 6-core/12-thread host. See
  `docs/perf/2026-11-21/lighting-{pool,fixture,review,bench}.md`. Wired into
  `ferrite-server` (`game/lighting.rs`, `AnvilChunkSource::relight_batch`) with batch-wide
  atomic packet publication behind the default-off `lighting-pool` setting (`--lighting-pool`).
  Still open: real-world multi-chunk generation relight comparison (`panic = "abort"` in release leaves panic recovery test-only).
- `AnvilChunkSource::flush_all` now writes up to eight region files concurrently (`26f7f9b7`) and uses a dedicated multi-core batch NBT + compression pass for `.fworld`.
  2026-11-22 `flush_bench` (64 dirty Rust-worldgen chunks, release):
  - Anvil `flush_all` (1 region file / serial Zlib): `41.18 ms`; (4 region files / parallel Zlib): `15.74 ms` (`2.62x`); (4 region files / parallel LZ4): `5.37 ms` (`7.67x`).
  - `.fworld` (`FWRD` v3, streaming cold load + per-chunk parallel + `FworldColdStore::insert_raw_batch`) LZ4 atomic save: `3.85 ms` (`252,700` bytes, `10.71x` vs serial Anvil); incremental save (4/64 dirty): `0.84 ms`; cold load: `0.08 ms`.
  - Overworld worldgen scaling (48 chunks, release): 1/2/4/8/12 threads = `76.6 / 150.5 / 290.6 / 469.1 / 523.0 chunks/s` (`6.83x`). 12-thread dimension throughput: Overworld `487.3/s`, Nether `1,776.4/s`, The End `4,839.1/s`.
- 2026-11-22 Engine optimization + 26.3 gameplay/protocol/command completeness pass:
  - **Optimizations**: Lockless coordinator activity batching (`active_players` drained once per tick), 16-shard `settings` consolidation, `FxMap`/`FxSet` in `rate_limit`, `ferrite-region`, `ferrite-entity/{index,tracker}`, and `redstone`, 2x2 direct-mapped `ChunkReadCache` front-cache, reusable `QueuedBody`/batch buffers in `ferrite-net`, `chunks_tick` `retain` compaction, `chunk_gen` TLS corner pooling, `CubicSpline` small-slice linear search, `chunk_nbt` `BTreeMap` reuse, and boxed large `ServerboundPlay` variants.
  - **Verification & release microbenchmarks (`flush_bench`, `examples/bots 200`, `wirecheck`)**:
    - Full `bash tools/check.sh` (`cargo fmt`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, all unit/integration/doctests) passed.
    - `tools/wirecheck`: live release capture `ok=543 bad=0`, 62-mob `wiredump` `ok=144 bad=0`.
    - `flush_bench` (release, 64 chunks): Overworld 1/2/4/8/12 threads = `102.7 / 208.0 / 362.6 / 485.4 / 562.7 chunks/s`; 12-thread Nether `2,274.8/s`, End `8,390.9/s`; `.fworld` v3 LZ4 full save `4.11 ms` (`6.73x` vs serial Anvil `27.68 ms`), incremental `0.90 ms`, cold load `0.10 ms`; 200-player `SharedFrame` fan-out `22.50 ms -> 0.12 ms` (`191.0x`); `raycast_shapes` `20.5 ns/ray`.
    - `examples/bots 200` (`view=10`, release): `superflat` join `p50=44.324 ms` (`p99=51.846 ms`), `rss_mb=266.2`, coordinator events mean `0.0026 ms`; `fixture` `avg_mspt=2.004 ms` (`p50=1.322 ms`), join `p50=44.022 ms`, `rss_mb=413.9`.
- 2026-11-22 coordinator/net/worldgen/light pass & 4-server 26.3 comparison (`flush_bench` + `bench_compare.py`, release):
  - Coordinator dirty/revision gating (`locator`, `advancement_worker`, `recipe_worker`), cipherless `SharedFrame` inline net batching with reusable scratch buffer, non-HoL `chunks_tick` (`PREFETCH_CHUNKS=192`, `PENDING_LIMIT=256`), TLS flat `[u16]` section generation + `PalettedContainer::from_states_u16` (`global_bits=16` in `chunk_gen`) + all-solid/all-water cell skips, and `LightEngine` empty/uniform-palette section skips + sky top skip (`SnapshotBlockView`/`SingleChunkView`).
  - Fresh `seed=0` 400-bot MSPT dropped `14.03 -> 5.13 ms` (`.fworld`) / `14.31 -> 5.23 ms` (`anvil`), 200-bot MSPT dropped `4.68 -> 1.54 ms` (`.fworld`, `68.7%` CPU, `44.7 ms` join p50), and `fixture-world` `view=10` chunk streaming rose `690.8 -> 968.3 chunks/s` (`1.67 ms` 200-bot MSPT vs Folia `26.84 ms`, Paper `105.93 ms`, Purpur `109.73 ms`, Leaf `113.87 ms`).
  - `flush_bench` (release, 64 chunks): Overworld 1/2/4/8/12 threads = `79.0 / 153.8 / 295.5 / 474.9 / 487.3 chunks/s`; 12-thread Nether `2,418.3/s`, End `8,184.8/s`; `.fworld` v3 LZ4 full save `4.27 ms` (`11.70x` vs serial Anvil `49.98 ms`), incremental `1.02 ms`, cold load `0.10 ms`; 200-player `SharedFrame` fan-out `22.31 ms -> 0.11 ms` (`195.4x`).
- 2026-11-21 release measurements on this 12-CPU Linux host:
  - `ferrite-region` `holders` (4096 chunks): add 12998 us -> 2637 us (batched), remove 7799 us -> 556 us (batched).
  - `ferrite-light` `pool_throughput` (12 full-height jobs): 1/2/3/4/8 workers achieved 1.00x / 1.96x / 2.92x / 3.78x / 5.23x on disjoint centres (`light_throughput`: 24-section full lighting 9.398 ms, torch place/remove 0.133 ms).
  - `ferrite-net` `writer_stage` (128 conns, 4x64 KiB/50 ms): inline p99 184,695 us (727 batches) -> offload p99 35,470 us (1,375 batches).

## 7. Release and tooling hygiene

- Default startup directory creation is implemented (`ferrite_server::startup_dirs`, wired into
  `run_server`): creates `config/`, `<world>/{data,datapacks,dimensions/minecraft/overworld/region,entities,playerdata}`
  (or `<root>/playerdata` without an on-disk world), and `<worldgen.work-dir>` (default `worldgen/`,
  only when `generator = "vanilla"`), without creating `plugins/` or overwriting existing files.
  Fresh, existing, custom-path (relative and absolute), and headless cases are covered by unit and
  integration tests (`startup_dirs`); Windows GUI startup (`cfg(windows)` Tauri shell) cannot be
  exercised on Linux and still needs a Windows check.
- `v2.1.0` is the current release tag on `main`. Verify the Windows ZIP/NSIS output
  and release notes when publishing updates, and attach artifacts to the GitHub release.
- Keep `docs/CONFIG.md`, `docs/windows-console.md`, `README.md` and `docs/COMMANDS-STATUS.md` in
  step with code; `docs/README.md` marks the historical handoff and coordinator notes.
- The Windows GUI is a Tauri 2 + WebView2 shell in `crates/ferrite-server`; its layout,
  lifecycle, and known limitations are documented in `docs/windows-console.md`.
- The portable Windows ZIP currently contains the executable and start notes; the NSIS
  bundle described by `tauri.conf.json` is not part of the ZIP.

- Dependabot alert 10 (`glib 0.18.5`, GHSA-wrw7-89jp-8q8g) is open. `glib` is a Linux GTK
  transitive dependency of Tauri; Ferrite compiles Tauri only under `cfg(windows)`, where
  GTK/glib is not built, and `cargo tree -i glib@0.18.5` prints nothing on Linux. The patched
  `glib 0.20` requires a newer GTK stack than the locked `gtk 0.18.2`; revisit when Tauri
  updates it and do not force the version.

## 8. Runtime worlds

Runtime worlds are implemented internally: a persistent descriptor, a dedicated chunk source and
scheduler keyed by `WorldId::Runtime`, and transactional unload with player/transfer guards and
save/restore (`game/runtime_persist.rs` tests). They are used by commands and server tooling.

Open: exercising create, reopen, unload, block access, travel and persistence with a real client.
`docs/handoff-runtime-worlds-2026-11-11.md` is a point-in-time note from before travel, shutdown
persistence and unload recovery landed; its "Known gaps" list is superseded by this section.

## 9. Orca FB integration review

Reviewed the five FB worktrees through Orca on 2026-11-10. The release branch already contains the
production behavior from their completed work. Duplicate or older patches were not applied:

- `fb-cross-region-leads`: moving-piston overlays are present. Its local SmallFireball changes
  duplicate the projectile support and tests already in the release.
- `fb-end-respawn`: completed commits are already in the release base; its current local changes
  are handoff notes only.
- `fb-trader`: horse bucking and gear, cat/parrot taming and persistence, trader spawning, and the
  cross-dimension block-entity bridge are present. The branch's trader-spawn patch uses a separate
  per-region timer and removes traders after they drink, so that stale patch is not suitable.
  Its local portal preflight is superseded by the release's atomic block update.
- `fb-vault` and `fb-villager-trading`: completed code is already in the release base; local
  changes are handoff notes only.

Orca reported no live terminals for these worktrees. The full-workspace test result and the final
release checks below are the evidence for this release; they do not certify real-client visuals.

## 10. Vanilla command completeness (A4)

- Tree parity (`commands_tree_parity.rs`): 91/94 vanilla roots match exactly; 3 vanilla roots
  (`execute`, `gamerule`, `return`) and 3 non-vanilla diagnostic roots (`mspt`, `tps`, `profile`)
  have 6 allow-list entries (`a`/`c` only). Fixed `/item` (`target`), `/place template`
  (executable without `<pos>`), `/place feature` (opaque parser), and `/fillbiome` (`biome`
  `RegistryParser::Resource`).
- Non-player entity NBT: `/data get|merge|modify|remove entity`, `data modify ... from entity`,
  and `execute store ... entity` read region-published `EntityIndex` save-NBT snapshots and route
  writes to the owning region via `Action::EntityNbt` / `entity_save::apply_entity_nbt`.
- Component-aware item predicates: `execute if|unless items|slots` supports `[component=value,...]`
  filters reusing `parse_item_components` and `ferrite_item::Component` NBT.
- Remaining limitation: no real Minecraft client was attached during these checks.
