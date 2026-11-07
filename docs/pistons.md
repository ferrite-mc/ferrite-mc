# Pistons

Pistons are implemented server-side in `game::piston`. Resolution follows the 26.3
`PistonStructureResolver` rules:

- The main line and every sticky branch share the twelve-block limit.
- An unextended piston base is movable; an extended one is not. Block entities and states with
  destroy speed `-1` are never moved.
- `PUSH` (the 16 glazed terracotta blocks) is pushable only along the piston's facing. It extends
  on the main line, is never pulled by a sticky piston, and is not dragged by a slime or honey
  branch.
- `POPPED` states (torches, wire, water, plants, ...) are destroyed; their drops are produced once
  from the stored state after the atomic block commit.
- A moved slime or honey block pulls the contiguous line behind it and branches only to its
  perpendicular sides. Slime and honey never stick to each other.
- During retraction the head cell reads as air while the pull plan is built, matching vanilla's
  removal of the head before `PistonStructureResolver` runs.
- Colliding push lines are reordered so the line that runs into an already-moving line moves
  first.

The region adapter refuses a plan that reads an unowned or unloaded position, so no world
mutation occurs across a region boundary. A sticky retraction whose target is across the boundary
retracts without pulling and leaves the target in place. Every edited position is checked against
the world border and the dimension's build height (`within_height`) before the commit.
`AnvilChunkSource::compare_set_blocks` publishes the whole expected-state batch atomically, then
redstone and neighbour updates run and destroyed blocks use the normal drop path.

Entity pushes use the destination's real collision shapes rather than an air test: water, plants
and other non-colliding states allow the displacement, while a solid shape leaves the entity where
it is. Extending pistons carry mobs, items and players in the pushed cell; sticky retraction
carries mobs and players standing on the pulled block. Player retraction is covered by
`a_retracting_sticky_piston_pulls_a_player_standing_on_the_block`.

The client overlay is a two-tick vanilla-style moving-piston animation on top of the committed
world. Every moved block gets a `moving_piston` block update and a `minecraft:piston` block entity
at its destination. The entity NBT carries `blockState`, `facing`, `progress`, `extending`, and
`source`; the extension head travels from the base to the head cell with `source=true`, while
regular moved blocks use `source=false`. Progress starts at zero and advances by 0.5 each region
tick, so a move completes on the second tick. Retraction uses the same direction and progress
convention as `PistonMovingBlockEntity`: the client interpolates the visual block back toward the
piston.

Final block updates are held for two ticks and keyed by position, so a reversal or another piston
update at the same cell replaces the stale delayed update. The overlay is client-only: collision,
entity pushes, block-entity contents and drops continue to use the committed world state, keeping
drops exactly once. Region split/merge carries pending animations and delayed updates with their
section, and overlapping animations are replaced rather than duplicated.

The committed world state persists, but the moving overlay does not: a player who starts tracking
a chunk mid-animation sees the committed final block, not a moving-piston block entity. That is
the deliberate cost of committing the final state immediately; persisting a moving block entity
would require staging the movement instead. Interrupted movement at a region boundary, resolver
parity for a real client, and full cross-region runtime load behaviour still need client/runtime
coverage. The local vanilla extractor records push reactions and block-entity capability for every
state in `vanilla/26.3/generated/ferrite/piston_states.json`; existing vanilla extractions need to
be regenerated with `python tools/fetch_vanilla.py`.

Focused coverage includes the twelve/thirteen-block limit, immovable and block-entity refusals,
`PUSH` extension/retraction, slime/honey branching, glazed terracotta, sticky pull and push-only
targets, head removal, collision-shape entity clearance, animation progression and replacement,
delayed-update coalescing, region split/merge, unowned main lines and side branches, and
cross-boundary retraction. The animation timing and packet sequence have not been checked with a
real Minecraft client.
