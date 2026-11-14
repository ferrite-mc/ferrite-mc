# 플러그인 API 남은 스텁 인벤토리 (2026-11-09, fb-plugin)

이 문서는 **남아 있는 미지원 호출과 그 소유자**를 기록한다. 두 종류를 섞지 않는다.

1. **생성된 폴백 프로브** — `ferrite.native=false`(호스트 없음)에서 각 API를 기본 인자로 호출해
   `UnsupportedOperationException("Ferrite: not implemented: ...")`이 난 목록이다. 실제 서버에서는
   Rust 호스트가 답하는 메서드도 포함되므로, 이 목록만으로 "구현 안 됨"이라고 단정하지 않는다.
2. **코드에 남은 명시적 미지원 경로** — 구현 파일이 의도적으로 거부하는 메서드다. 소유자와 필요한
   선행 작업을 함께 적는다.

2026-11-08 fb-32 후속에서 `Advancement#getDisplay`, `PotionBrewer#getEffects`,
`Player#ban(..., Date/Instant/Duration, ...)`가 구현되어 이 표에서 제거됐다.

생성 파일은 `java/ferrite-api-impl/build/catalog/`에 있고 커밋하지 않는다. 갱신:

```sh
cd java/ferrite-api-impl
JAVA_HOME="C:/Program Files/Eclipse Adoptium/jdk-25.0.5.8-hotspot" ./gradlew test --no-daemon --console=plain
```

## 생성된 프로브 (2026-11-08 실행 결과)

| 파일 | 줄 수 | 의미 |
|---|---:|---|
| `build/catalog/stubs.txt` | 210 | 호스트 응답이 필요한 `Player`+`Server` 라우팅 스텁 (Player 210개, Server 0개가 `ServerStubs`에 남음) |
| `build/catalog/player-missing.txt` | 106 | 호스트 없이 호출했을 때 "not implemented"가 난 Player 메서드 |
| `build/catalog/server-missing.txt` | 5 | 같은 조건의 Server 메서드 |
| `build/catalog/world-missing.txt` | 61 | 같은 조건의 World 메서드 |
| `build/catalog/player-unoverridden.txt` | 210 | `PlayerStubs` 선언을 그대로 쓰는 라우팅 메서드 |
| `build/catalog/server-unoverridden.txt` | 0 | 모든 `ServerStubs` 메서드가 `FerriteServer`에서 오버라이드됨 |
| `build/catalog/world-unoverridden.txt` | 0 | `FerriteWorld`가 동적 프록시라 해당 없음 |

### 주의

`player-missing.txt`의 `getPing`, `getEntityId`, `getFirstPlayed`, `getLastPlayed`, `getLastSeen`,
`damage`, `kill`, `lookAt`, `stopSound`, `sendBlockChange`, `sendExperienceChange` 등은 **실제 호스트가
처리하는 경로가 이미 있다** (`crates/ferrite-server/src/plugins/facts.rs`, `network.rs`, `extra.rs`).
이 파일은 호스트 없는 JUnit 환경의 폴백 결과일 뿐이다. 실제 서버 기준의 남은 목록은 아래 2절과
`docs/plugin-stubs-status.md`의 "남은 스텁" 절을 따른다.

## 2026-11-09 fb-plugin — 드래그 이벤트와 Server 경계

`InventoryDragEvent`가 이제 실제로 발화한다. `crates/ferrite-server/src/game/container.rs`는
QuickCraft 드래그가 끝나는(status 2) 시점에 드래그 시작(status 0) 때 잡은 메뉴 스냅샷과 결과를
비교해 바뀐 슬롯을 모아 한 번 부르고, 리스너가 취소하면 시작 시점의 메뉴/인벤토리를 복원한 뒤
`container_set_content`로 전체 상태를 다시 보낸다. 슬롯 분배 계산은 `ferrite-inventory`의 기존
`quick_craft`가 계속 소유하며 `container.rs`는 다시 구현하지 않는다.
`ServerHooks::inventory_dragged`는 기본 구현 `Ok(true)`라 기존 구현체를 깨지 않는다. 호출은
플레이어를 소유한 리전 스레드에서만 일어나고 새 락이나 동기 대기를 추가하지 않는다.

