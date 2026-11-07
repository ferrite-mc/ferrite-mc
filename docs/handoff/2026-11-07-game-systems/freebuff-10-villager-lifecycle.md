# Freebuff task 10: villager lifecycle, work, rest and restocking

Read `README.md`, `freebuff-03-villager-trading.md` and
`freebuff-09-villager-trade-handoff.md` first.

## Target

Make villagers live through a full day instead of only carrying trade state. This covers every
modelled profession, workstation claiming, the adult and baby schedules, going to a claimed bed at
night, meeting near a bell, and the vanilla two-restocks-per-day rule.

## What the source already has

- `VillagerState` persists profession, level, XP, workstation, offers, gossip and home.
- The 26.3 offer tables and deterministic per-villager draws are implemented.
- Trade menus synchronize concurrent viewers, apply per-player reputation pricing, and persist.
- `ferrite_inventory::Menu` already accumulates villager trade XP and exposes `take_villager_xp`.
- `apply_trades` levels a villager up and appends the new level's offers.
- `ai.rs` has path following and a villager home clamp, but it only wanders.

## Gaps found in the source

1. `ferrite_entity::villager::tick` is not called by the game loop. Villagers therefore never
   acquire a profession or restock in a running server.
2. Restocking uses a fixed 24000-tick timer and ignores the work schedule, the job-site reach
   rule, sold-out offers, and the two-per-day cap.
3. There is no workstation ownership: every nearby villager claims the same block.
4. There are no bed claims, no rest destination and no meeting destination. The AI has no
   activity-aware target.
5. Trade XP is accumulated by the merchant menu but never read back into the villager.
6. The schedule/restock/bed state has no persistence.

## Scope

### Schedule

Day time is `GameState.clock_time.rem_euclid(24000)`; tick 0 is 06:00. The Java table from the
Minecraft Wiki is:

| Tick | Employed adult | Unemployed adult / nitwit | Baby |
|---:|---|---|---|
| 10 | wander | wander | wander |
| 2000 | work | wander | wander |
| 3000 | work | wander | play |
| 6000 | work | wander | wander |
| 9000 | meet | meet | wander |
| 10000 | meet | meet | play |
| 11000 | wander | wander | wander |
| 12000 | rest | rest | rest |

Implemented as `Activity::{Idle, Play, Work, Meet, Rest}`. Every profession uses the same
schedule; the wiki documents no per-profession schedule table for Java. Profession-specific work
goals (farmer harvesting, librarian bookshelf inspection, fisherman docks) are **not** in scope.

### Workstation ownership

- A villager claims at most one workstation, and a workstation has at most one owner in its
  region. Ownership is checked against all villager records in the same `EntityStore`.
- Unemployed adults search up to 16 blocks horizontally and 4 vertically every 40 ticks, as
  today. The first unclaimed workstation assigns its profession and level-1 offers.
- An employed villager keeps its remembered block only while that block still maps to its
  profession. A removed or changed block clears the claim and allows a new matching block to be
  claimed.
- Nitwits and babies never take a profession. Babies may still claim a bed.
- The villager walks to its workstation during `Work`.
- "Reached the job site" for restocking follows the wiki: within 8 blocks horizontally and no
  more than 9 blocks below the villager's feet. Claim search remains the existing 16/4 range.

Known limitation: claims are per region store. Two villagers in different regions can claim the
same block near a region boundary because there is no global POI table in this task.

### Beds and meeting

- An adult or baby without a bed searches up to 32 blocks horizontally and 4 vertically for an
  unclaimed bed, canonicalizing head halves to the foot position. The shared pathfinder keeps 16
  waypoints, so a distant destination may need a shorter path or a direct approach; this is a
  known path-budget limit, not a silent teleport.
- A bed has at most one owner in its region; a removed bed clears the claim.
- During `Rest` the villager walks to its bed and stops there. The sleeping pose/occupied bed
  block state is not modelled.
- During `Meet` the villager walks to the nearest bell within 32 blocks, falling back to home.
  Bells are shared; villagers do not claim them.
- During `Idle`/`Play` the existing home-clamped wander continues.

### Restocking

- Restock only during `Work`, only when the villager has a valid workstation of its profession,
  is within job-site reach, has at least one sold-out offer, and has restocked fewer than two
  times today.
- At least 2400 game ticks must pass between restocks.
- A restock resets `uses`, `demand` and `special_price` on every offer, matching
  `MerchantOffer.resetUses`.
- `restocks_today` resets on a new day and after 12000 ticks without a restock (vanilla
  `RestocksToday` decay). The two-per-day cap still applies.
- No workstation or no sold-out offer means no restock; the old fixed timer is removed.

### Trade XP

- After a trade menu is committed, take the menu's accumulated villager XP and add it to the
  villager through the existing level/offer progression.
- This applies to the immediate click path and the once-per-tick `sync_trades` safety net.
- Wandering traders keep their existing no-level behavior.

### Persistence

Persist `Bed`, `LastRestock` (game time), `RestocksToday` and `RestockDay`. Read old saves
without these fields as "no bed, no restock yet today". `Activity`, destination and meeting point
are derived each tick and are not persisted.

### AI

- Villagers during `Work`, `Meet` or `Rest` follow a path to the destination instead of the
  random wander target.
- The home clamp and liquid avoidance still apply.
- If the destination is missing or unreachable, the villager stays near home; no teleport.

## Out of scope

- Global cross-region POI ownership.
- Sleeping pose, bed occupancy animation and wake-up animation.
- Profession-specific work actions, breeding/meeting socialization, gossip decay/sharing.
- Villager type biome variants, baby growth, zombie-villager conversion.
- Wandering trader, vault, horse, leash, wolf combat, fluids, pistons, End and worldgen.

## Tests

Entity unit tests:

- schedule boundaries for employed, unemployed/nitwit and baby villagers.
- two villagers cannot claim the same workstation; a second unclaimed block is used.
- a removed/replaced workstation clears the claim and a matching replacement is claimed.
- two villagers cannot claim the same bed; removing it releases the claim.
- restock requires `Work`, a valid job site, job-site reach and a sold-out offer.
- exactly two restocks per day, with the 2400-tick spacing and 12000-tick `RestocksToday` decay.
- restock clears uses, demand and special price.
- trade XP levels the villager and appends offers.

Server tests:

- a real region tick assigns a profession and sets the day's activity.
- trade-menu XP is applied to the villager after commit.
- bed/restock fields survive save/load.

AI test:

- a villager with a work destination walks toward it rather than selecting a random wander yaw.

## Acceptance

- All new tests above pass.
- Existing villager, merchant, reputation, concurrent-menu and persistence tests stay green;
  no test is weakened or deleted.
- `cargo fmt --all -- --check` passes.
- Scoped clippy with `-D warnings` passes for the changed crates.
- The final report distinguishes tested behavior from client-visible behavior that still needs a
  real Minecraft client.

## References

- Java schedule and job-site reach: <https://zh.minecraft.wiki/w/村民#日程安排> (Minecraft Wiki;
  the English wiki returned HTTP 403 to this runner).
- Two restocks per day, work-time restocking and job-site reach:
  <https://zh.minecraft.wiki/w/村民#工作>.
- Local 26.3 block states: `vanilla/26.3/generated/reports/blocks.json`; bed `part`/`facing` and
  bell block names are used for canonical positions.
- Local trade tables: `ferrite_data::villager_trades` under
  `vanilla/26.3/generated/data/minecraft/villager_trade`.
