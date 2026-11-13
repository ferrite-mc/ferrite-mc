# W7: real typed Bukkit GAME_RULE registry

## Revision and integration

- Branch `yxshadxw/w7-gamerule-registry`, based on fetched main `f4be922613985a9c027b4058ad8f9c63f65c95c6`.
- Tested implementation head `4960a1bd441f04ddc4c5b47947cc80a24f80e0b5`; delivery includes the following documentation commit (resolve the PR head for its final SHA).
- Independent of unmerged PR #13 (`8a071802d30a72bb9e9bed64d8f5e9a630dbef65`). Preserve both features during integration. Coordinator merges; no self-merge, tags or release changes.
- PR #13 last observed: Linux CI passed, Windows CI still running. No actual CI failures observed to repair.

## Owning truthful data and extraction

The existing `crates/ferrite-command/src/gamerule.rs::SPECS` has 59 typed defaults. Read-only comparison found no differences against the official jar, but Java does **not** copy or parse that implementation table.

`tools/extractor/GameRuleExtractor.java` is an original adapter compiled against the user's existing `vanilla/26.3/lib/server-26.3.jar` and its extracted libraries. It invokes only `SharedConstants.tryDetectVersion()` and `Bootstrap.bootStrap()`, then reads `BuiltInRegistries.GAME_RULE`. The public official `GameRule` API supplies `valueClass()`, `defaultValue()`, `getDescriptionId()` and `requiredFeatures()`; registry IDs come from the actual registry.

No MinecraftServer, world, player, save, network listener or EULA object is created. No download is performed. No decompiled implementation is copied. Generated output/classes/logs stay under ignored build output; official jars/assets/data are not committed. This worktree's ignored vanilla junction points at the pre-existing extraction as input.

`tools/extract_gamerules.py` compiles/runs the adapter, checks version + complete key/ID coverage against the independent official `reports/registries.json`, and with `--verify` runs a second process enumerating public official `GameRules` **fields instead of the registry iterator**. Both direct readings of the official jar must agree. The second output feeds regression comparisons, independently from the runtime adapter/resource reader.

Result: **59/59 keys supported; 47 Boolean, 12 Integer; zero unrepresentable keys**. The only nonempty required feature set is `max_minecart_speed` requiring `minecraft:minecart_improvements`. These are immutable registry definitions; current per-world values are a different contract.

## API coverage

| API | Delivered behavior | Evidence/test |
|---|---|---|
| Registry.GAME_RULE, RegistryKey.GAME_RULE, class lookup GameRule.class | Same cached registry; 59 real definitions, complete iteration/stream/keyStream/size | actualBukkitRegistryEnumeratesTypedDefaultsWithoutHost; generated report key set |
| Registry.get/getKey, Keyed.getKey/key | Canonical namespaced identities; unknown key/namespace -> null, null input -> explicit NPE | actualBukkitRegistryEnumeratesTypedDefaultsWithoutHost; parallelReadsAreInternedAndEnumerationIsStable |
| GameRule.getType/getDefaultValue | Exact Boolean/Integer class and official immutable boxed default for every key | everyTypeDefaultAndTranslationMatchesIndependentOfficialConstants compares all definitions to independently read official fields |
| getName/translationKey/requiredFeatures | Namespaced registered name, actual official description ID and immutable typed FeatureFlag set | same; unknown features rejected, no generic empty-feature fallback |
| GameRules constants; GameRule legacy constants | Every installed static constant initializes. Ordinary aliases retain canonical identity; transformed aliases use the conversion functions supplied by the installed API | allInstalledConstantsAndSupportedLegacyConversionsInitialize |
| GameRule.getByName/values | Installed API behavior preserved: canonical bare/namespaced names work, no invented camelCase aliases; values returns a defensive array | same |
| concurrent reads/equality/hash | Registry values interned with ConcurrentHashMap putIfAbsent, allowing same-thread recursive alias initialization; canonical objects have typed key equality/hash; no artificial ordering contract (GameRule is not Comparable) | parallelReadsAreInternedAndEnumerationIsStable |
| missing/invalid extraction data | Missing resource, wrong version/key set, missing fields, wrong primitive/type, fractional/overflowing Integer and unknown features throw explicit IllegalStateException, with causes. No false/zero coercion | malformedMissingAndUnfaithfulDefinitionsAreRejected |

