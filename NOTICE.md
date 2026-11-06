# Notices

Ferrite is an independent, from-scratch implementation. It is **not** affiliated with, endorsed by,
or sponsored by Mojang Studios or Microsoft. "Minecraft" is a trademark of Mojang Synergies AB.

- **No Mojang code or assets are distributed in this repository.** Vanilla data (registries, block
  states, worldgen JSON, loot tables, ...) is generated *locally* on your machine from the official
  server jar you download yourself (`python tools/fetch_vanilla.py`) and is git-ignored.
- Some tests and tools (`tools/wirecheck`, `tools/extractor`) compile small Java
  programs against that locally downloaded jar to check Ferrite's behavior against the real game.
  Only our own source is committed, never the jar, its classes, or dumps produced from it.
- This repository contains no world generator. Vanilla world generation is done by the official
  generator inside the server jar you download yourself, run unmodified in a separate JVM process
  by our own host (`java/ferrite-worldgen-host`, see `docs/worldgen-host.md`). The host is compiled
  on your machine against that jar and contains only our code; no Mojang code is copied or distributed
  here. Faster replacements of generator steps, if added, are our own algorithms written from
  the observable behaviour and checked against the unmodified jar's output. Ferrite never accepts the Minecraft EULA for you: you set `eula=true`
  in `eula.txt` yourself, and the host's jar reads the same file.
- Compatibility target for plugins is the public Bukkit/Spigot/Paper API (a separate project,
  licensed by its own authors). The Paper API artifact is downloaded by the Gradle build and not
  vendored here.

## License and ported code

Ferrite is licensed under **GPL-3.0-or-later** (`LICENSE`). It was MIT OR Apache-2.0 up to
commit `6b5b191`; it moved to GPLv3 so that code from GPL/LGPL projects can be ported directly.
Ported code keeps a comment naming the upstream project and file. Upstream projects:

| Project | License | Used for |
|---|---|---|
| [Folia](https://github.com/PaperMC/Folia) / [Paper](https://github.com/PaperMC/Paper) | GPL-3.0 (patches), MIT (some API) | regionizer, chunk system, schedulers |
| [Lithium](https://github.com/CaffeineMC/lithium) | LGPL-3.0 | game-logic optimizations |

Other optimization projects are listed with their licenses in `docs/OPTIMIZATIONS.md`.
