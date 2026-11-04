# 내장 최적화 (Built-in optimizations)

Ferrite는 Fabric/Paper 생태계의 대표적인 최적화 모드가 하는 일을 **기본 탑재**합니다.
모드 코드를 그대로 가져오지 않습니다. **기법을 Rust로 새로 구현**하고, 바닐라와 동작이
같은지 테스트로 확인합니다. 최적화가 바닐라 동작을 바꾸는 경우(예: 일부 Lithium 옵션,
Pufferfish의 DAB)는 설정으로 끌 수 있게 합니다.

이 문서는 2026-10-30 기준으로 각 프로젝트의 **저장소를 직접 clone하여 LICENSE 파일과
`gradle.properties`/브랜치를 확인한 결과**를 기록합니다. 저장소는 `--filter=blob:none`
shallow clone으로 확인했으며, 인용한 파일/클래스 경로는 해당 시점의 기본 브랜치(또는 명시한
버전 브랜치) 기준입니다.

- 대상: **Minecraft Java 26.3, protocol 777**
- 라이선스 방향: Ferrite는 **GPLv3로 전환 예정** (현재 저장소는 MIT OR Apache-2.0)
- 원칙: **Mojang 코드/에셋은 절대 가져오지 않습니다.** 아래 프로젝트들도 Mojang 바닐라
  클래스를 수정하는 패치 형식이므로, Ferrite에는 *수정된 바닐라 코드*가 아니라 **독자
  구현된 Rust 기법**만 들어갑니다.

## 범례

### 상태

| 표시 | 의미 |
|---|---|
| ✅ | 구현되어 있고 테스트로 검증됨 |
| 🔧 | 부분 구현/진행 중 (크레이트는 있으나 최적화가 미완) |
| 📋 | 예정 (해당 크레이트/모듈이 아직 없거나 기법 미적용) |
| ⛔ | 범위 밖 (클라이언트 전용, 혹은 26.3에 적용 불가) |

### 라이선스/포팅

GPLv3로 배포되는 프로젝트에 다른 라이선스 코드를 넣을 때의 판단 기준입니다.

| 표시 | 의미 |
|---|---|
| 🟢 직접 포팅 가능 | GPL-3.0, LGPL-3.0, MIT, Apache-2.0. Rust로 옮긴 번역물도 원 라이선스를 따르며, GPLv3 전체와 호환됨. `NOTICE.md`에 저작자/라이선스 고지 필요. |
| 🟡 기법만 참고 | 라이선스는 허용되지만 **26.3용 코드가 없음**(구버전 전용). 알고리즘/아이디어만 참고하고 26.3 동작에 맞게 새로 작성. |
| 🔴 사용 불가 | All-Rights-Reserved, 라이선스 불명, 또는 GPLv3와 비호환. 코드/구조 복사 금지. |

> **LGPL-3.0 주의**: LGPL-3.0은 GPLv3와 결합·재배포가 가능합니다(§3, §4). 다만 Ferrite
> 바이너리를 배포할 때 LGPL 부분의 소스 제공 및 수정/재링크 가능성 고지를 지켜야 합니다.
> Rust 포팅은 "번역"이므로 원 저작자 표시가 필요합니다. MIT는 저작권 고지와 허가문을
> `NOTICE.md`에 포함하면 됩니다. Paper의 GPL 코드 중 일부 저자는 별도로 MIT 재라이선스에
> 동의했지만(`Paper/LICENSE.md` 목록), **목록에 없는 저자의 코드는 GPL-3.0**입니다.

### 26.x 지원 판정

- ✅: 26.3을 명시한 브랜치/태그/`gradle.properties`가 존재
- ⚠️: 26.1~26.2까지만 지원 (26.3 코드 없음)
- ❌: 26.x 이전 버전에서 개발 중단

---

## 한눈에 보기

| 프로젝트 | 저장소 | 라이선스 (LICENSE 파일 확인) | 26.3 지원 | 포팅 |
|---|---|---|---|---|
| Lithium | <https://github.com/CaffeineMC/lithium> | LGPL-3.0 | ✅ `mc26.3-0.26.3` (2026-10-28) | 🟢 |
| C2ME | <https://github.com/RelativityMC/C2ME-fabric> | MIT (단, `c2me-opts-accel-opencl/`는 ARR) | ✅ `minecraft_version=26.3` (2026-10-30) | 🟢 |
| Moonrise | <https://github.com/Tuinity/Moonrise> | GPL-3.0 | ✅ `supported_minecraft_versions=26.3` (2026-10-27) | 🟢 |
| Starlight | <https://github.com/PaperMC/Starlight> | LGPL-3.0 | ❌ 1.20.1에서 중단 (2024-03-08) | 🟡 |
| ScalableLux | <https://github.com/RelativityMC/ScalableLux> | LGPL-3.0 | ✅ `ver/26.3.0` 브랜치 | 🟢 |
| FerriteCore | <https://github.com/malte0811/FerriteCore> | MIT | ⚠️ 26.1까지 (2026-03-24) | 🟢 |
| ModernFix | <https://github.com/embeddedt/ModernFix> | LGPL-3.0-or-later | ⚠️ `26.1` 브랜치까지 | 🟢 |
| Krypton | <https://github.com/astei/krypton> | LGPL-3.0 | ✅ 26.3 (2026-09-30) | 🟢 |
| VMP (Very Many Players) | <https://github.com/RelativityMC/VMP-fabric> | MIT | ✅ `ver/26.3.0` 브랜치 | 🟢 |
| Pufferfish | <https://github.com/pufferfish-gg/Pufferfish> | GPL-3.0 (패치), MIT (기타) | ❌ 1.21.10 (2025-12-21) | 🟡 |
| Purpur | <https://github.com/PurpurMC/Purpur> | MIT (`LICENSE`) | ✅ `apiVersion=26.3` (2026-10-28) | 🟢 |
| Leaf | <https://github.com/Winds-Studio/Leaf> | MIT 패치(일부 GPL/LGPL/Apache), 바이너리 GPL-3.0-only | ✅ `apiVersion=26.3` (2026-10-25) | 🟢 |
| Gale | <https://github.com/GaleMC/Gale> | GPL-3.0 | ⚠️ 26.2 (2026-10-01) | 🟢 |
| Paper | <https://github.com/PaperMC/Paper> | GPL-3.0 (+ 일부 저자 MIT) | ✅ `apiVersion=26.3` (2026-10-29) | 🟢 |
| Folia | <https://github.com/PaperMC/Folia> | GPL-3.0 (`PATCHES-LICENSE`) | ✅ `apiVersion=26.3` (2026-10-29) | 🟢 |
| Async | <https://github.com/AxalotLDev/Async> | GPL-3.0 | ✅ `minecraft_version_range=[26.3]` (2026-10-04) | 🟢 |
| ServerCore | <https://github.com/Wesley1808/ServerCore> | MIT (일부 파일 GPL-3.0 표기) | ✅ `minecraft=26.3` (2026-09-16) | 🟢 |
| Velocity | <https://github.com/PaperMC/Velocity> | GPL-3.0 | ✅ `MINECRAFT_26_3(777)` (2026-10-28) | 🟢 |
| spark | <https://github.com/lucko/spark> | GPL-3.0 | ✅ `com.mojang:minecraft:26.3` (2026-10-24) | 🟢 |

---

## 1. 게임 로직

### 1.1 Lithium — 엔티티/블록/인벤토리 미세 최적화

