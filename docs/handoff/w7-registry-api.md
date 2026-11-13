# W7 local worker: Bukkit registry value properties

## Revision and ownership

- Base: `origin/main`, fetched and verified at `c2c3ec51a6197a952aef9ea3174deaacc85e4f8b`.
- Branch: `yxshadxw/w7-registry-api`.
- Tested implementation head: `91ff797f5216dc00b1155db69052c9eb2dfadd62`.
- The following documentation commit is the delivery head; resolve it with `git rev-parse yxshadxw/w7-registry-api` or the PR head SHA. A document cannot contain its own commit hash.
- Deliver through a PR against main. Coordinator reviews/merges; this worker does not merge, change tags, or publish a release.
- No agents spawned. No Rust, host operation numbers, Player/entity mutations, ItemMeta/PDC, maps, networking, lifecycle, release, or policy changes.

## Evidence and data source

Inspected installed `io.papermc.paper:paper-api:26.3.build.227-stable` with Java 25 `javap`, including inherited `OldEnum` methods and `RegistryKey` types. `PaintingVariant` is exposed as **`org.bukkit.Art`**, and Cat's value type is **`Cat.Type`**. Wolf/Pig/Cow/Chicken variants and all five animal sound variant interfaces have no property getters beyond `Keyed`.

Canonical files are local generated official data, under `vanilla/26.3/generated/data/minecraft/<registry>/<key>.json`; report-backed identities come from `reports/registries.json`. This isolated worktree uses an ignored junction from `vanilla` to the existing `../plan1104/vanilla`, as a read-only reference. No extraction output, texture, audio, model, jar, runtime world, or Mojang source is committed.

Observed schemas/counts (all files compared independently in Java tests for implemented property families):

| Registry | Count | Relevant fields |
|---|---:|---|
| trim_material | 11 | description (styled component), palette_id |
| trim_pattern | 18 | description (component), asset_id, decal |
| jukebox_song | 22 | sound_event (registered key), description, length_in_seconds, comparator_output |
| painting_variant | 51 | width, height, asset_id; optional title, author |
| instrument | 8 | sound_event (registered key), description, use_duration, range |
| wolf_variant | 9 | assets, baby_assets, spawn_conditions |
| cat_variant | 11 | asset_id, baby_asset_id, spawn_conditions |
| frog_variant | 3 | asset_id, spawn_conditions |
| pig/cow/chicken_variant | 3 each | asset_id, baby_asset_id, model, spawn_conditions |
| wolf/cat/pig/chicken_sound_variant | 7/2/3/2 | adult_sounds, baby_sounds |
| cow_sound_variant | 2 | ambient_sound, death_sound, hurt_sound, step_sound |

`javap -v` on Paper's `InstrumentRegistryEntry`, `JukeboxSongRegistryEntry`, and `PaintingVariantRegistryEntry` independently establishes duration >= 0, range > 0, song length > 0, comparator 0..15, and dimensions 1..16. Nonfinite numbers, numeric strings, fractional integers, negative durations (including float underflow to -0.0), and out-of-range values are rejected.

## Supported getter matrix

`RegistryValuePropertiesTest` is the new regression class; method names below identify individual coverage. Tests call the actual Bukkit APIs, with a transport that throws if a property reaches the host, plus an explicit no-host test.

