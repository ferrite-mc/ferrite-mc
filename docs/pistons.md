# Pistons

Piston work is in progress. `game::piston` currently computes a read-only movement
plan, with a twelve-block limit, basic sticky branching, and rejection when the world
cannot read a needed position. Redstone changes discover nearby pistons, check direct
and quasi-connectivity power, and apply extension or retraction in the region tick.
Changes are committed as one expected-state block batch, sent to tracking clients,
and passed to redstone and neighbour updates. Destroyed blocks use the normal drop
path. Sticky retraction, observer notification and a basic extension have integration
tests.

The local vanilla extractor records push reactions and block-entity capability for
every state in `vanilla/26.3/generated/ferrite/piston_states.json`. Existing vanilla
extractions need to be regenerated with `python tools/fetch_vanilla.py`. The caller
also supplies actual block-entity presence. Resolver parity across all branching,
world-border and height-limit cases is not yet established.

The region adapter rejects movement plans that read an unowned or unloaded position;
the tick also rejects edits outside the current world border or the dimension's build
height (`within_height`), so nothing is pushed out of the world. Delayed final block
updates are coalesced by position and checked against the current world state before
broadcast, so an earlier piston state cannot overwrite a later extension or retraction.
`AnvilChunkSource::compare_set_blocks` validates and publishes the resulting batch
across loaded chunks atomically. Sticky retraction leaves its target in place if it
cannot resolve a pull.

The world is still committed immediately as one atomic block batch. On top of that state,
clients receive a two-tick vanilla-style moving-piston overlay: every moved block gets a
`moving_piston` block update and a `minecraft:piston` block entity at its destination. The
entity NBT carries `blockState`, `facing`, `progress`, `extending`, and `source`; the extension
head travels from the base to the head cell with `source=true`, while regular moved blocks use
`source=false`. Progress starts at zero and advances by 0.5 each region tick, so a move
completes on the second tick. Retraction uses the same direction and progress convention as
`PistonMovingBlockEntity`: the client interpolates the visual block back toward the piston.

The final block updates are held for two ticks and keyed by position, so a reversal or another
piston update at the same cell replaces the stale delayed update. The overlay is client-only:
collision, entity pushes, block-entity contents, and drops all continue to use the committed
world state, preserving exactly-once drops. Unit tests cover progression and completion, NBT
state encoding, overlapping animations, interrupted extension/replaced by retraction, and a
full extension-then-retraction world-state round trip; a region test covers delayed-update
replacement. The protocol surface already represented these packets, so no new packet types
were needed.

Extending pistons carry mobs/items in the pushed cell when the destination has room
(`an_extending_piston_carries_the_mob_in_the_way`); sticky retraction carries mobs standing on
the pulled block (`a_retracting_sticky_piston_carries_the_mob_standing_on_the_block`). Players
are pushed too when both feet and head cells at the destination are air; the new
`an_extending_piston_pushes_a_player_in_the_way` integration test checks the position sync and
the moved block. Player retraction remains untested. The moving overlay has not been checked
with a real Minecraft client, and it is not persisted in chunk data: a player who starts
tracking the chunk mid-animation may see the committed final block instead of the overlay.
Interrupted movement at the region boundary, resolver parity, world-border/drop edge cases and
full cross-region runtime behaviour also still need coverage.