- 저장소: <https://github.com/CaffeineMC/lithium>
- 라이선스: `LICENSE.md` = **LGPL-3.0**
- 26.3: ✅ 기본 브랜치 `develop`, 태그 `mc26.3-0.26.3` (2026-10-28)
- 포팅: 🟢 (LGPL-3.0 → GPLv3 프로젝트에 포함 가능)

| 기법 | 원본 구현 (파일/클래스) | Ferrite 위치 | 상태 |
|---|---|---|---|
| 엔티티 충돌: 청크 섹션 단위로 블록을 순회하고 충돌 shape을 캐시 | `common/entity/movement/ChunkAwareBlockCollisionSweeper.java`, `LithiumEntityCollisions.java` | `ferrite-entity`(신설) + `ferrite-world` | 📋 |
| VoxelShape 특수화(정렬된 직육면체/단순 큐브/빈 shape)와 offset 캐시 | `common/shapes/VoxelShapeAlignedCuboid.java`, `VoxelShapeSimpleCube.java`, `VoxelShapeEmpty.java`, `OffsetVoxelShapeCache.java` | `ferrite-world`(shapes) / `ferrite-math` | 📋 |
| 호퍼/컨테이너: 인벤토리 변경 추적, 리스너 없으면 스캔 생략, 슬롯 리스트 압축 | `common/block/entity/inventory_change_tracking/InventoryChangeEmitter.java`, `InventoryChangeTracker.java`, `common/hopper/LithiumStackList.java`, `HopperCachingState.java` | `ferrite-server::game`(호퍼) → `ferrite-entity` | 📋 |
| 레드스톤 와이어: 전력 계산 결과 캐시/전파 최소화 | `common/block/redstone/RedstoneWirePowerCalculations.java`, mixin `block/redstone_wire/DefaultRedstoneWireEvaluatorMixin.java` | `ferrite-server::game`(레드스톤) | 📋 |
| 폭발: 블록 저항/광선/엔티티 노출 계산 캐시 | `common/explosion/LithiumExplosion.java`, `DirectMappedExplosionBlockCache.java`, `ExplosionEntityRays.java` | `ferrite-server::game`(폭발) | 📋 |
| 길찾기: 블록 상태별 path 노드 캐시 | `common/ai/pathing/PathNodeCache.java`, `BlockStatePathingCache.java`, mixin `ai/pathing/PathNavigationRegionMixin.java` | `ferrite-entity`(AI) | 📋 |
| Brain/메모리: 변경된 메모리만 재평가 | `common/ai/task/memory_changes/BehaviorMixin.java`, `common/ai/brain/memories/MemoryModificationCounter.java` | `ferrite-entity` | 📋 |
| POI/블록 검색 캐시 (비-POI 목표 블록) | `common/ai/non_poi_block_search/CommonBlockSearchesCheckAndCache.java`, `LithiumMoveToBlockGoal.java` | `ferrite-world`(POI 인덱스) | 📋 |
| 섹션 단위 엔티티 이동 추적(깨우기/충돌) | `common/tracking/entity/SectionedEntityMovementTracker.java`, `SectionedColliderEntityMovementTracker.java`, `SectionedInventoryEntityMovementTracker.java` | `ferrite-entity` | 📋 |
| 랜덤 틱/엔티티 반복 할당 제거 | mixin `alloc/chunk_random/LevelMixin.java`, `alloc/entity_iteration/EntitySectionMixin.java`, `alloc/entity_tracker/ChunkMap$TrackedEntityMixin.java` | `ferrite-world` / `ferrite-entity` | 📋 |
| 유체 흐름: 방향/높이 계산 캐시 | `block/fluid/flow/FlowingFluidMixin.java`, `block/flatten_states/FluidStateMixin.java` | `ferrite-world`(유체) | 📋 |
| 빠른 컬렉션(Bucket list, Long2Object 리스닝 맵) | `common/util/collections/BucketedList.java`, `ListeningLong2ObjectOpenHashMap.java`, `PredicateFilterableList.java` | `ferrite-*` 공통 유틸 | 📋 |
| 문자열/레지스트리 interning | `common/util/deduplication/LithiumInterner.java` | `ferrite-data` | 📋 |

> Lithium의 일부 옵션(sub‑chunk collision, 깨우기 추적 등)은 바닐라와 **관측 가능한 동작이
> 달라질 수 있습니다.** Ferrite에서는 `ferrite.toml`의 `[optimizations.lithium]` 아래
> 옵션으로 노출하고, 기본값은 바닐라 동등으로 둡니다.

### 1.2 Async — 엔티티 병렬 틱

- 저장소: <https://github.com/AxalotLDev/Async>
- 라이선스: `LICENSE` = **GPL-3.0**
- 26.3: ✅ `gradle.properties`의 `minecraft_version_range=[26.3]` (2026-10-04)
- 포팅: 🟢
- 주의: 원본도 **alpha/실험적**이라고 명시. 엔티티 틱 순서·관측 동작이 바뀔 수 있어
  Ferrite에서는 region 단위 병렬화(ferrite-region)와 별개로 **기본 비활성** 옵션이어야 합니다.

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| 엔티티를 워커 풀에서 병렬 틱 (단계별 barrier로 순서 보존) | `common/ParallelProcessor.java`, `common/AsyncCommon.java` | `ferrite-region` + `ferrite-entity` | 📋 |
| 섹션 단위 엔티티 저장소의 동시 접근(락 최소화) | `mixin/entity/movement/EntitySectionMixin.java`, `EntitySectionStorageMixin.java`, `mixin/entity/EntityLookupMixin.java` | `ferrite-entity` | 📋 |
| 병렬 길찾기 (PathFinder/PathNavigation) | `mixin/entity/movement/PathFinderMixin.java`, `PathNavigationMixin.java` | `ferrite-entity` | 📋 |
| 병렬 POI 검색 | `mixin/entity/movement/PoiManagerMixin.java` | `ferrite-world`(POI) | 📋 |
| 병렬 몹 스폰/밴드 계산 | `mixin/entity/spawn/NaturalSpawnerMixin.java`, `SpawnStateMixin.java`, `LocalMobCapCalculatorMixin.java` | `ferrite-server` | 📋 |
| 병렬 센서(가장 가까운 엔티티/아이템/플레이어) | `mixin/entity/sensor/NearestLivingEntitiesSensorMixin.java`, `NearestItemSensorMixin.java`, `PlayerSensorMixin.java` | `ferrite-entity` | 📋 |
| 동시성 컬렉션/동기화 유틸 | `api/utils/ConcurrentCollections.java`, `api/fastutil/Long2ObjectConcurrentHashMap.java`, `common/utils/LockHelperMixin.java` | `ferrite-region` 공통 | 📋 |
| 스레드 안전 난수 | `mixin/utils/ThreadSafeLegacyRandomSourceMixin.java` | `ferrite-math` | 📋 |
| 파티클/블록 업데이트의 스레드 경계 처리 | `mixin/world/LevelMixin.java`, `BlockMixin.java`, `CollectingNeighborUpdaterMixin.java` | `ferrite-region` | 📋 |

### 1.3 ServerCore — 활성화 범위/청크 틱/몹캡