| Installed getter/API | Canonical source | Before | Delivered typed result | Regression |
|---|---|---|---|---|
| TrimMaterial.description(), TrimPattern.description() | description | unavailable host | Adventure Component, preserving color/style | trimDescriptionsPreserveStyleAndTranslation |
| TrimMaterial/TrimPattern.getTranslationKey() | description.translate | fabricated key-derived string | actual TranslatableComponent key | same; successfulReadsAreSnapshotsAndDoNotPromiseReload covers literal descriptions |
| JukeboxSong.getSound() | sound_event | unavailable host | interned Registry.SOUND_EVENT Sound | everySongMatchesCanonicalFieldsAndTypedSound |
| JukeboxSong.getDescription() | description | unavailable host | Adventure Component | same |
| JukeboxSong.getTranslationKey() | description.translate | key-derived string | actual component translation key | same; literal description explicitly unsupported |
| JukeboxSong.getLengthInSeconds() | length_in_seconds | unavailable host | finite positive float, seconds | same; numericBoundariesFollowInstalledRegistryEntryAnnotations |
| JukeboxSong.getComparatorOutput() | comparator_output | unavailable host | exact integer 0..15 | same; missingMalformedAndInvalidSongDataFailExplicitlyAndNeverReachHost |
| Art.getBlockWidth(), getBlockHeight() | width, height | unavailable host | exact integers 1..16 | everyPaintingMatchesDimensionsAssetAndOptionalComponents; paintingOptionalMeansAbsentNotMalformedAndDimensionsAreBounded |
| Art.assetId() | asset_id | unavailable host | Adventure Key (NamespacedKey implements Key), no asset bytes | everyPaintingMatchesDimensionsAssetAndOptionalComponents |
| Art.title(), author() | optional title, author | unavailable host | nullable immutable Adventure Component; absent author stays null | same; malformed/null field fails explicitly |
| Art.getId(), Art/Cat.Type/Frog.Variant.ordinal(), compareTo() | generated synchronized registry IDs | unavailable host | integer/order matching current Ferrite wire order | legacyVariantAndPaintingIdsFollowTheHostsSortedGeneratedKeys |
| Art/Cat.Type/Frog.Variant.name() | canonical key | key-derived name | uppercase key, preserved | existing registry tests; order tests exercise value lookups |
| MusicInstrument.getDuration(), getRange() | use_duration, range | JSON float, unvalidated | finite bounded floats, duration in seconds | allInstrumentsMatchCanonicalDataWithoutAHost; numericBoundariesFollowInstalledRegistryEntryAnnotations; negativeDurationCannotUnderflowIntoAnAcceptedZero |
| MusicInstrument.getSound(), description(), translationKey() | sound_event, description | existing file reads | validated typed Sound, styled Component, actual translation key | allInstrumentsMatchCanonicalDataWithoutAHost; instrumentRejectsInvalidNumbersAndUnknownSoundWithoutHostFallback |
| Keyed.getKey()/key(), Registry.get()/stream()/size()/iterator() | existing generated key manifest | cached identities | preserved canonical membership, unknown key -> null | FerriteRegistryBackingTest (8 tests, independently recomputes counts) |
| equality/hash and canonical cached references | registry kind/type + key | identity-only proxies; instrument Object equality | type-aware key equality for implemented proxies; instrument key equality; canonical interning retained | trimValuesPreserveTypedReferencesAndKeyEqualityAcrossInstances; concurrentReadsReturnCachedImmutableValues; allInstrumentsMatchCanonicalDataWithoutAHost |

The installed `ArmorTrim.equals` compares material/pattern by reference (`javap -c` uses `if_acmpne`). Canonical registry interning therefore remains necessary and is tested, in addition to value-object equality across independently constructed instances. These interfaces, MusicInstrument, and ArmorTrim expose no public clone method. Immutable components are modified by creating new values; tests verify the originals do not change.

## Identity-only and unsupported inventory

| Registry/API family | Status and exact boundary |
|---|---|
| Wolf/Pig/Cow/Chicken Variant; animal SoundVariant; ZombieNautilus.Variant; SulfurCube.Archetype; PotPatternType | Existing generated identities retained. Installed APIs expose no texture/model/sound-set property accessors. JSON fields do not justify inventing API methods. Cached key/type behavior tested for the prioritized variants. |
| Cat.Type, Frog.Variant | Full installed identity + OldEnum accessors implemented. Texture/spawn conditions have no installed getter. |
| Trim material palette, trim pattern asset/decal | Present in data, absent from these installed value interfaces; no invented accessors. Registry builder APIs/mutation remain outside this lane. |
| Painting asset content; variant textures/models; sound/audio bytes | Not present in generated data pack JSON. Only the Art asset key is exposed. No resource loading, synthetic assets, or client rendering claims. |
| Inline/unregistered sound definitions | Public getSound() requires a Sound. Current generated instrument/song files use registered string keys. Object-valued or unknown sound_event fails explicitly; no fake registry member. |
| Legacy translation getter on literal description | Component getter works; translation getter throws UnsupportedOperationException because the component has no translation key. |
| Tags and registry mutation/builders | Existing explicit host behavior retained. No empty tag collections or successful mutation placeholders added. Host exceptions propagate. |
| game_rule | Remains explicitly unavailable. Report lists **59 keys/IDs**, not value defaults. Installed GameRule.getType()/getDefaultValue() require real typed defaults; GameRules constants resolve through Registry.GAME_RULE (not independent ready-made values). commands.json cannot supply defaults. No fabricated true/0 values. World get/set bridge belongs to remote world API worker. |
| memory_module_type | Remains explicitly unavailable. Report lists **115 keys/IDs**; installed final MemoryKey exposes **45 typed constants**, a private `(NamespacedKey, Class<T>)` constructor, and no contract for the other 70 value types. Returning just 45 would conceal incomplete full-registry enumeration. Existing constants need no new registry implementation. Entity memory mutation belongs to remote entity API worker. Error text now explains the typed-subset limitation. |
| Other W6 registries audited, unchanged | Villager Profession/Type (OldEnum), Banner PatternType (OldEnum/getIdentifier), POI is(BlockData)/hasOccupants, DataComponentType.isPersistent, DialogLike, Biome/Attribute/Sound properties. Runtime/legacy properties not represented by their current JSON/reports remain explicit host failures. Data-driven scope here does not assign speculative legacy banner identifiers, POI occupancy, persistence flags, dialog runtime behavior, or remote-owned attributes. |
| Existing specialized registries | Structure.getStructureType, Enchantment, PotionEffectType, DamageType, GameEvent, block/item/menu/particle/potion and StructureType implementations retained; no broad parity claim. Item, attributes, and runtime operations remain under their assigned lanes. |

