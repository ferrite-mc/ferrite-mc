# Local data-backed damage registry

- Base `origin/main`: `f4be922613985a9c027b4058ad8f9c63f65c95c6`.
- Branch/worktree: `yxshadxw/w7-damage-registry`, `../w7-damage-registry`.
- Tested implementation: `5b0da881bcfce31e68f4888a181fd8b3a450055f`; final delivery adds this documentation commit (PR head is final SHA).
- Independent of PRs #13 and #16. Coordinator merges; no world/entity damage application, physics, release/tag or host-op changes.

## Problem and faithful source

`FerriteRegistryAccess.damageTypeRegistry()` returned NEVER/HURT/DEFAULT/0.1 for every entry and derived the translation from the registry key. Examples from canonical generated files: fall requires WHEN_CAUSED_BY_LIVING_NON_PLAYER/FALL_VARIANTS/0.0; player_attack has message_id `player`; in_fire has effect `burning`. Installed API exposes exactly five properties: getTranslationKey, getDamageScaling, getDamageEffect, getDeathMessageType, getExhaustion.

`FerriteInternalBridge.getDamageEffect(name)` also returned DamageEffect.HURT during DamageEffect's own static initializer, leaving all six API constants null. The new adapter resolves each name independently to an immutable typed value and its real Sound.

Original adapter `tools/extractor/DamageTypeExtractor.java` compiles against the already-fetched official 26.3 jar and decodes **every generated damage_type JSON through DamageType.DIRECT_CODEC**. Thus optional effects/death-message defaults come from official codec behavior, not guessed fallback tables. It separately reads the public official constructor's optional defaults for test evidence, and all `DamageEffects` enum members' registered sounds. It bootstraps registries only; no world/server/network/EULA startup. `tools/extract_damage_types.py` verifies version and complete canonical filename/key coverage. No downloads, decompiled implementation, assets or official jars/data are committed.

Result: **51/51 DamageType definitions and 6/6 DamageEffect values/sounds**.

| API | Delivered value | Regression/evidence |
|---|---|---|
| getTranslationKey | death.attack + actual message_id (including camelCase IDs) | every raw generated JSON, player_attack example |
| getDamageScaling | typed DamageScaling from official codec | all 51 raw scaling values |
| getExhaustion | actual finite float, including zero; no invented nonnegative restriction beyond official codec | all raw exhaustion values, malformed numeric-string/overflow tests |
| getDamageEffect | canonical typed DamageEffect for decoded explicit/default effect | all raw fields or independently read official constructor defaults |
| getDeathMessageType | typed enum from decoded explicit/default value | all raw fields/default evidence |
| DamageEffect.getSound | interned Registry.SOUND_EVENT reference from official enum | all six names; explicit drowning/on-fire API constants checked |
| Registry get/size/iteration/identity/equality | existing canonical key manifest and interning; immutable value objects, key equality/hash, unknown key -> null | backing suite and parallel reads |

Missing resource, bad version/incomplete key set, absent required decoded fields, invalid enums, unknown effects/sounds, nonnumeric or nonfinite exhaustion fail explicitly. No empty/success placeholders. Generated definitions are immutable build-time snapshots, with no runtime datapack reload claim. Effect names remain distinct even when they share a sound (hurt/thorns). No API clone contract exists here; no mutable JSON escapes the loader.

## Verification

Java 25 (`C:/Program Files/Eclipse Adoptium/jdk-25.0.5.8-hotspot`), from java/ferrite-api-impl:

```powershell
$env:JAVA_HOME = 'C:/Program Files/Eclipse Adoptium/jdk-25.0.5.8-hotspot'
./gradlew.bat test --tests dev.ferrite.server.DamageTypeRegistryTest --tests dev.ferrite.server.FerriteRegistryBackingTest --console=plain
./gradlew.bat test build --console=plain
```

The build invokes `python tools/extract_damage_types.py vanilla/26.3 <ignored-output>/damage-types.json` (paths are resolved from the project). An ignored vanilla junction references the pre-existing official extraction read-only.

- Actual API regression failed before implementation on the fixed scaling value.
- Final focused: **12 tests** (4 new + 8 existing), all passed.
- Full affected Java suite: **293 tests, zero failures/errors/skips**; compile/build/shadedJar passed. Independent main base excludes the additional tests in #13/#16.
- Mutation: removing the numeric-type guard made the malformed-data regression fail; restored, then reran final focused/full validation.
- Both built jars audited: extractor classes/logs are excluded. No official assets, libraries or input JSON copied into git; generated output remains ignored.
- Diff/check/security/correctness/concurrency review passed. No new dependencies, skipped tests, weakened assertions or lint suppression. Existing deprecation warnings retained.
- No Rust changes/tests or live gameplay/client verification. Coordinator must build matching integrated Java jars before Rust plugin integration (`FERRITE_JAVA_PREBUILT=1`).

## Shared integration hunks

1. RegistryAccess: replace only damageTypeRegistry's fixed proxy with existing identityRegistry + FerriteDamageType, add a private-lane read-only damage-key helper. Preserve #13's identity/property factory changes and #16's GameRule dispatch.
2. InternalBridge: only getDamageEffect delegate. Preserve #16's separate legacyGameRuleBridge hunk and remote-owned methods.
3. build.gradle.kts: append dedicated generateDamageTypes task/resource root. Preserve #16's independent generateGameRules task if resolving the EOF hunk; both outputs/class exclusions must remain.
4. New lane-only helpers/extractor/tests and this handoff. No root manifests, Rust generator logic or remote-lane files edited.

Queue owner remains `w7-registry-api/docs/handoff/local-plugin-api-queue.md`; next independent verified gap is built-in Sound/Villager OldEnum behavior.

## Registry integration update

Merged latest `origin/main` `d55af6bae6a99e95c8a9edbbb33e1bbad2dd280c` (including #13/#16/#17) with a normal two-parent merge, without force or self-merging the PR. Resolved build.gradle's shared append hunk by preserving both full generation tasks, resource roots, exclusions and the GameRule test-reference property. Reviewed auto-merged RegistryAccess/InternalBridge: both property helpers/factories and the legacy GameRule bridge remain. All incoming Rust changes are byte-for-byte main (`git diff origin/main -- crates` empty).

Final integrated verification: **31 focused registry tests**, then **312 full Java tests, 0 failures/errors/skips**, Java25 wrapper `test build` including shadedJar passed. Both artifacts contain game-rules.json and damage-types.json and exclude this lane's extractor classes/logs/reference output. Diff/check/correctness review passed; tests unchanged. New CI runs on the pushed merge head; old independent head had passed both platforms. See PR head for the exact integration merge SHA.