- 저장소: <https://github.com/Wesley1808/ServerCore>
- 라이선스: **MIT** (파일 상단에 GPL-3.0 표기가 있는 파일은 GPL-3.0) — `README.md`의 License 절
- 26.3: ✅ `gradle.properties` `minecraft=26.3`, `mod_version=1.5.20` (2026-09-16)
- 포팅: 🟢 (파일별 헤더 확인)

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| Entity Activation Range 2.0 (플레이어/월드 경계 기준 활성 범위) | `common/activation_range/ActivationRange.java`, `EntityTypeTests.java` | `ferrite-entity` | 📋 |
| 청크 틱 목록을 매 초만 갱신(매 틱 순회 제거) | `common/utils/ChunkTickManager.java`, `ChunkManager.java` | `ferrite-world` / `ferrite-region` | 📋 |
| 동기 청크 로드 축소 (lagspike 방지) | `common/config/OptimizationConfig.java`의 `reduce-sync-loads` | `ferrite-server`(ChunkSource) | 📋 |
| 커맨드 블록 파싱 캐시 | `common/config/OptimizationConfig.java`의 `optimize-command-blocks` | `ferrite-command` | 📋 |
| 중복 유체 랜덤 틱 취소 | `OptimizationConfig`의 `cancel-duplicate-fluid-ticks` | `ferrite-world` | 📋 |
| 몹캡 강제/브리딩 캡 | `common/config/data/mob_spawning/EnforcedMobcap.java`, `breeding_cap/BreedingCap.java` | `ferrite-server` | 📋 |
| 엔티티 타입별 필터 리스트 | `common/collections/FilteredList.java` | `ferrite-entity` | 📋 |
| Spark 연동 동적 설정(틱 지연 시 자동 완화) | `common/dynamic/DynamicManager.java`, `SparkDynamicManager.java` | `ferrite-profiler` | 📋 (profiler 🔧) |

### 1.4 Leaf — 대규모 게임 로직/월드 옵션

- 저장소: <https://github.com/Winds-Studio/Leaf>
- 라이선스: `LICENSE.md` — **패치는 MIT**(헤더에 GPL-3.0/LGPL-3.0/Apache-2.0 표기가 있으면 그 라이선스), **바이너리는 GPL-3.0-only**
- 26.3: ✅ `apiVersion=26.3` (2026-10-25)
- 포팅: 🟢 (패치별 헤더 확인 필수)

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| 랜덤 틱 시스템 재작성(대상 섹션만 순회) | `org/dreeam/leaf/config/modules/opt/OptimizeRandomTick.java`, `org/dreeam/leaf/world/RandomTickSystem.java` | `ferrite-world` | 📋 |
| 몹 스폰 스로틀/자연 스폰 청크 맵 | `opt/OptimizeMobSpawning.java`, `ThrottleNaturalMobSpawning.java`, `world/NatureSpawnChunkMap.java` | `ferrite-server` | 📋 |
| 비동기 길찾기(노드 평가기 캐시 포함) | `org/dreeam/leaf/async/path/AsyncPathProcessor.java`, `NodeEvaluatorCache.java`, `AsyncPath.java` | `ferrite-entity` | 📋 |
| 비동기 `/locate` (구조물 탐색) | `org/dreeam/leaf/async/locate/AsyncLocator.java` | `ferrite-command` | 📋 |
| 엔티티 충돌/활성/푸시 상태 캐시 | `org/dreeam/leaf/world/EntityCollisionCache.java`, `EntityActivation.java`, `EntityPushState.java` | `ferrite-entity` | 📋 |
| 블록 엔티티 최적화(수면/스냅샷) | `opt/OptimizeBlockEntities.java`, `SleepingBlockEntity.java`, `TileEntitySnapshotCreation.java` | `ferrite-world`(블록 엔티티) | 📋 |
| 플레이어 이동 패킷 처리 간소화 | `opt/OptimizePlayerMovementProcessing.java` | `ferrite-net` + `ferrite-server` | 📋 |
| 불필요 패킷 제거(맵 데이터/웨이포인트) | `opt/ReduceUselessPackets.java`, `SkipMapItemDataUpdates.java`, `OptimizeWaypoint.java` | `ferrite-net` / `ferrite-item` | 📋 |
| 가상 스레드 지원 | `opt/VirtualThreadSupport.java` | `ferrite-region`(IO) | 📋 |
| 고속 RNG(XorShift) | `opt/FastRNG.java` | `ferrite-math` | 📋 |
| 비동기 플레이어 데이터 저장 | `org/dreeam/leaf/async/AsyncPlayerDataSaving.java` | `ferrite-server::game::playerdata` | 📋 |

### 1.5 Gale — Paper 포크의 미세 최적화 패치 모음

- 저장소: <https://github.com/GaleMC/Gale>
- 라이선스: `LICENSE.md` = **GPL-3.0** (Paperweight 파일만 MIT)
- 26.3: ⚠️ `apiVersion=26.2` (2026-10-01) — 26.3 아님
- 포팅: 🟢 (GPL-3.0)
- 참고: 모든 기능은 `Features.md`에 설명되어 있고, 소스에는 `// Gale` 마커가 있습니다.

| 기법 | 원본 패치/클래스 | Ferrite 위치 | 상태 |
|---|---|---|---|
| 랜덤 틱 난수 이동/청크 틱 난수 최적화 | `patches/server/0024-Move-random-tick-random.patch`, `0025-Optimize-random-calls-in-chunk-ticking.patch` | `ferrite-world` | 📋 |
| 유체 방향/등반 가능/충돌 컨텍스트 캐시 | `0031-Improve-fluid-direction-caching.patch`, `0032-Cache-on-climbable-check.patch`, `0033-Make-EntityCollisionContext-a-live-representation.patch` | `ferrite-world` / `ferrite-entity` | 📋 |
| 컨테이너 검사를 비트셋으로 | `0034-Improve-container-checking-with-a-bitset.patch` | `ferrite-server::game` | 📋 |
| 엔티티/배열/람다 할당 제거 | `0041-Reduce-entity-allocations.patch`, `0082-Reduce-array-allocations.patch`, `0084-Reduce-lambda-and-Optional-allocation-in-EntityBased.patch` | `ferrite-entity` | 📋 |
| AI goal set / game rule map / attribute map / class map 최적화 컬렉션 | `0085`~`0089` 패치 | `ferrite-entity` / `ferrite-server` | 📋 |
| BlockStatePairKey 해시 캐시, CubeVoxelShape 배열 캐시 | `0091-Cache-BlockStatePairKey-hash.patch`, `0092-Cache-CubeVoxelShape-shape-array.patch` | `ferrite-world` | 📋 |
| CubePointRange 나눗셈을 곱셈으로, parts를 size로 | `0093`, `0094` 패치 | `ferrite-math` | 📋 |
| 빠른 청크 직렬화 | `0096-Faster-chunk-serialization.patch` | `ferrite-world::chunk_nbt` | 📋 |
| 몹 카운트 배열화, 엔티티 트래커 링크드 맵 | `0104-Store-mob-counts-in-an-array.patch`, `0105-Use-linked-map-for-entity-trackers.patch` | `ferrite-entity` | 📋 |
| PlayerInfo/keepalive 패킷 묶음 전송 | `0111`~`0114` 패치 | `ferrite-net` | 📋 |
| VarInt/VarLong 크기 사전 계산 및 쓰기 최적화 | `0136-Pre-compute-VarInt-and-VarLong-sizes.patch`, `0137-Optimize-VarInt-write-and-VarLong-write.patch` | `ferrite-protocol::codec` | 📋 |
| RandomSource 인스턴스 감소, xor-shift RNG | `0138-Reduce-RandomSource-instances.patch`, `0139-Add-xor-shift-random.patch` | `ferrite-math` | 📋 |
| 시야(LOS) 캐시 | `0141-Initialize-line-of-sight-cache-with-low-capacity.patch`, `0142-Reduce-line-of-sight-updates-and-cache-lookups.patch` | `ferrite-entity` | 📋 |
| 가상 스레드 | `0145-Virtual-thread-support.patch` | `ferrite-region` | 📋 |