## Snapshot and wire semantics

- Each value loads one private JSON snapshot on its first successful parse from Bootstrap's configured extracted-data directory. Synchronized publication makes simultaneous first reads safe; no mutable JSON is exposed.
- Missing source -> UnsupportedOperationException naming kind/key; malformed JSON, missing required fields, invalid keys/numbers -> IllegalStateException naming kind/key/field; IO errors retain UncheckedIOException causes. An absent optional component alone returns null. JSON null or malformed optional content is an error.
- A failed file parse is not cached, so configuring/fixing the source can recover before first successful parse. A parsed object is retained even if a required field is invalid. Subsequent file or Bootstrap directory changes do not reload an existing snapshot. This matches the host's static bootstrap model; datapack reload is **not implemented or promised**.
- `generate_registry_keys.py` now emits synchronized data-driven IDs in the same sorted-relative-path order as `crates/ferrite-net/src/registry.rs::json_files` / `load_vanilla` (lines 52 and 103 onward). This is Ferrite's actual protocol ordering, not a guessed Mojang numeric ID or a promise of stable IDs between data snapshots.

## Validation and review

Java: `C:/Program Files/Eclipse Adoptium/jdk-25.0.5.8-hotspot`. From `java/ferrite-api-impl`:

```powershell
$env:JAVA_HOME = 'C:/Program Files/Eclipse Adoptium/jdk-25.0.5.8-hotspot'
./gradlew.bat test --tests dev.ferrite.server.RegistryValuePropertiesTest --tests dev.ferrite.server.FerriteRegistryBackingTest --console=plain
./gradlew.bat test build --console=plain
```

- Original RED: all 9 initial new tests failed (unavailable host routes/invalid instrument number). Numeric boundary and float-underflow regressions also failed before their fixes.
- Final focused: **22 tests** (14 new + 8 existing), all passed.
- Final full affected Java suite: **303 tests / 44 classes, 0 failures, 0 errors, 0 skipped**. `build` including compile, fixtures, and shadedJar succeeded.
- Mutation review: removed integer maximum validation temporarily; `missingMalformedAndInvalidSongDataFailExplicitlyAndNeverReachHost` failed on comparator 16. Restored code and reran focused and full build successfully.
- Existing deprecation/unchecked, Gradle deprecation, and SQLite native-access warnings remain; no lint suppression or test exclusions added.
- `git diff --check` passed. Reviewed correctness, data validation, concurrency, identity, shared-file scope, accidental assets/secrets, and dependencies. No new dependencies or assets.
- No Rust files changed: cargo tests/fmt/clippy and Rust integration were not run. Matching Java jars were built; coordinator must use these jars before Rust plugin integration with `FERRITE_JAVA_PREBUILT=1`. No EULA action taken.
- Real Minecraft client, live plugins, actual audio/texture rendering, runtime world mutations, datapack reload, and integrated remote-lane release validation were not exercised.

## Shared integration hunks

1. `FerriteRegistryAccess.java`: only identity factory selection for six data-backed/OldEnum kinds, and MemoryKey refusal explanation. No other registry dispatch branches edited.
2. `tools/generate_registry_keys.py`: four lines add synchronized DATA_DRIVEN ID output using existing canonical keys. No build.gradle/root manifest changes.
3. Existing `FerriteMusicInstrument.java`: replaced private duplicate file parsing with the shared lane-specific validated snapshot reader.
4. New lane-only helpers `FerriteRegistryValueData` and `FerriteRegistryValueProperties`, plus `RegistryValuePropertiesTest` and this handoff.

Coordinator: include this PR before final integration/release; do not infer real-client or reload support from Java test success.