### 이번 라운드에 경계가 확인되어 구현/검증된 것

| API | 구현 위치 | 경계 / 남은 제약 |
|---|---|---|
| `Server#getHelpMap` | `FerriteHelpMap` — 명령 맵의 모든 명령을 `GenericCommandHelpTopic`으로 만들고, `registerHelpTopicFactory`가 등록한 팩토리를 우선한다. `addTopic`/`clear`도 동작. | 플러그인 `plugin.yml`의 `help` 토픽과 `getIgnoredPlugins` 설정은 Bukkit `SimpleHelpMap` 로더가 없어 비어 있다. |
| `Server#getRecipesFor` | `FerriteRecipes` — 데이터팩의 바닐라 레시피를 결과 `Material`로 거른다. `recipeIterator`, `getRecipe`, `addRecipe`, `removeRecipe`, `clearRecipes`, `resetRecipes`도 같은 목록. | 레시피 매칭과 제작 결과 계산은 Rust `ferrite-recipe`가 소유한다. 플러그인이 추가한 레시피는 Bukkit 뷰에만 있고 제작 메뉴에는 쓰이지 않는다. |
| `Server#createMap`/`getMap` | `FerriteMapView` — 메모리 `MapView`, 렌더러 목록, `sendMap` 때 캔버스 렌더. | 맵 id/메타가 Rust 아이템으로 왕복하지 않는다(아이템은 material+amount). 숨김 커서와 스타일 캡션은 26.3 decoration 레코드에 없어 보내지 않는다. |
| `Server#getTags`/`getTag` | `FerriteTags` — 데이터팩 `tags/block|item|fluid|entity_type|game_event`. | 위 5개 레지스트리 밖은 `UnsupportedOperationException`. |
| `InventoryDragEvent` | `game/container.rs` + `ferrite-plugin` + `Bootstrap#fireInventoryDrag` | 드래그가 실제로 바꾼 슬롯만 담는다(시작 시점과 끝 시점의 메뉴 차이). 단일 슬롯 드래그는 `ferrite-inventory`가 일반 클릭으로 적용하지만, 결과가 바뀌면 그 슬롯을 담아 이벤트를 보낸다. |
| `Server#createWorld(WorldCreator)`, `Server#unloadWorld` | `FerriteServer` + 호스트 `createRuntimeWorld(String,String,long)` / `unloadRuntimeWorld(String,boolean)` (`plugins.rs`) + `game::World::create_runtime_world`/`unload_runtime_world` | 이미 등록된 이름·키는 그 월드를 돌려준다. 새 월드는 영구 디스크립터와 별도 청크 소스·리전 스케줄러(`WorldId::Runtime`)를 만들고 `WorldLoadEvent`를 보낸다. 시드는 생성 때만 적용되고 기존 월드는 디스크의 시드를 돌려준다. `unloadWorld`는 취소 가능한 `WorldUnloadEvent` 뒤에 호스트가 (`save` 인자에 따라 저장하고) 내리며, 기본 월드(`world`)는 내릴 수 없다. | `generator`가 있거나 `generateStructures(false)`이거나 `WorldType`이 `NORMAL`이 아니면 `UnsupportedOperationException`. 환경은 `NORMAL`/`NETHER`/`THE_END`만. 검증: JUnit `ApiSurfaceTest`(시드, 재열기, 거부 경로), Rust `game/runtime_persist.rs` 테스트(언로드 저장·실패·`save=false`). 실제 클라이언트로 월드 간 이동을 확인한 것은 아니다. |

### 런타임 월드와 플레이어 동작 (2026-11-11, W5)