### 1.6 Pufferfish — DAB/활성 범위/SIMD

- 저장소: <https://github.com/pufferfish-gg/Pufferfish>
- 라이선스: `PATCH-LICENSE` = **GPL-3.0**(패치), 그 외 파일 MIT (README의 License 절)
- 26.3: ❌ `gradle.properties` `version=1.21.10-R0.1-SNAPSHOT`, 마지막 커밋 2025-12-21
- 포팅: 🟡 (라이선스는 가능하지만 26.3 코드가 없음 → 기법만)
- 아래 `patches/NNNN-...` 경로는 `pufferfish-server/minecraft-patches/features/` 기준입니다.

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| SIMD 기반 팔레트(block state 압축 컨테이너) | `gg/pufferfish/pufferfish/simd/VectorMapPalette.java`, `SIMDChecker.java`, `SIMDDetection.java` | `ferrite-world`(팔레트) | 📋 |
| DAB (Dynamic Activation of Brain): 비활성 엔티티 brain 틱 주기 조절 | `patches/0009-Dynamic-Activation-of-Brain.patch`, `PufferfishConfig.java`의 `dab.*` | `ferrite-entity` | 📋 (기본 비활성 옵션) |
| 활성 범위(activation-range) | `patches/0009`, `PufferfishConfig`의 `activation-range.*` | `ferrite-entity` | 📋 |
| 비동기 몹 스폰 | `patches/0004-Optimize-mob-spawning.patch`, `PufferfishConfig`의 `enable-async-mob-spawning` | `ferrite-server` | 📋 |
| 질식(suffocation) 검사 최적화 | `patches/0003-Optimize-suffocation.patch` | `ferrite-entity` | 📋 |
| 투사체 청크 로드 제한 | `patches/0006-Reduce-projectile-chunk-loading.patch` | `ferrite-entity` / `ferrite-server` | 📋 |
| TPS catch-up(틱 부채 따라잡기) | `patches/0017-TPS-catchup.patch`, `PufferfishConfig`의 `tps-catchup` | `ferrite-region` | 📋 |
| Goal selector 스로틀 | `patches/0015-Throttle-goal-selector-during-inactive-ticking.patch` | `ferrite-entity` | 📋 |
| 내장 프로파일러(Flare) + Sentry | `gg/pufferfish/pufferfish/flare/ProfilingManager.java`, `sentry/SentryManager.java` | `ferrite-profiler` | 📋 (크레이트 🔧) |
| 비동기 실행 풀 | `gg/pufferfish/pufferfish/util/AsyncExecutor.java` | `ferrite-region` | 📋 |

### 1.7 Purpur

- 저장소: <https://github.com/PurpurMC/Purpur>
- 라이선스: `LICENSE` = **MIT** (Paper 파생 패치는 원저자 라이선스가 별도로 적용될 수 있음)
- 26.3: ✅ `apiVersion=26.3` (2026-10-28)
- 포팅: 🟢 (MIT, 단 Paper 패치 유래 코드는 `Paper/LICENSE.md` 확인)

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| 대체 keepalive (핑 스파이크 완화) | `PurpurConfig.useAlternateKeepAlive` (`settings.use-alternate-keepalive`) | `ferrite-net` | 📋 |
| TPS catch-up | `PurpurConfig.tpsCatchup` (`settings.tps-catchup`) | `ferrite-region` | 📋 |
| 블록 상태 캐시 초기화 | `PurpurConfig`의 `Block.BLOCK_STATE_REGISTRY.forEach(...::initCache)` | `ferrite-data` / `ferrite-world` | 🔧 (block_props) |
| TPSBar(보스바 TPS/MSPT/핑 표시) | `org/purpurmc/purpur/task/TPSBarTask.java`, `command/TPSBarCommand.java` | `ferrite-profiler` | 📋 |
| 월드별 세부 규칙(몹/엔티티/블록) | `org/purpurmc/purpur/PurpurWorldConfig.java` | `ferrite-config` | 📋 |
| WASD 탑승 컨트롤러 등 게임플레이 확장 | `org/purpurmc/purpur/controller/*` | `ferrite-server::game` | ⛔ (최적화 아님) |

> Purpur는 최적화보다 **게임플레이 토글**이 많은 포크입니다. Ferrite에서는 성능에 직접
> 영향을 주는 `tps-catchup`/`use-alternate-keepalive`/월드별 mob cap 정도만 옵션으로
> 수용하고, 나머지는 Paper/Bukkit 호환 단계에서 다룹니다.

---

## 2. 청크 시스템 · 월드 IO

### 2.1 Moonrise — Paper/Folia의 청크 시스템 + Starlight 내장

- 저장소: <https://github.com/Tuinity/Moonrise>
- 라이선스: `LICENSE.md` = **GPL-3.0**
- 26.3: ✅ `supported_minecraft_versions=26.3` (2026-10-27), `mc/26.3` 브랜치
- 포팅: 🟢 (GPL-3.0 → GPLv3에 그대로 통합 가능)
- 구조: `src/main/java/ca/spottedleaf/moonrise/patches/<기능>/...`에 바닐라 믹신,
  `common/`에 공용 자료구조.

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| 비동기 청크 상태 전이 스케줄러(우선순위 홀더, 티켓 전파) | `patches/chunk_system/scheduling/ChunkTaskScheduler.java`, `ChunkHolderManager.java`, `NewChunkHolder.java`, `ThreadedTicketLevelPropagator.java` | `ferrite-region` + `ferrite-world` | 🔧 (리전라이저 v3만 존재) |
| 청크 로드/생성/저장을 워커 풀로, 메인 스레드는 완료된 청크만 소비 | `scheduling/task/ChunkLoadTask.java`, `ChunkProgressionTask.java`, `ChunkFullTask.java`, `ChunkLightTask.java` | `ferrite-server`(IO 풀) + `ferrite-world` | 📋 |
| 리전 파일 IO를 별도 스레드로(동기 쓰기 플래그) | `patches/chunk_system/io/MoonriseRegionFileIO.java`, `SyncChunkWrites.java`, `ChunkSystemRegionFileStorage.java` | `ferrite-world::region` | 📋 (현재 동기) |
| 청크 언로드 큐 | `patches/chunk_system/queue/ChunkUnloadQueue.java` | `ferrite-world` | 📋 |
| 청크 단위 엔티티 저장소(`ChunkEntitySlices`) | `patches/chunk_system/level/entity/ChunkEntitySlices.java`, `ServerEntityLookup.java` | `ferrite-entity` | 📋 |
| 플레이어별 청크 로더(지역화된 청크 전송) | `patches/chunk_system/player/RegionizedPlayerChunkLoader.java` | `ferrite-server`(청크 스트리밍) | 🔧 (prefetch 구현) |
| 엔티티 트래커 최적화 | `patches/entity_tracker/ChunkMapMixin.java`, `TrackedEntityMixin.java` | `ferrite-entity` / `ferrite-net` | 📋 |
| 청크 틱 반복 최적화(틱 청크만 순회) | `patches/chunk_tick_iteration/*` | `ferrite-world` / `ferrite-region` | 📋 |
| 빠른 팔레트/비트 스토리지 | `patches/fast_palette/*`, `patches/bitstorage/SimpleBitStorageMixin.java`, `ZeroBitStorageMixin.java` | `ferrite-world`(팔레트) | ✅ (단일값/간접 압축) / 📋 (비트 연산 특수화) |
| 블록 카운팅(섹션별 non-air 카운트) | `patches/block_counting/LevelChunkSectionMixin.java` | `ferrite-world` | 📋 |
| NBT 읽기/쓰기 최적화(스트링 풀, 바이트 배열) | `patches/nbt_rw/NbtIoMixin.java`, `CompoundTagMixin.java`, `FriendlyByteBufMixin.java` | `ferrite-nbt` | 📋 |
| 블록 상태 property 접근 fast path | `patches/blockstate_propertyaccess/*` | `ferrite-data` / `ferrite-world` | 📋 |
| 랜덤 틱 대상 바이옴 캐시 | `patches/random_ticking/*` | `ferrite-world` | 📋 |
| 높이맵 갱신 최적화 | `patches/heightmap/HeightmapMixin.java` | `ferrite-world`(heightmaps) | 🔧 |
| POI 조회 최적화 | `patches/poi_lookup/PoiManagerMixin.java`, `AcquirePoiMixin.java` | `ferrite-world`(POI) | 📋 |
| 몹 스폰 밴드/캡 | `patches/mob_spawning/NaturalSpawnerMixin.java`, `LocalMobCapCalculator$MobCountsMixin.java` | `ferrite-server` | 📋 |
| 틱 루프 패킷 처리(패킷 처리량 제한) | `patches/tick_loop/PacketProcessorMixin.java`, `MinecraftServerMixin.java` | `ferrite-net` / `ferrite-server` | 📋 |
| 충돌: AABB/보셀 병합 캐시, 유체/폭발 캐시 | `patches/collisions/CollisionUtil.java`, `shape/MergedORCache.java`, `ExplosionBlockCache.java` | `ferrite-entity` / `ferrite-world` | 📋 |
| Starlight 조명 내장 | `patches/starlight/*`, `common/misc/Delayed26WayDistancePropagator3D.java` | `ferrite-light` | ✅ (Starlight 방식, 불일치 0) |
| Typed instance(블록/유체 상태 캐스팅 제거) | `patches/typed_instance/*` | `ferrite-world` | 📋 |

