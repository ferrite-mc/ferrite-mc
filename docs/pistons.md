# Pistons

Piston work is in progress. `game::piston` currently computes a read-only movement
plan, with a twelve-block limit, basic sticky branching, and rejection when the world
cannot read a needed position. It is not connected to the region tick or redstone.
The module also prepares expected-state changes for extension and retraction and
provides the piston-specific direct and quasi-connectivity power check. These are
covered by unit tests, but do not animate or apply a piston in a running server.

The block reaction list is incomplete and has not been validated against all 26.3
states. In particular, the available block properties do not expose vanilla push
reactions or whether a block state supports a block entity. The caller must supply
block-entity presence; presence alone is not a complete state classification.
Do not use this planner for live movement until those gaps are resolved.

Extension, retraction, drops, animation and block events, observer notifications,
and runtime region-boundary handling remain unimplemented and untested. A complete
operation must validate every source and destination is owned before mutating any
block. `AnvilChunkSource::compare_set_blocks` can atomically validate and publish a
batch across loaded chunks, but is not yet connected to piston movement; ownership
checks and runtime integration remain necessary.