| API | 소유자 | 동작 | 증명 테스트 |
|---|---|---|---|
| `Server#createWorld`/`unloadWorld`, 런타임 월드 생성·언로드·재오픈 | `game/runtime_persist.rs`, `game/dimension.rs` | 새 월드마다 청크 소스·Anvil 저장·스폰·리전 스케줄러를 만든다. 플레이어가 안에 있으면 언로드는 거부한다. | `plugin_lifecycle::runtime_world_gap7_create_edit_enter_leave_unload_reopen` |
| 런타임 월드에서 접속 종료 후 재접속 | `game/playerdata.rs`, `game/mod.rs` | 플레이어 파일이 `world`(런타임 월드 UUID)를 저장하고, 접속 시 그 월드가 로드돼 있으면 거기로 복원한다. 월드가 없으면 저장된 차원으로 대체한다. 접속 스냅샷과 위치 갱신도 그 월드를 보고한다. | `integration::player_rejoins_the_runtime_world_they_left_in`, `playerdata` 왕복 테스트 |
| `Player#breakBlock(Block)` | `game/interaction.rs::may_break`, `region.rs::plugin_break_block`, `plugins/playerops.rs` | 소유 리전이 플레이어 자신의 규칙으로 부순다: 공기는 `false`, 서바이벌의 베드락 등 파괴 불가(경도 < 0)는 `false`, 모험·관전은 `false`, 명령 블록 등은 OP 권한(2) 필요, `BlockBreakEvent`가 취소되면 `false`. 다른 월드의 블록은 `IllegalArgumentException`. 리전 답변을 2초까지 기다리고, 못 받으면 `false`. | `interaction::break_rules_follow_hardness_mode_and_permission`, `integration::plugin_break_block_follows_the_players_rules`, `playerops` 단위 테스트, `PlayerActionsTest` |
| `Player#chat("/명령")` | `plugins/playerops.rs`, 게임 루프 `ChatCommand` 경로 | 슬래시 명령을 채팅창에 친 것과 같은 경로로 플레이어 권한으로 실행한다. 콘솔 권한을 주지 않는다. 일반 채팅 줄은 기존대로 Java가 채팅 이벤트를 부른다. | `integration::plugin_chat_commands_keep_the_players_permission`, `PlayerActionsTest` |
| `Player#broadcastSlotBreak(EquipmentSlot)` | Java `PlayerMisc` | `getTrackedBy()` + 본인에게 2-인자 호출로 보낸다. | `PlayerActionsTest.oneArgumentSlotBreakIncludesThePlayerThemselves` |
| `Player#broadcastHurtAnimation(Collection)` | `plugins/broadcast.rs`, `PluginPacket::EntityHurt` | 목록의 각 시청자에게 이 플레이어의 엔티티 id로 피격 흔들림(yaw 0)을 보낸다. 목록을 다 읽지 못하면 아무에게도 보내지 않는다. | `broadcast::hurt_animation_reaches_each_listed_viewer_with_the_subjects_entity` |
| `Player#playPickupItemAnimation(Item,int)` | `plugins/network.rs`, `PluginPacket::PickupAnimation`, `facts::EntityFact::entity_id` | 호스트가 아는 아이템 엔티티의 네트워크 id로 `TakeItemEntity`를 플레이어 청크 시청자에게 보낸다. 모르는 아이템이나 0 이하 수량은 미구현 오류. | `network::pickup_animation_names_the_item_entity_and_a_positive_count` |

한계: `breakBlock`은 크리에이티브에서 검을 든 경우의 제한과 도구 내구도 소모, 월드 보더 검사를 하지 않는다.
`Player#performCommand`는 `Native.dispatchCommand`와 서버 명령 디스패처를 통해 구현되어 있다.

### 경계별 구현 상태와 남은 제한

| API | 소유자 | 왜 지금 구현할 수 없는가 | 필요한 선행 작업 |
|---|---|---|---|
| `EntityFactory#createEntitySnapshot` | `ferrite-server/src/plugins/snapshot.rs` + `FerriteEntitySnapshot` | 구현됨. 호스트가 SNBT를 파싱해 타입·위치·UUID·체력·속도·이름을 반환하고, 저장 NBT 스냅샷은 런타임 월드에서 재생성한다. | 기본 월드·네더·엔드도 `plugins::world_id`로 같은 NBT 게시 경로를 쓴다(2026-11-14). |
| `Structure#getEntities`, `Structure#place(..., EntityTransformer)` | `CapturedStructure` + `ferrite-server/src/plugins/snapshot.rs` | 런타임 월드에서 `ferriteEntities`로 비플레이어 엔티티 NBT를 캡처하고, 변환기 호출 뒤 `ferriteSpawnSnapshot`으로 재생성한다. | 세 차원의 목록은 `plugins::world_id`로 게시되며, 런타임 월드와 같은 `ferriteSpawnSnapshot` 경계를 쓴다. |