Paper's installed `GameRule` static initializer uses `InternalAPIBridge.legacyGameRuleBridge`; it previously reached an unavailable host. The adapter retains the canonical modern rule and **both** API-provided conversion functions. `getDefaultValue()` applies the official to-legacy conversion, including inverted Boolean and Boolean/int aliases. Registry iteration contains only canonical modern values. No world mutation or host conversion implementation was added; remote runtime workers can use the retained modern/conversion fields when implementing that separate path.

Source corroboration for registered-name and legacy-value behavior: [Paper CraftGameRule](https://github.com/PaperMC/Paper/blob/main/paper-server/src/main/java/org/bukkit/craftbukkit/CraftGameRule.java). The installed API's bytecode was the authority for which alias functions and constants actually exist.

## Required initialization order

A regression exposed Paper's circular `GameRule`/`GameRules` static initialization: after starting Bukkit, a plugin accessing modern constants first could still leave legacy constants null. `Bootstrap.startServer()` now initializes `GameRule.values()` before plugin entry. This is a two-line registry initialization hunk (comment + call), not a world/lifecycle bridge change. The regression calls the actual production startup with native disabled; it failed **before** this production change and passed after it. Tests were not ordered or disabled to hide the cycle.

Use Bukkit APIs after `Bootstrap.startServer()`, as live plugins do. Arbitrary direct modern-static access in a fresh JVM **before server API initialization** is not a supported lifecycle. No reflective writes to final API constants or patched Paper classes are used.

## Reproduction and final evidence

Java 25 at `C:/Program Files/Eclipse Adoptium/jdk-25.0.5.8-hotspot`:

```powershell
$env:JAVA_HOME = 'C:/Program Files/Eclipse Adoptium/jdk-25.0.5.8-hotspot'
python tools/extract_gamerules.py vanilla/26.3 java/ferrite-api-impl/build/gamerule-evidence/game-rules.json --verify
# From java/ferrite-api-impl:
./gradlew.bat test --tests dev.ferrite.server.GameRuleRegistryTest --tests dev.ferrite.server.FerriteRegistryBackingTest --console=plain
./gradlew.bat test build --console=plain
```

- Original actual Bukkit API test failed on unavailable registry size before implementation.
- Initial implementation exposed unavailable legacy bridge, then the startup-order regression exposed null modern aliases; both fixed in production code.
- Final focused: **13 tests** (5 new GameRule + 8 registry backing), all pass.
- Final full affected Java suite: **294 tests, zero failures/errors/skips**; compile/build/shadedJar passed. This independent base does not yet contain #13's 14 new tests.
- Mutation review removed Boolean type validation: malformed-definition regression failed. Restored and reran final focused/full suite successfully.
- Separately generated local and Gradle extraction JSON compare equal. All keys/IDs match the official report. All defaults match the existing Rust model (read-only comparison).
- Inspected both built jars: neither packages GameRuleExtractor classes, verification reference, official server jar, or official bootstrap logs. Only the intended generated game-rule JSON is added as a resource, consistent with existing extracted registry build resources.
- `git diff --check` and correctness/security/identity/concurrency review passed; no dependencies, test exclusions, lint suppressions, Rust changes or host operation numbers changed. Existing Java/Gradle/native-library deprecation warnings remain.
- Rust integration not run; matching Java jars are built for coordinator's subsequent `FERRITE_JAVA_PREBUILT=1` integration. No world get/set, live plugin/client, rendering, datapack reload, or gameplay verification claim.

## Shared hunks / conflicts

1. RegistryAccess: two GAME_RULE dispatches and one dedicated lazy value factory. Preserve #13's separate value-property factory and MemoryKey explanation hunks.
2. generate_registry_keys.py: append game_rule to report-backed names. Preserve #13's data-driven sorted-ID loop.
3. build.gradle.kts: append a dedicated generateGameRules task/resource root and test reference property; existing registry task unchanged. Requires the already-fetched official bundled jar/lib (same prerequisite as current extraction).
4. InternalBridge: only legacyGameRuleBridge's Java delegate changed. Do not replace unrelated remote workers' bridge edits.
5. Bootstrap.startServer: only the documented registry pre-initialization call before plugins. Preserve any independent lifecycle edits.
6. Existing refusal test now asserts supported GAME_RULE coverage; MemoryKey/unknown registries still explicitly fail. PR #13 also edits MemoryKey wording, so retain both test contracts if resolving nearby hunks.

Continue the finite queue in `local-plugin-api-queue.md`; coordinator owns integration/merge/release.
