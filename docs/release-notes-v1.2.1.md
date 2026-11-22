# Ferrite 1.2.1

> Adds standalone **Vanilla Anvil (`.mca`) ↔ Ferrite `.fworld` (`FWRD` v3)** world converter binary (`ferrite-convert`) to all release archives and standalone release assets.

## What's New

- **Standalone World Converter (`ferrite-convert`)**:
  - `ferrite-convert anvil-to-fworld <world_dir> <out.fworld> [--compression lz4|zlib|gzip|none]`
  - `ferrite-convert fworld-to-anvil <in.fworld> <world_dir> [--compression zlib|lz4|gzip|none]`
  - Supports both Minecraft `26.3` (`dimensions/minecraft/{overworld,the_nether,the_end}/region/`) and legacy/Paper (`region/`, `DIM-1/region/`, `DIM1/region/`) layouts, automatically converting Overworld, Nether (`<stem>.nether.fworld`), and End (`<stem>.end.fworld`).
  - Converts `2,603` fixture chunks (`9.03 MiB` `.fworld`) in `0.17s` (`anvil-to-fworld`) and `0.49s` (`fworld-to-anvil`).
- **Release Packaging**:
  - Bundled `ferrite-convert` / `ferrite-convert.exe` inside `Ferrite-v1.2.1-ubuntu-x86_64.tar.gz` and `Ferrite-v1.2.1-windows-x86_64.zip`, and published standalone `ferrite-convert-v1.2.1-*` assets on GitHub Releases.