### 구조물 엔티티 경계 검증

> 2026-11-14 코드 대조: 런타임 월드와 기본 월드·네더·엔드의 비플레이어 엔티티는
> `facts::EntityFact`(NBT 포함, `World#ferriteEntities`)로 읽고 `ferriteSpawnSnapshot`으로
> 재생성한다. 세 차원은 `plugins::world_id`(기본 `default_world_id`, 네더 `2`, 엔드 `3`)로
> 게시되므로 구조물 엔티티 캡처/재생성이 차원에 관계없이 같은 경계를 쓴다.

1. **완료:** 호스트가 런타임 월드의 구조물 청크 범위에서 UUID·타입·위치·회전·NBT를 읽는다.
2. **완료:** `CapturedStructure`가 `fill(..., true)` 결과를 보관하고 구조물 API로 노출한다.
3. **완료:** `EntityTransformer`를 최종 위치에서 호출하고, 허용된 엔티티를 변환된 위치에
   저장 NBT로 재생성한다. 기본 월드·네더·엔드의 NBT도 같은 경로로 게시된다.

## `FerriteWorld`의 `UnsupportedOperationException` 16곳 (2026-11-12, W6, 기준 `b50f8d9`)

`FerriteWorld.java`에서 이 이름이 나오는 줄은 16곳이다. 던지는 곳(`throw`) 8줄(예외 9개)과 받아서 처리하는 곳(`catch`) 8줄이다.
`World`는 동적 프록시라 `setTime`/`setStorm`/`setDifficulty`/날씨 지속시간 같은 메서드는 `FerriteWorld`에 코드가 없고
`HostBridge.dispatch("World", …)`로 호스트(`plugins.rs`, `worldinfo.rs`)에 간다. 호스트가 없는 JUnit에서만 "not implemented"로 보인다.

던지는 곳 — 모두 "호스트가 그 위치의 데이터를 주지 않음"이라 데이터가 실제로 없을 때의 명시적 거부로 유지한다.

| 메서드 | 예외 | 판단 |
|---|---|---|
| `typeAt` (`Block#getType`, `getBlockAt` 등) | `no block data at …` | 호스트가 블록을 돌려주지 않는 위치(미로드 청크·호스트 없음). 공기를 지어내지 않는다. 유지. |
| `Block#getLightLevel` | `no light data at …` | 같은 이유. 유지. |
| `Block#getLightFromSky` | `no light data at …` | 같은 이유. 유지. |
| `getHighestBlockYAt(…, HeightMap)` | `no <HEIGHTMAP> height data` | 호스트가 저장된 높이 지도를 읽지 못한 열. 유지. |
| `getHighestBlockYAt` 그 밖의 인자 수 | `unsupported … overload` | 도달 불가: Bukkit에는 이 네 가지 오버로드(`(int,int)`, `(Location)`, `(int,int,HeightMap)`, `(Location,HeightMap)`) 말고 없다. |
| `getHighestBlockYAt(int,int)` | `no height data at …` | 호스트가 높이를 모르는 열. 유지. |
| `getBiome`/`getComputedBiome` | `no biome data at …` | 호스트가 바이옴을 모르는 위치. 유지. |
| `climate` (`getTemperature`/`getHumidity`) | `no biome data at …` | 같은 이유. 유지. |
| `findLightningTarget` | `no height data at …` | 높이를 모르면 대상 위치를 계산할 수 없다. 유지. |

받는 곳 — 호스트가 없거나 읽을 수 없을 때의 동작:

