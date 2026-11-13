# Biome legacy registry follow-up

- Branch `yxshadxw/w7-biome-registry`, **stacked on PR #19**, parent `7f2e865724f2f5fe3aaa82fc51bcb90f25502bff` (underlying main `fdfc887b09a279366463b26dad70bdc216039feb`).
- Tested code `d8e33b678fcf9759468d01dd965f48784207565c`; delivery adds this documentation commit. Coordinator merges #19 first and retargets/rebases this follow-up onto main as appropriate. Do not duplicate parent commits or merge automatically.
- Reuses #19's legacy proxy rather than duplicating it. No dependency on #13, although both have adjacent generated-ID hunks: preserve both.

Installed Biome exposes OldEnum and a default translationKey, but generic identity dispatch sends ordinal/compareTo to host and fabricates the wrong translation prefix. All **67 canonical biomes** now use exact API defaults and stable local wire ordering. IDs are generated from sorted canonical JSON paths exactly as `ferrite-net/src/registry.rs::json_files/load_vanilla` enumerates synchronized biomes, not guessed Mojang numeric IDs.

The generator adds only the worldgen/biome sorted-ID output. Legacy helper adds only that type; #19's selected ordering already makes values()[ordinal] valid. Actual API valueOf/name, typed comparisons, key identity and defensive arrays work after normal Bukkit API startup with no native host. Every canonical biome is compared independently to local JSON filenames/order and the installed translation contract; 512 parallel reads remain interned.

Deprecated **Biome.CUSTOM is not a generated registry entry**. Existing valueOf("CUSTOM")/name identity remains; installed default translation executes. ordinal/compareTo now fail explicitly without a fabricated wire ID or host call. Registry.BIOME.get(minecraft:custom) remains null. Canonical comparison against CUSTOM also fails rather than assigning an invented order.

Java 25, from java/ferrite-api-impl:

```powershell
$env:JAVA_HOME = 'C:/Program Files/Eclipse Adoptium/jdk-25.0.5.8-hotspot'
./gradlew.bat test --tests dev.ferrite.server.BiomeRegistryValuesTest --tests dev.ferrite.server.LegacyRegistryValuesTest --tests dev.ferrite.server.FerriteRegistryBackingTest --console=plain
./gradlew.bat test build --console=plain
```

All 3 new API tests failed before the fix. Final focused **15 passed**, full Java **296 tests, 0 failures/errors/skips**, build/shadedJar passed. Inverting the default-method condition made the CUSTOM regression fail; restored and reran final checks. No dependencies, assets, test exclusions, suppression, Rust changes or host op changes. Diff/security/correctness review and git diff --check passed. Read-only existing vanilla junction reused. No biome mutation, terrain generation, world physics, datapack reload, live client or gameplay validation claim.

Shared hunks: #19 helper's type map; customBiome fallback only; generator's new biome ID block. Preserve #13's distinct data-driven IDs and value-property helper, plus #16/#18 factories and remote-owned behavior. Queue stays in original w7-registry-api worktree.
