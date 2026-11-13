# Static POI registry values

- Independent main base `fdfc887b09a279366463b26dad70bdc216039feb`, branch/worktree `yxshadxw/w7-poi-registry` / `../w7-poi-registry`.
- Tested code `75f800f1793bbc98e72d0b94b1d728449100b6b1`; delivery adds this documentation commit. Coordinator merges; no self-merge/tags/releases.
- No dependency on #13/#16/#18/#19/#20. No live POI occupancy, AI/villager state, entity operations or world physics edits.

Installed `io.papermc.paper.entity.poi.PoiType` exposes `is(BlockData)` and `hasOccupants()`, previously unavailable host dispatch. Both are static definition properties: matching canonical block states and whether the type has nonzero ticket capacity. [Primary Paper implementation](https://github.com/PaperMC/Paper/blob/main/paper-server/src/main/java/io/papermc/paper/entity/poi/PaperPoiType.java) confirms that hasOccupants is `maxTickets != 0`, not a query of current world occupants.

Original `tools/extractor/PoiTypeExtractor.java` bootstraps the already-fetched official 26.3 registries without a world/server/EULA. It reads all **21 POI types**, their `maxTickets()` and `matchingStates()`, resolving state IDs through Block.BLOCK_STATE_REGISTRY. It independently scans **all 35,723 official block states** with `PoiType.is(state)` and requires identical extracted sets. The Python runner verifies version and exact key coverage against the independent official registry report.

Java values retain immutable private sets and canonical key equality/hash/interning. `is(BlockData)` uses the existing validated FerriteBlockStates state-string conversion (no new block implementation); malformed properties/null input/foreign getter exceptions propagate. `hasOccupants()` exposes only static capacity semantics. Unknown registry keys remain null; missing resource, wrong version/incomplete registry, absent fields, invalid/noninteger/out-of-range/duplicate state IDs fail explicitly. No empty/success placeholders. Data is a static build snapshot, with no reload claim.

Java 25, from java/ferrite-api-impl:

```powershell
$env:JAVA_HOME = 'C:/Program Files/Eclipse Adoptium/jdk-25.0.5.8-hotspot'
./gradlew.bat test --tests dev.ferrite.server.PoiRegistryValuesTest --tests dev.ferrite.server.FerriteRegistryBackingTest --console=plain
./gradlew.bat test build --console=plain
```

- Original actual API regression failed on host dispatch.
- Final focused **12 tests** (4 new + 8 existing); full **293 Java tests, 0 failures/errors/skips**. Compile/build/shadedJar passed.
- Java tests independently read reports/blocks.json to exercise every positive POI state, all state variants of matching block types, and every block's default state against all POI predicates. Bed head/foot, blast furnace and ordinary stone examples included.
- Immutable snapshots, all capacity flags, typed key/cache identity, 512 concurrent reads, invalid data/input and propagation of foreign BlockData exceptions verified.
- Mutation inverted capacity condition; actual API regression failed, restored and final focused/full validation rerun.
- Source security/correctness/concurrency review, jar audit and git diff --check passed. Extractor classes/logs excluded from both jars; generated data/classes remain ignored. Existing vanilla junction reused read-only. No official sources/assets/jars copied into git, new dependencies, suppression or test exclusions.
- Rust integration/live client/plugin/gameplay not run; matching Java jars built for coordinator's subsequent FERRITE_JAVA_PREBUILT=1 integration.

Shared hunks: RegistryAccess adds only a read-only POI key helper and specific point_of_interest_type factory; preserve other PR dispatches/proxies. build.gradle appends its independent generation task/root; preserve GameRule/Damage/Banner tasks if resolving EOF hunks. New helpers/extractor/tests only, no remote-owned classes edited. Queue remains in original w7-registry-api worktree.