| 위치 | 동작 | 판단 |
|---|---|---|
| `setAutoSave` | 호스트가 없으면 값만 기억 | 유지(`isAutoSave`가 되돌려 준다). |
| `hostLong` (`getTime`, `getFullTime`, `getSeed`, `getMoonPhase`) | 호스트가 없으면 월드 등록 때의 스냅샷 | 유지. **`getMoonPhase`의 폴백은 `0`(항상 보름달)에서 "스냅샷 시간의 일수 mod 8"로 고쳤다.** |
| `hostValue` (`getGameTime`, `hasStorm`, `isThundering`, `isClearWeather`, 날씨 지속시간, `getDifficulty`) | **고침**: 예전에는 호스트가 없으면 `null`을 돌려줘 `boolean`/`int` 질의는 `NullPointerException`, `Difficulty`는 조용한 `null`이었다. 이제 `Ferrite: not implemented: World#hasStorm()`처럼 명시적으로 거부한다(`set*`로 넣은 값은 `HostBridge`가 기억해 되돌려 준다). |
| `setRule` (`setGameRule`, `setGameRuleValue`) | 호스트가 거부하거나 없으면 `false` | 유지. |
| `playersHere`, `hostEntities`, `findRod`, `playEffect`(시청자 순회) | 읽을 수 없는 항목은 건너뜀 | 유지. |

**이번에 고친 것:** `setSpawnLocation(int,int,int[,float])`과 `setSpawnLocation(Location)`을 호스트의 `setworldspawn`에 연결하고,
성공하면 `getSpawnLocation()`이 새 위치와 각도를 돌려준다(호스트에 스폰 getter가 없어 Java가 기억한다). 예전에는 `Location` 오버로드가 호스트가
모르는 서명으로 가서 거부됐고, 이동 뒤에도 `getSpawnLocation()`은 등록 때 위치였다.

**이미 호스트가 답하던 것(코드 변경 없이 테스트로 고정):** 월드 보더(`FerriteHostBorder` ↔ `border*()`/`borderSet`/`borderCenter` 등), 시간, 날씨, 난이도, 달 위상.
`FerriteWorldHostTest`가 호스트 서명 문자열과 되돌려 받는 값을 확인한다.

## 그 밖에 남은 명시적 미지원 경로와 소유자