### 2.2 C2ME — 병렬 청크 생성/IO

- 저장소: <https://github.com/RelativityMC/C2ME-fabric>
- 라이선스: `LICENSE.md` — **MIT**, 단 `c2me-opts-accel-opencl/`는 **All Rights Reserved**
- 26.3: ✅ `minecraft_version=26.3`, `mod_version=0.4.2-alpha.0` (2026-10-30)
- 포팅: 🟢 (OpenCL 모듈 제외)
- 범위: 월드 생성(밀도 함수·노이즈·바이옴) 관련 기법은 이 저장소의 범위에서 **제외**합니다. 청크 시스템·IO·조명·스케줄링만 다룹니다.

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| 청크 시스템 재작성(상태 전이 병렬화) | `c2me-rewrites-chunk-system/` | `ferrite-region` | 📋 |
| 청크 IO 재작성(리전 파일 병렬 읽기/쓰기) | `c2me-rewrites-chunkio/`, `c2me-opts-chunkio/mixin/hide_sync_disk_writes_behind_flag/MixinRegionBasedStorage.java` | `ferrite-world::region` | 📋 |
| 청크 직렬화기 재작성 | `c2me-rewrites-chunk-serializer/` | `ferrite-world::chunk_nbt` | 📋 |
| 조명 스레딩 | `c2me-threading-lighting/` | `ferrite-light` | 📋 |
| 태스크 스케줄링(작업 훔치기/우선순위) | `c2me-opts-scheduling/`, `FlowSched/` | `ferrite-region` | 📋 |
| 스레딩 버그 수정(청크 IO/월드젠/일반) | `c2me-fixes-*-threading-issues/` | `ferrite-region` | 📋 |

> C2ME의 `c2me-opts-accel-opencl`은 **ARR**이므로 절대 참조/복사하지 않습니다.

### 2.3 Paper — Moonrise 통합 + 자체 패치

- 저장소: <https://github.com/PaperMC/Paper>
- 라이선스: `LICENSE.md` = **GPL-3.0**, 일부 저자는 MIT 재라이선스에 동의(목록 참조)
- 26.3: ✅ `apiVersion=26.3` (2026-10-29)
- 포팅: 🟢 (GPL-3.0; MIT 동의 저자 코드는 MIT 조건도 가능)

| 기법 | 원본 패치/클래스 | Ferrite 위치 | 상태 |
|---|---|---|---|
| Moonrise 청크 시스템 통합 | `paper-server/patches/features/0003-Moonrise-optimisation-patches.patch`, `paper-server/src/main/java/ca/spottedleaf/moonrise/**` | `ferrite-region` + `ferrite-world` | 🔧 |
| Entity Activation Range 2.0 | `features/0004-Entity-Activation-Range-2.0.patch` | `ferrite-entity` | 📋 |
| 연결/패킷 처리 최적화 | `features/0005-Optimize-Connection-and-add-advanced-packet-support.patch` | `ferrite-net` | 🔧 (틱 배치 flush) |
| VoxelShape 병합 최적화 | `features/0006-Optimize-Voxel-Shape-Merging.patch` | `ferrite-world` | 📋 |
| 흙/눈 퍼짐 최적화 | `features/0007-optimize-dirt-and-snow-spreading.patch` | `ferrite-server::game` | 📋 |
| Velocity 압축/암호 네이티브 사용 | `features/0008-Use-Velocity-compression-and-cipher-natives.patch` | `ferrite-protocol::frame` / `ferrite-net` | ✅ 압축(libdeflate) / 📋 암호 네이티브 |
| 비트 연산 인라인 | `features/0010-Optimize-Bit-Operations-by-inlining.patch` | `ferrite-world`(팔레트) | 📋 |
| 증분 청크/플레이어 저장 | `features/0014-Incremental-chunk-and-player-saving.patch` | `ferrite-world` / `ferrite-server` | 📋 |
| EntityScheduler 틱 최적화 | `features/0016-Optimise-EntityScheduler-ticking.patch` | `ferrite-region` | 📋 |
| 직렬화 실패 청크는 디스크에 쓰지 않음 | `features/0019-Only-write-chunk-data-to-disk-if-it-serializes-witho.patch` | `ferrite-world` | 📋 |
| Pathfinder 스트림 제거 | `features/0023-Optimize-Pathfinder-Remove-Streams-Optimized-collect.patch` | `ferrite-entity` | 📋 |
| 플레이어 이동 패킷 충돌 검사 최적화 | `features/0026-Optimise-collision-checking-in-player-move-packet-ha.patch` | `ferrite-net` + `ferrite-entity` | 📋 |
| 호퍼 최적화 | `features/0032-Optimize-Hoppers.patch` | `ferrite-server::game` | 📋 |
| 공용 유틸(좌표 팩/스레드 풀/카운터/스트링 풀) | `io/papermc/paper/util/MCUtil.java`, `concurrent/ScalingThreadPool.java`, `IntervalledCounter.java`, `StringPool.java`, `SizeLimitedSet.java` | `ferrite-world` / `ferrite-region` / `ferrite-profiler` | 📋 |
| 월드 설정(청크 로딩/충돌/엔티티 저장 한도) | `paper-server/src/main/java/io/papermc/paper/configuration/WorldConfiguration.java` | `ferrite-config` | 🔧 |

---

## 3. 조명

### 3.1 Starlight (원본)

