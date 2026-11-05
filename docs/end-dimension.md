# End dimension status

## Travel (in progress)

- Eyes fill inward-facing portal frames; twelve filled frames activate the 3x3 portal.
- End portal blocks initiate travel with a cooldown; entering creates the 5x5 obsidian platform and clears its headroom. End returns use the stored spawn or overworld spawn.
- The End uses the configured terrain generator.
- The frame, eye and platform geometry have unit tests. A network bot test verifies Overworld-to-End travel and the platform; exit spawn safety and cooldown behavior need further testing in a running client.

## Dimension rules (in progress)

- End void damage starts below Y=-64; End sea level is 0. Dimension travel preserves the player's health and food in the client update.
- The 26.3 vanilla dimension data has fixed time and a zero visual sky-light factor, but its `has_skylight` property is true; the server's dimension helper currently reports no skylight for the End. Lighting parity needs investigation.
- Cross-dimension command travel and actual falling behavior still need runtime verification.

## Remaining

- Exit fountain, gateways, dragon, crystals, boss bar, egg and fight lifecycle are not implemented.