| 영역 | 예시 (정확한 이름) | 소유자 / 선행 작업 |
|---|---|---|
| 다른 구현 인벤토리 | `Player#openInventory(Inventory)` (Ferrite 인벤토리 아님) | Java API (직렬화 불가) |
| 맵 캔버스 | `MapCanvas`의 기본 메서드 | Java `FerriteMapView` |
| 아이템 핸들 | `DroppedItem#<미지원 Item 메서드>` | Java 핸들 |
| HostBridge 반환 변환 | `HostBridge#call`의 미지원 반환 타입 | Java 브리지 |
| BlockData | `BlockData#<미지원 메서드>` (재질별) | Java `FerriteBlockData` |
| 청크 데이터 | `ChunkData#getBiome` | 좌표 없는 생성 버퍼는 바이옴을 알 수 없어 명시적으로 거부한다. `ChunkSnapshot#contains(Biome)`는 이미 호스트의 4×4×4 바이옴 셀을 조회한다. |
| 포션/인챈트/레지스트리 | `Enchantment` 속성, `RegistryAccess` 중 `GameRule`(`game_rule`)과 `MemoryKey`(`memory_module_type`)만 미지원 — 생성된 보고서에 값 타입·기본값이 없고 `MemoryKey` 생성자가 private이라 `UnsupportedOperationException("Ferrite: registry minecraft:... is not available: ...")`로 거부한다. 그 밖의 `RegistryKey` 레지스트리 41개는 생성 데이터로 `size`/`keyStream`/`stream`/`iterator`/`get`이 동작한다(내장 레지스트리는 `registries.json`, 데이터 기반 레지스트리 21개는 `data/minecraft/<이름>/*.json`, `Particle`/`PotionType`은 enum 상수를 키로 색인, `MusicInstrument`는 `instrument/*.json`의 `use_duration`·`range`·`sound_event`·`description`을 읽는다). 레지스트리 태그(`hasTag`/`getTag`/`getTags`)는 그대로이며 `Server#getTags`(`FerriteTags`)만 실제 데이터를 쓴다. Rust 호스트에는 레지스트리 호출이 없다. (`PotionEffectType`은 구현됨: 색·카테고리·attribute는 `FerritePotionEffectData` 표, `getId`는 생성된 `registries.json`의 protocol id, `getAttributeModifierAmount`는 바닐라 `amount*(amplifier+1)`, `isEnabledByFeature`는 26.3에서 required feature를 쓰는 효과가 없어 항상 true, `getDurationModifier`는 26.3 `MobEffect`에 해당 필드가 없어 1.0) | `ferrite-data` 데이터/호스트 |
| 루트 테이블 | `LootTable#populateLoot`, `fillInventory` | Java `FerriteLoot` — 생성 데이터팩의 `loot_table`/`predicate`/`tags`를 읽고 `FerriteItemStack`/`FerriteItemMeta`로 스택을 만든다. luck·bonus rolls·looting·killer/this_entity를 반영하고, `enchant_randomly`의 최대 레벨/호환성은 호스트의 `Enchantment` 응답을 쓴다(호스트가 없으면 1레벨/호환으로 폴백). 폭발 반경·블록 엔티티 데이터·미모델링 컴포넌트는 불투명 처리한다. |
| 구조물 | `StructureManager#<미지원>`, `Template#<미지원>`, `Structure#getStructureType` | 구조물 월드젠 호스트 |
| NMS/Unsafe | `UnsafeValues`의 NMS 의존 멤버, `CraftItemStack.asNMSCopy`, `CraftWorld#getHandle`, `CraftMagicNumbers` | 의도적 미지원 (가짜 구현 금지); 계속 `NoSuchMethodError` |
| 투사체/타일/장비/효과 | `Player#launchProjectile(Trident)` — 위력 2.5로 `launchThrown(TRIDENT)`를 호스트에 보내 리전이 소유자·속도·기본 삼지창 아이템을 가진 실제 삼지창 엔티티를 만들고 비행이 끝나면 아이템으로 떨어뜨림(눈덩이·달걀·진주·경험치 병·투척/잔류 물약·바람 구슬·화염구는 구현됨, 물약 내용물은 호출에 실을 수 없어 콜백의 `setItem`은 거부). `sendBlockUpdate`: 표지판 + 호스트가 모델링한 상자·함정 상자·통·셜커 상자·화로 계열은 해당 플레이어에게만 `ClientboundBlockEntityDataPacket`(Items, 화로는 타이머)으로 전송; 배너(`patterns`: `BannerBlockEntity.saveAdditional`)와 머리(`profile`: `ResolvableProfile$Partial`의 `name`/`id`)도 보내며(`tile_update.rs`, 단위 테스트 + JUnit `PlayerTileUpdateTest`), 머리의 프로필 속성(텍스처)·노트블록 소리·배너 이름 등 그 밖의 모델 없는 `TileState`와 메타데이터가 있는 아이템은 `UnsupportedOperationException`(월드는 바뀌지 않음). `Player#playEffect`는 데이터 종류별로 구현: Boolean(`COMPOSTER_FILL_ATTEMPT`·`VAULT_*` 1/0, `ENDER_DRAGON_BREATH` 1/-1), Integer·Number(그대로), `Color`(불투명 ARGB), `Axis`(ordinal), `BlockFace`(`WHITE_SMOKE_SHOOT`만, `Direction#get3DDataValue`), `BlockData`(전역 블록 상태 id, `block-states.tsv`). 거부: `RECORD_PLAY`의 `JukeboxSong`(데이터 기반 레지스트리 id가 생성 보고서에 없음), `SMOKE_SHOOT`·`DESTROY_PROGRESS*`(바닐라 서버 호출 지점을 못 찾음), `EXTINGUISH`·`TRIAL_SPAWNER_*`의 Boolean(고정값/인코딩 입자만 보여 매핑 미확인) — 원시 int 오버로드는 항상 가능. 장비: 플레이어의 슬롯 6개(HAND·OFF_HAND·FEET·LEGS·CHEST·HEAD)는 모두 연결, `BODY`·`SADDLE`은 바닐라 `Player#canUseSlot`이 false라 `UnsupportedOperationException`. | Rust 호스트/프로토콜 |
| 아이템 메타 | `PlayerInventory#setItem`이 지원하지 않는 메타, 미지원 장비 슬롯 | `ferrite-item` 컴포넌트 |
| 엔티티 조회 | `World#getEntities*`는 플레이어와 호스트가 게시한 비플레이어 엔티티를 합친다. | `FerriteWorld#hostEntities` → `facts::world_entities`; 제거·월드 필터 회귀 테스트가 있다. 기본 월드의 구조물 엔티티 NBT 경계는 별도 잔여 작업이다. |
| 월드젠 호스트 | `locateNearestBiome`, `locateNearestStructure`, `hasStructureAt`, `Registry.STRUCTURE` | vanilla 월드젠 호스트 (EULA 때문에 개발자 로컬만) |