- 저장소: <https://github.com/PaperMC/Starlight>
- 라이선스: `LICENSE` = **LGPL-3.0**
- 26.3: ❌ README에 "1.20 이후 업데이트 없음" 명시, 마지막 커밋 2024-03-08
- 포팅: 🟡 (기법/구조 참고; Ferrite는 이미 Starlight 방식 구현)

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| 레이어(청크 섹션) 단위 조명 전파, 경계 큐 지연 처리 | `common/light/StarLightEngine.java`, `StarLightInterface.java` | `ferrite-light` | ✅ |
| 하늘/블록 조명 분리 엔진 | `common/light/SkyStarLightEngine.java`, `BlockStarLightEngine.java` | `ferrite-light` | ✅ |
| 단일 기록 다중 판독 니블 배열(SWMR) | `common/light/SWMRNibbleArray.java` | `ferrite-light`(`NibbleArray`) | ✅ |
| 청크 저장 시 조명 데이터 직렬화 | `common/util/SaveUtil.java`, `mixin/common/world/ChunkSerializerMixin.java` | `ferrite-world::chunk_nbt` | ✅ |

### 3.2 ScalableLux — Starlight의 멀티스레드 포크

- 저장소: <https://github.com/RelativityMC/ScalableLux>
- 라이선스: `LICENSE` = **LGPL-3.0**
- 26.3: ✅ `ver/26.3.0` 브랜치 (기본 브랜치는 26.2)
- 포팅: 🟢 (LGPL-3.0)

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| 조명 업데이트의 스케줄링/스레드 풀 분리 | `ca/spottedleaf/starlight/common/thread/SchedulingUtil.java`, `GlobalExecutors.java`, `SimpleTask.java` | `ferrite-light` + `ferrite-region` | 📋 |
| 조명 청크 잠금(lock token)으로 동시 relight | `common/thread/LockTokenImpl.java`, `common/light/StarLightInterface.java` | `ferrite-light` | 📋 |
| 바닐라 `ThreadedLevelLightEngine` 대체 인터페이스 | `common/light/vanillainterface/ThreadedLevelLightEngineVanillaInterface.java`, `BaseLevelLightEngineVanillaInterface.java` | `ferrite-light` | 📋 |
| 청크 직렬화 시 조명 데이터 확장 | `common/world/ExtendedSerializableChunkData.java` | `ferrite-world` | 📋 |
| 설정(스레드 수/업데이트 배치) | `common/config/Config.java` | `ferrite.toml` | 📋 |

> ScalableLux는 "Starlight + 멀티스레드 조명"입니다. Ferrite의 현재 조명은
> **단일 스레드, 리전 스레드 내 실행**이며 바닐라 대조 불일치 0을 달성했습니다. 다음
> 단계는 ScalableLux식 **조명 전용 워커 + 청크 잠금**입니다.

### 3.3 Moonrise 내장 Starlight

Moonrise는 Starlight를 자체 청크 시스템에 맞게 내장합니다
(`patches/starlight/lightengine/ThreadedLevelLightEngineMixin.java`,
`common/misc/Delayed26WayDistancePropagator3D.java`). Ferrite는 Moonrise를 직접 쓰지 않고
같은 전파 알고리즘을 Rust로 구현했으므로, **청크 시스템과 조명의 결합 방식**(청크 상태
전이 시 조명 태스크 예약)만 참고합니다.

---

## 4. 메모리

### 4.1 FerriteCore

- 저장소: <https://github.com/malte0811/FerriteCore>
- 라이선스: `LICENSE` = **MIT**
- 26.3: ⚠️ 26.1 브랜치까지, 마지막 커밋 2026-03-24 (9.0.0)
- 포팅: 🟢

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| 블록 상태 property map을 압축된 fast map으로 | `fastmap/FastMap.java`, `CompactFastMapKey.java`, `BinaryFastMapKey.java`, `mixin/fastmap/StateDefinitionMixin.java` | `ferrite-data` | 📋 (u16 상태 ID 테이블은 ✅) |
| BlockStateCache(shape/collision/loot) 공유/압축 | `impl/BlockStateCacheImpl.java`, `mixin/blockstatecache/BlockStateCacheMixin.java` | `ferrite-world` | 🔧 (block_props) |
| 스레딩 감지기 메모리 축소 | `util/SmallThreadingDetector.java`, `mixin/threaddetec/PalettedContainerMixin.java` | `ferrite-world`(팔레트) | 📋 (Ferrite는 단일 기록자) |
| 데이터 컴포넌트 패치 맵 압축 | `mixin/datacomponents/PatchedDataComponentMapMixin.java` | `ferrite-item` | 📋 |
| VoxelShape 해시 캐시(배열/슬라이스) | `hash/ArrayVoxelShapeHash.java`, `SliceShapeHash.java`, `VoxelShapeHash.java` | `ferrite-world` / `ferrite-math` | 📋 |

### 4.2 ModernFix

- 저장소: <https://github.com/embeddedt/ModernFix>
- 라이선스: `LICENSE` = **LGPL-3.0-or-later**
- 26.3: ⚠️ `26.1` 브랜치까지 (기본 브랜치는 1.20.1, 2026-10-24)
- 포팅: 🟢 (LGPL-3.0-or-later)
- 참고: 상당수 기능이 **클라이언트/모델 로딩** 대상이라 서버에는 해당 없음. 서버에
  유효한 것만 아래에 정리.

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| BlockState 캐시 배열 공유 | `common/mixin/perf/cache_blockstate_cache_arrays/AbstractBlockStateCacheMixin.java` | `ferrite-world` | 📋 |
| 팔레트/비트 스토리지 압축 | `common/mixin/perf/compact_bit_storage/PalettedContainerMixin.java` | `ferrite-world` | 📋 |
| ImposterProtoChunk 압축 | `common/mixin/perf/compact_imposterprotochunks/*` | `ferrite-world` | 📋 |
| 월드젠 벤치마크 | `org/embeddedt/modernfix/benchmark/WorldgenBenchmark.java` | `ferrite-profiler` | 📋 |
| 청크 데드락/동시성 버그 수정 | `common/mixin/bugfix/chunk_deadlock/*`, `bugfix/concurrency/*` | `ferrite-region` | 📋 |
| 레지스트리 중복 제거 | `perf/compact_mojang_registries/*` | `ferrite-data` | 📋 |
| 시작 시간 단축(동적 DFU/리소스) | `perf/dynamic_dfu/*`, `perf/dynamic_resources/*` | — | ⛔ (서버 무관) |

---

## 5. 네트워크

### 5.1 Krypton

- 저장소: <https://github.com/astei/krypton>
- 라이선스: `LICENSE` = **LGPL-3.0**
- 26.3: ✅ `minecraft_version=26.3`, `mod_version=0.3.2` (2026-09-30)
- 포팅: 🟢

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| 네이티브 zlib 압축/해제(풀링 포함) | `mod/shared/network/compression/MinecraftCompressEncoder.java`, `MinecraftCompressDecoder.java` | `ferrite-protocol::frame` | ✅ (libdeflater) / 📋 (버퍼 풀) |
| AES-CFB8 네이티브(AES-NI) 경로 | `mod/shared/network/pipeline/MinecraftCipherEncoder.java`, `MinecraftCipherDecoder.java` | `ferrite-net::wire` | 🔧 (`aes` 크레이트 사용, 쓰기마다 할당) |
| flush 통합/자동 flush 제어 | `mod/shared/network/ConfigurableAutoFlush.java`, `MinecraftVarintPrepender.java` | `ferrite-net` | ✅ (틱당 1회 batch write) |
| VarInt 길이/인코딩 fast path | `mod/shared/network/util/VarIntUtil.java`, `mixin/shared/network/microopt/VarIntMixin.java` | `ferrite-protocol::codec` | 📋 |
| 프레임 디코더 최적화(길이 사전 검사) | `mixin/shared/network/pipeline/Varint21FrameDecoderMixin.java` | `ferrite-protocol::frame` | 📋 |
| UTF-8 문자열 인코딩 최적화 | `mixin/shared/network/microopt/Utf8StringMixin.java` | `ferrite-protocol::codec` | 📋 |
| 리소스 누수 검사 비활성(운영) | `mixin/shared/debugaid/ResourceLeakDetectorDisableConditionalMixin.java` | `ferrite-net` | 📋 |

