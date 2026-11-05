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
height (`within_height`), so nothing is pushed out of the world.
`AnvilChunkSource::compare_set_blocks` validates and publishes the resulting batch
across loaded chunks atomically. Sticky retraction leaves its target in place if it
cannot resolve a pull.

Pistons currently move blocks immediately: there is no moving-piston block entity or
validated vanilla animation/timing. Extending pistons do carry non-player entities (mobs,
items) standing in a cell a block is pushed into one block along, when the destination has
room for them (test `an_extending_piston_carries_the_mob_in_the_way`); a retracting sticky piston carries non-player
entities standing on the pulled block toward the piston (test
`a_retracting_sticky_piston_carries_the_mob_standing_on_the_block`, needs the fixture world and
has not been run); players are not carried. The server sends
a block event and final block updates, but client animation has not been verified.
Vanilla edge cases, including interrupted movement, world borders, drops and full
cross-region runtime behaviour, still need regression coverage and parity validation.
