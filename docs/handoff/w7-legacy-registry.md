# Built-in legacy registry value completion

- Branch `yxshadxw/w7-legacy-registry`, isolated worktree `../w7-legacy-registry`.
- Base `origin/main` `fdfc887b09a279366463b26dad70bdc216039feb`.
- Tested implementation `270e4085cff586551549b6e7955331fb7810bb53`; delivery adds this handoff commit (PR head is final SHA).
- Independent of #13/#16/#18. Coordinator merges; no runtime entity, attribute, world, network or release changes.

## Faithful coverage

Installed Paper 26.3.build.227-stable declares OldEnum on Sound and Villager.Profession/Type. The existing identity proxy implemented key/name only; ordinal/compareTo fell through to the unavailable host. Profession.translationKey also incorrectly used the generic registry-name prefix rather than its installed default method.

| Family | Official count | Delivered |
|---|---:|---|
| Sound | 1,991 | full key/name/ordinal/typed compareTo, canonical interning, key equality/hash |
| Villager.Profession | 15 | same, plus exact installed API default translationKey invocation |
| Villager.Type | 7 | full key/name/ordinal/typed compareTo, canonical interning, key equality/hash |

IDs are read only from locally generated official `reports/registries.json` through the existing registry-ids resource; no guessed IDs or new extraction needed. Registry key iteration for these three kinds now follows that ID order, so installed values()[ordinal] behavior works. Other identity registries retain their existing iteration behavior.

All actual Sound.valueOf canonical/name lookups and Villager valueOf/values APIs are exercised after normal no-native Bootstrap.startServer. Unknown lookups retain API exceptions/null semantics. Wrong-type/null comparisons fail with ClassCastException/NullPointerException; unsupported factory kinds or unknown IDs fail explicitly. No mutation/clone API exists on these immutable value interfaces. Host exceptions are not swallowed, and selected value methods never call host.

## Verification

Java 25; from java/ferrite-api-impl:

```powershell
$env:JAVA_HOME = 'C:/Program Files/Eclipse Adoptium/jdk-25.0.5.8-hotspot'
./gradlew.bat test --tests dev.ferrite.server.LegacyRegistryValuesTest --tests dev.ferrite.server.FerriteRegistryBackingTest --console=plain
./gradlew.bat test build --console=plain
```

- Original API regression failed on Sound.ordinal host dispatch; a second regression failed on values() order before fixing selected key iteration.
- Final focused **12 tests** (4 new + 8 existing), full **293 tests with 0 failures/errors/skips**. Compile/build/shadedJar passed.
- Every ID/count compared independently against official report JSON, including all sound names. Typed conversions, invalid input, defensive arrays and 512 parallel reads/equality checks passed.
- Mutation reversed compareTo ordering; report-based regression failed. Restored and reran focused/full validation.
- Diff/security/correctness review and `git diff --check` passed. No new dependencies, generated assets, test exclusions, suppression or Rust changes. No real client/audio/gameplay claims.
- Ignored vanilla junction references the already-existing extraction; no data/jars/assets committed. Existing Java/Gradle deprecation warnings remain.

## Integration conflict with #13

This PR and #13 both extend `identityRegistry` factory selection in FerriteRegistryAccess. **Retain both disjoint branches**: explicit factory, then FerriteLegacyRegistryValues for sound_event/villager_profession/villager_type, then #13's FerriteRegistryValueProperties for trim/song/art/cat/frog, then generic identity proxy. The supported-kind sets do not overlap. Also retain this PR's selected key-ordering hunk.

#16's GAME_RULE and #18's DamageType factories are independent. Do not replace their registry dispatch or remote-owned attribute behavior. This PR edits no shared generator/build files. The durable workstream queue is maintained in the original w7-registry-api worktree.