### 5.2 VMP (Very Many Players)

- 저장소: <https://github.com/RelativityMC/VMP-fabric>
- 라이선스: `LICENSE` = **MIT**
- 26.3: ✅ `ver/26.3.0` 브랜치 (기본 브랜치는 26.1.1)
- 포팅: 🟢

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| 플레이어 주변 청크 감시 area map | `common/chunkwatching/AreaPlayerChunkWatchingManager.java`, `common/maps/AreaMap.java` | `ferrite-server`(청크 스트리밍) | 📋 |
| 엔티티 트래커 area map + 근접 엔티티 추적 | `common/playerwatching/NearbyEntityTracking.java`, `TACSExtension.java`, `EntityTrackerEntryExtension.java` | `ferrite-entity` / `ferrite-net` | 📋 |
| 로그인 시 초기 청크 비동기 로드 | `common/chunk/loading/async_chunks_on_player_login/AsyncChunkLoadUtil.java` | `ferrite-server` | 📋 |
| POI 비동기 프리로드 | `common/chunk/loading/IPOIAsyncPreload.java`, `mixins/chunk/loading/MixinPointOfInterestStorage.java` | `ferrite-world`(POI) | 📋 |
| 자체 청크 전송 + 패킷 단위 rate limit | `common/chunk/loading/*` (chunk sending) | `ferrite-net` | 📋 |
| 플레이어별 render distance 패킷 | `common/chunkwatching/PlayerClientVDTracking.java` | `ferrite-server` | 📋 |
| Netty 이벤트 루프 분리(봇 공격 완화) | `common/networking/eventloops/VMPEventLoops.java` | `ferrite-net` | 📋 |
| 유체/바이옴/스폰 밀도 캐시 | `common/entity/item/CachingWaterState.java`, `common/general/cache_ops/biome/PreloadingBiome.java`, `spawn_density_cap/SpawnDensityCapperDensityCapDelegate.java` | `ferrite-world` / `ferrite-entity` | 📋 |
| 객체 풀 | `common/util/SimpleObjectPool.java` | `ferrite-protocol` / `ferrite-net` | 📋 |
| 티켓 전파기(Paper `MCUtil` 기반, MIT 패치) | `mixins/ticketsystem/ticketpropagator/MixinChunkLevelManager.java` | `ferrite-region` | 📋 |

### 5.3 Velocity (프록시)

- 저장소: <https://github.com/PaperMC/Velocity>
- 라이선스: `LICENSE` = **GPL-3.0**
- 26.3: ✅ `ProtocolVersion.MINECRAFT_26_3(777, "26.3")` (2026-10-28)
- 포팅: 🟢
- 참고: Ferrite는 서버지만, **프록시 친화 네트워킹**과 압축/암호 구현을 참고합니다.
  Velocity modern forwarding은 Paper와 동일 형식으로 구현 예정입니다.

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| libdeflate/네이티브 zlib 압축기 | `com/velocitypowered/natives/compression/LibdeflateVelocityCompressor.java`, `NativeZlibDeflate.java`, `VelocityCompressorFactory.java` | `ferrite-protocol::frame` | ✅ |
| 네이티브 AES 암호 | `com/velocitypowered/natives/encryption/NativeVelocityCipher.java`, `JavaVelocityCipher.java` | `ferrite-net::wire` | 🔧 |
| Netty 이벤트 루프/전송 계층(epoll) | `proxy/src/main/java/com/velocitypowered/proxy/network/ConnectionManager.java`, `TransportType.java` | `ferrite-net` | 📋 (tokio 사용, epoll은 OS 기본) |
| 패킷 rate limiter | `proxy/src/main/java/com/velocitypowered/proxy/network/limiter/PacketLimiter.java`, `SimpleBytesPerSecondLimiter.java` | `ferrite-net` | 📋 |
| Modern forwarding(플레이어 정보 서명) | `proxy/.../connection/backend/` (Velocity forwarding) | `ferrite-net` | 📋 |

### 5.4 Paper 네트워킹 패치

Paper의 `0005-Optimize-Connection...`, `0008-Use-Velocity-compression-and-cipher-natives`,
`0026-Optimise-collision-checking-in-player-move-packet-handling`은 위 Krypton/Velocity와
같은 계열입니다. Ferrite에서는 **압축(libdeflate)과 틱 배치 flush는 이미 구현**, 나머지는
`ferrite-net`/`ferrite-protocol`에 순차 적용합니다.

---

## 6. 틱 루프 · 리전 스케줄링

### 6.1 Folia

- 저장소: <https://github.com/PaperMC/Folia>
- 라이선스: `PATCHES-LICENSE` = **GPL-3.0**
- 26.3: ✅ `apiVersion=26.3` (2026-10-29), `ver/26.3.x` 릴리스 워크플로
- 포팅: 🟢 (GPL-3.0). Ferrite의 `ferrite-region`은 Folia 방식의 **독자 Rust 구현**입니다.

| 기법 | 원본 구현 (Folia `minecraft-patches/features/*.patch`) | Ferrite 위치 | 상태 |
|---|---|---|---|
| 청크를 리전으로 묶고 리전별 독립 틱 루프 | `io.papermc.paper.threadedregions.ThreadedRegionizer`, `TickRegions`, `TickRegionScheduler` | `ferrite-region` | ✅ (regionizer v3) |
| 리전별 데이터/태스크 큐 | `RegionizedWorldData`, `RegionizedData`, `RegionizedTaskQueue` | `ferrite-region` | ✅ / 🔧 (deferred mutation 큐) |
| 리전 경계 안전 지점(teleport/엔티티 이동) | `RegionizedServer`, `TeleportUtils.teleport`, `TickRegions.TickRegionSectionData` | `ferrite-region` | ✅ |
| 엔티티/글로벌 스케줄러 | `EntityScheduler.java`, `scheduler/FoliaRegionScheduler.java`, `FoliaGlobalRegionScheduler.java` | `ferrite-region` | 🔧 |
| 리전별 스레드 로컬 난수 | `util/SimpleThreadLocalRandomSource.java` | `ferrite-math` | 📋 |
| 리전 프로파일러 | `features/0007-Region-profiler.patch` | `ferrite-profiler` | 📋 (크레이트 🔧) |
| 청크 시스템 처리량 카운터(TPS에 표시) | `features/0003-Add-chunk-system-throughput-counters-to-tps.patch` | `ferrite-profiler` | 📋 |
| 워치독 스레드 | `features/0008-Add-watchdog-thread.patch` | `ferrite-server` | 📋 |
| 논-오너 청크 블록 업데이트 차단 | `features/0004-Prevent-block-updates-in-non-loaded-or-non-owned-chu.patch` | `ferrite-region` / `ferrite-world` | 🔧 |

### 6.2 Gale / Leaf / Paper의 스케줄러·틱 관련