## 스텁 카탈로그 분류 (2026-11-13 재생성, plugins-z)

`cd java/ferrite-api-impl && ./gradlew test --no-daemon --console=plain`이 `build/catalog/*.txt`를 다시 만든다.
이 목록은 `ferrite.native=false`에서 기본 인자로 호출해 "not implemented"가 난 메서드이므로, 아래 기준으로 분류했다.

- **호스트/Java가 답한다:** 같은 메서드 이름의 문자열 서명이 Rust 호스트(`crates/**`)에 있거나(`facts.rs`, `network.rs`, `extra.rs`, `worldgen.rs`),
  `FerriteWorld`가 `case "<이름>"`으로 Java에서 처리한다. 이름만으로 매칭했으므로 **인자 조합별 완전성은 검증하지 않았다.**
- **정말 열려 있다:** 위 두 곳 어디에도 처리 코드가 없다. 호출은 호스트에서 "not implemented"가 된다.

| 목록 | 줄 수 | 호스트/Java가 답함 | 정말 열려 있음 |
|---|---:|---:|---:|
| `player-missing.txt` | 102 | 90 | 12 |
| `server-missing.txt` | 4 | 4 (`reload`, `reloadData`, `reloadWhitelist`, `sendPluginMessage`) | 0 |
| `world-missing.txt` | 69 | 55 (Java `case` 또는 호스트) | 14 |

### 정말 열려 있는 Player 메서드 (12)

`canCriticalAttack(Entity)`, `copy()`, `copy(Location)`, `fireworkBoost(ItemStack)`, `getAsString()`, `getConnection()`,
`getSentChunkKeys()`, `loadData()`, `postEffects()`, `sleep(Location,boolean)`, `spawnAt(Location,SpawnReason)`,
`startRiptideAttack(int,float,ItemStack)`. 모두 `PlayerStubs`가 호스트로 보내고 Rust에 핸들러가 없다.
저렴하게 고칠 수 있는 것은 없었다(낙하 거리, 연결 객체, 저장 데이터 재적재, 침대 수면, 불꽃놀이 엔티티 등 호스트 상태가 필요).

### 정말 열려 있는 World 메서드 (14)

`addEntity(Entity)`, `createEntity(Location,Class)`, `getBlockState(int,int,int)`, `getFluidData(int,int,int)`,
`getChunksAtAsync(...)`, `getTileEntityCount()`, `getTickableTileEntityCount()`, `getWorldPath()`, `isVoidDamageEnabled()`,
`locateNearestPoi(...)`, `locateAllPoiInRange(...)`, `spawn(Location,Class,...)`(세 오버로드), `vanillaBiomeProvider()`.
`getBlockState(x,y,z)`는 Paper 기본 구현이 `getBlockAt(x,y,z).getState()`라 쉬워 보이지만, `getState()`가 호스트를 필요로 해
호스트 없는 JUnit으로 검증할 수 없어 이번에는 고치지 않았다(검증 없는 코드는 넣지 않음).

`Entity` 파생 API(`copy`, `getAsString`)는 월드/플레이어가 아닌 엔티티 NBT 경계가 필요하다.

### 주의

- 위 숫자는 이 환경에서 재생성한 프로브의 결과이며, "답한다"로 분류된 항목이 모든 인자 조합에서 동작한다는 뜻이 아니다.
- NMS 계열(`CraftItemStack.asNMSCopy`, `CraftWorld#getHandle`, `CraftMagicNumbers`)은 이 목록에 없고 의도적으로 `NoSuchMethodError`다.
