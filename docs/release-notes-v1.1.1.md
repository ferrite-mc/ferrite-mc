# Ferrite 1.1.1

> Portable & Windows launch hotfix for **Minecraft Java 26.3 (protocol 777)**.

## Fixes

- **Fixed `loading vanilla registry` path error (`os error 3`) on portable & Windows builds**:
  - Previously, `ferrite-server`, `ferrite-net`, and multiple runtime game modules resolved `vanilla/26.3/generated` from the compile-time `CARGO_MANIFEST_DIR` path baked in on the GitHub Actions runner (`D:\a\ferrite-mc\...`), ignoring `./vanilla/26.3/generated`, `<exe_dir>/vanilla/26.3/generated`, and `FERRITE_VANILLA_ROOT`.
  - Added unified runtime resolution in [`ferrite-protocol`](../crates/ferrite-protocol/src/lib.rs) (`FERRITE_VANILLA_ROOT` -> runtime override -> CWD & executable ancestors -> manifest fallback) and updated `ferrite-net` plus all server runtime data loaders (`game/`, `main.rs`) to use it.
- **Added automatic first-launch Vanilla 26.3 data bootstrapper (`vanilla_setup`)**:
  - When `vanilla/26.3/generated` is missing on first launch (after accepting `eula.txt`), `ferrite-server` now locates an installed Java 25+ JDK (checking `FERRITE_JAVA25`, `JAVA_HOME`, `PATH`, and standard Windows/Linux JDK install paths), downloads the official Minecraft `26.3` `server.jar` from Mojang (SHA-1 verified), runs Mojang's `--reports --server` data generator and embedded extractors (`CREATE_NO_WINDOW` on Windows), and extracts templates/resources into `./vanilla/26.3/generated/`.
  - When Java 25 is not installed, reports a clear actionable Korean/English error instead of an OS path error.
- **Included `tools/fetch_vanilla.py` and `tools/extractor/*.java` in release archives**:
  - Updated `.github/workflows/release.yml` so both `Ferrite-v1.1.1-ubuntu-x86_64.tar.gz` and `Ferrite-v1.1.1-windows-x86_64.zip` bundle `tools/` and updated `START-HERE.txt`.