| 기법 | 원본 | Ferrite 위치 | 상태 |
|---|---|---|---|
| 가상 스레드 지원 | Gale `0145-Virtual-thread-support.patch`, Leaf `VirtualThreadSupport.java` | `ferrite-region` | 📋 |
| 틱 청크 목록 캐시(ServerCore) | `ChunkTickManager.java` | `ferrite-world` | 📋 |
| 틱 루프 패킷 처리량 제한(Moonrise) | `patches/tick_loop/PacketProcessorMixin.java` | `ferrite-net` | 📋 |
| EntityScheduler 틱 최적화(Paper) | `features/0016-Optimise-EntityScheduler-ticking.patch` | `ferrite-region` | 📋 |

---

## 7. 프로파일링 · 운영

### 7.1 spark

- 저장소: <https://github.com/lucko/spark>
- 라이선스: `LICENSE` = **GPL-3.0**
- 26.3: ✅ `spark-minecraft/build.gradle`의 `com.mojang:minecraft:26.3` (2026-10-24)
- 포팅: 🟢 (Java 프로파일러이므로 Rust로는 **개념만** 이식; `ferrite-profiler`가 대응)

| 기법 | 원본 구현 | Ferrite 위치 | 상태 |
|---|---|---|---|
| 비동기 샘플링 프로파일러(스택 트리) | `spark-common/.../common/sampler/Sampler.java`, `async/AsyncSampler.java`, `java/JavaSampler.java` | `ferrite-profiler::sampler` | 🔧 (구현됨, 틱 루프 미연결) |
| 틱 카운터(TPS/MSPT) | `spark-common/.../common/monitor/tick/TickMonitor.java`, `SparkTickStatistics.java`, `spark-bukkit/BukkitTickHook.java` | `ferrite-profiler::stats` / `tick` | 🔧 |
| 힙 덤프/GC 모니터링 | `spark-common/.../common/heapdump/HeapDump.java`, `common/monitor/memory/GarbageCollectorStatistics.java` | `ferrite-profiler` | 📋 |
| 명령/권한/플랫폼 추상화 | `common/command/`, `common/SparkPlatform.java` | `ferrite-command` / `ferrite-admin` | 📋 |

### 7.2 Pufferfish Flare

- `gg/pufferfish/pufferfish/flare/ProfilingManager.java`, `FlareCommand.java`,
  `collectors/TPSCollector.java` — spark와 유사한 경량 프로파일러. Ferrite는 spark 방식을
  우선하되, **0 오버헤드 틱 메트릭** 아이디어만 참고합니다.

---

## 8. Ferrite 적용 현황 (크레이트별)

| Ferrite 크레이트 | 이미 반영된 것 | 다음 작업 |
|---|---|---|
| `ferrite-protocol` | libdeflate 압축/해제, VarInt 코덱, 프레이밍 | VarInt fast path, 버퍼 풀, 오프-스레드 압축 |
| `ferrite-net` | TCP_NODELAY, 틱당 1회 batch write, AES-CFB8 | AES-NI 최적화(할당 제거), 압축 워커, 패킷 rate limit |
| `ferrite-light` | Starlight 방식 엔진(바닐라 불일치 0), SWMR 니블 배열 | ScalableLux식 조명 워커/청크 잠금 |
| `ferrite-world` | 팔레트 컨테이너(단일값/간접), Anvil IO, heightmap, 청크 NBT | 비동기 리전 IO, 청크 직렬화 최적화, 섹션 블록 카운트 |
| `ferrite-region` | Folia 스타일 regionizer v3, 리전별 큐/안전 지점 | 청크 시스템 태스크 스케줄러, 엔티티/글로벌 스케줄러 |
| `ferrite-data` | u16 블록 상태 테이블, 레지스트리 | FastMap식 property 조회, 상태 캐시 공유 |
| `ferrite-item` | 아이템 컴포넌트/와이어 코덱 | 데이터 컴포넌트 패치 압축 |
| `ferrite-profiler` | TPS/MSPT/샘플러/타이밍 | 틱 루프 연결, `/tps` `/mspt`, 리전 프로파일러 |
| `ferrite-nbt` | NBT/SNBT, 바이너리 IO | 스트링 풀, 바이트 배열 재사용 |
| `ferrite-math` | 벡터/수학 유틸 | xor-shift RNG, sin/cos 테이블, 방향 상수 |
| `ferrite-server` | 청크 스트리밍(prefetch), 플레이어 데이터, 월드 로드 | 호퍼/폭발/레드스톤 최적화, 증분 저장 |
| `ferrite-entity` (신설) | — | 충돌/트래커/AI/활성 범위/병렬 틱 |
| `ferrite-game` (신설, 현재 `ferrite-server::game`) | 블록 파괴/설치, 커맨드, 루팅 | 유체/폭발/레드스톤/호퍼 |

---

## 9. 검증 원칙

- **버킷(Bukkit/Paper) 서버 호환이 최우선입니다.** 어떤 최적화도 Bukkit/Paper API가 보장하는
  동작(이벤트 발생 순서와 취소 가능성, 스케줄러 의미, 인벤토리/엔티티 API 결과)을 바꾸면
  안 됩니다. 이벤트를 건너뛰는 최적화(예: 호퍼 이동 스킵)는 해당 이벤트에 등록된 리스너가
  없을 때만 적용합니다.
- 각 최적화는 **바닐라 대조 테스트**(같은 입력 → 같은 결과)를 통과해야 머지합니다.
  조명은 이미 256청크 불일치 0, 노이즈는 bit-exact로 검증 중입니다. 청크/틱/충돌 최적화도
  같은 방식의 differential test를 추가합니다.
- 바닐라 동작을 바꾸는 옵션은 **기본값 off**로 두고 `ferrite.toml`에서 켜게 합니다
  (Lithium의 일부 패치, Pufferfish의 DAB/activation-range, ServerCore의 `fast-biome-lookups`
  등). Paper 설정과 이름이 같은 옵션은 의미도 같게 유지합니다.
- 성능 수치는 `ferrite-server` 봇 부하 테스트(200봇/30초, MSPT·RSS)와 각 크레이트
  벤치마크로 기록합니다. 최적화 PR은 **전후 측정값**을 함께 남깁니다.
- 라이선스: 새로 참고한 프로젝트는 이 문서의 표에 **라이선스/버전/파일 경로**를 추가하고,
  코드를 옮긴 경우 `NOTICE.md`에 원저작자와 라이선스를 기록합니다. ARR 코드
  (`c2me-opts-accel-opencl` 등)는 참조하지 않습니다.

---

## 10. 참고 우선순위 (26.3 기준)

1. **Moonrise + C2ME (청크 시스템/IO)** — `ferrite-region`/`ferrite-world`의 다음 큰 단계.
   GPL/MIT라 직접 포팅 가능하고, 26.3 최신 코드가 있음.
2. **ScalableLux (조명 스레딩)** — 현재 조명 엔진을 그대로 두고 워커/잠금만 추가.
3. **Lithium (게임 로직)** — 엔티티 시스템(`ferrite-entity`)을 만들 때 처음부터 반영.
4. **Krypton + Velocity (네트워크)** — 압축/암호/프레이밍은 이미 일부 구현, 나머지는
   패킷 처리량 계측 후 적용.
5. **ServerCore + Leaf + Gale (운영 옵션/미세 패치)** — 설정 게이트와 함께 하나씩.
6. **FerriteCore/ModernFix** — 26.3 브랜치가 없으므로 구조만 참고(메모리 절감 효과는 큼).
7. **Paper/Folia** — 26.3이고 GPL-3.0이므로, 청크/스케줄러/네트워킹 패치를 직접 참고.
8. **Pufferfish/Starlight** — 26.x 코드가 없어 기법만 참고.
