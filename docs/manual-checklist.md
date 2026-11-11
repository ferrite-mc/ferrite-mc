# Manual 26.3 client checklist

Everything below needs a real Minecraft Java 26.3 client (protocol 777). The automated tests use
protocol bots and decode packets; they do not show how the client renders or animates anything.
**None of these steps has been run by the project's tests.** Tick a box only after doing it, and
write down the Ferrite commit you ran.

Commit under test: ______  Client: 26.3  Date: ______

## 1. Vanilla data and EULA

- [ ] `python tools/fetch_vanilla.py` finished and `vanilla/26.3/generated/reports/blocks.json`
  exists (needs Java 25).
- [ ] First start without `eula.txt`: the server creates it with `eula=false` and refuses to run.
  Set `eula=true` yourself, start again, the server listens on `bind-address`.
- [ ] `generator = "vanilla"` starts the worldgen host (needs the EULA); `generator = "superflat"`
  starts without it.

## 2. Join

- [ ] Add the server in the client list; the MOTD and player count show.
- [ ] Join with `online-mode = false`: no disconnect, spawn position correct, terrain loads
  around you (view distance as configured), no chunk holes after walking 100 blocks.
- [ ] `/stop` from the console saves and exits; rejoin: your position, inventory and any placed
  blocks are still there.

## 3. Dimension travel

- [ ] Build and light a nether portal; step in: arrive in the Nether, portal re-created on return.
- [ ] Light an end portal frame with 12 eyes of ender; step in: you arrive on the obsidian
  platform at x 100, y 48, z 0. Falling off the world deals void damage.
- [ ] From the End, the exit portal returns you to your bed or the world spawn.

## 4. Plugin output

- [ ] Put a test plugin in `plugins/` (see `tools/fetch_test_plugins.py`), start, join.
- [ ] Plugin commands run and reply in chat; plugin messages, titles and boss bars render.
- [ ] A plugin calling `Server#createWorld` returns a world, `/execute in <world>` reaches it, a
  teleport into it works, and `unloadWorld` is refused while you stand in it. Restart and check
  the world still exists (`docs/BACKLOG.md` section 8).

## 5. Pistons (`docs/pistons.md`)

- [ ] A piston pushes up to 12 blocks, refuses 13, and a sticky piston pulls one block back.
- [ ] Animation: the pushed block slides over two ticks, with no one-frame flicker of the block at
  its source or destination, and the head extends smoothly. Repeat with slime and honey.
- [ ] Entities (player, item, mob) standing in the path are carried or pushed, not stuck.
- [ ] Rejoin or walk away and back while a long piston clock is running: moving blocks show up
  in their current position, never duplicated.
- [ ] Stop the server while a piston clock runs, start again: no stray moving block, no missing
  block (the server-side half is covered by a test; confirm the client view).

## 6. The End (`docs/end-dimension.md`)

- [ ] The dragon spawns, its boss bar shows (pink, sky darkened, music), crystals heal it and
  explode when hit.
- [ ] Dragon motion: circling, strafe orbit and fireball, landing approach, perch, takeoff. The
  perch subphases (scanning, attacking, flaming) are approximated; note what looks wrong.
- [ ] Breath cloud radius and damage over time match vanilla.
- [ ] Kill the dragon: exit portal, dragon egg (first kill), 12000 experience.
- [ ] The first gateway opens: the magenta creation beam plays once from the bottom, about ten
  seconds. Leave the area, come back, and relog: the beam must **not** start again.
- [ ] Walk through a gateway: you land on the outer island (a void-only target cancels the trip).
- [ ] First trip home through the exit portal: the credits screen shows, then you respawn; the
  "Free the End" advancement toast shows. A second trip skips the credits.
- [ ] Re-summon the dragon with four crystals around the exit portal: beams, roars, spike crystals
  restored one by one, the dragon appears. Restart mid-sequence: it resumes.
