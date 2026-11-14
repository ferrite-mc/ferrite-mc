# 플러그인 API 잔여 작업 전체 구현 지시서

> 대상: Claude 구현 담당자  
> 목표: 아래 목록의 미완료·부분 완료·명시적 거부 경로를 하나도 남기지 않고 실제 서버 상태,
> 패킷, 저장, 이벤트, 리전 소유권까지 연결한다. 단순히 Java 메서드가 예외 없이 반환하는
> 폴백을 추가하지 말고, Bukkit/Paper가 관찰하는 결과가 실제 게임 상태와 일치해야 한다.
>
> 기준 문서: `docs/plugin-api-stubs.md`, `docs/plugin-stubs-status.md`,
> `docs/plugin-api-audit.md`, `README.md`, `docs/BACKLOG.md`.

## 0. 코드 대조 결과 — “없음”과 “껍데기/부분 구현”을 구분

2026-11-13 현재 소스와 대조했다. 아래 항목은 클래스나 메서드가 전혀 없는 것이
아니라 **서명은 존재하지만 실제 NMS 상태·메타데이터·리전 상태까지 연결되지 않은
부분 구현**이다.

| 코드 위치 | 현재 코드에 있는 것 | 실제로 남은 일 |
|---|---|---|
| `org/bukkit/craftbukkit/inventory/CraftItemStack.java` | `asNMSCopy`, `asBukkitCopy`, `asCraftMirror`가 존재하고 `ItemStack.of/toBukkit`를 호출 | `net.minecraft.world.item.ItemStack`의 변환이 material/amount 중심이다. enchantment, lore, component, PDC, profile, map, potion 등 전체 메타데이터를 양방향 보존해야 한다. |
| `org/bukkit/craftbukkit/CraftWorld.java` | `getHandle()`이 `ServerLevel` 래퍼를 반환 | `ServerLevel`이 월드 객체의 단순 조회 래퍼다. NMS 호출이 실제 청크·엔티티·월드 저장·리전 명령으로 이어지는 안전한 핸들 API로 확장되어야 한다. |
| `dev/ferrite/server/FerriteUnsafe.java` | registry/version/advancement 일부와 여러 HostBridge 호출이 존재 | `modifyItemStack`, JSON/NBT item·entity 직렬화, legacy 변환, entity serialization을 실제 Rust 데이터와 완성하고, 나머지 의도적 거부를 정확한 대체 경로로 바꿔야 한다. |
| `dev/ferrite/server/FerriteWorld.java` | 청크 load/unload, generation state, tree, structure locate 일부가 존재 | 현재 즉시 완료 future, 로컬 상태, 제한된 overload, `NONE`/`null`, 일부 데이터만 지원하는 경로를 실제 비동기·생성·저장·이벤트 동작으로 확장해야 한다. |
| `dev/ferrite/server/PlayerStubs.java` | Player 메서드가 HostBridge 라우트로 선언됨 | `PlayerStubs` 선언만으로 완료로 보지 않는다. `crates/ferrite-server/src/plugins`에 실제 라우트와 게임 상태/패킷/저장 구현이 있어야 한다. |
| `ServerGamePacketListenerImpl.java` | NMS 컨테이너의 기본 검증과 일부 메뉴 전달이 존재 | `rows == 0` 메뉴, 모든 NMS menu type, transaction/state id, 이벤트·슬롯 동기화를 완성해야 한다. |

따라서 아래 지시서의 “구현”은 메서드 선언 추가가 아니라 위 부분 구현을 실제
동작으로 완성하는 의미다. 특히 `CraftItemStack`, `CraftWorld`, `FerriteUnsafe`,
`FerriteWorld`를 이미 없다고 가정하고 중복 클래스를 만들지 않는다.

## 1. 반드시 지킬 구현 원칙

1. 구현 전 Java API 서명, Rust 호스트 라우트, 게임 루프 소유 리전, 패킷, 저장 경로를
   각각 확인한다.
2. `null`, 빈 컬렉션, 성공처럼 보이는 no-op으로 미구현을 숨기지 않는다. 정말 지원할 수
   없으면 먼저 상태 모델과 호스트 라우트를 추가한다.
3. 플레이어·엔티티·월드 변경은 해당 리전 소유 스레드에서 실행한다. 다른 리전은 기존
   메시지/명령 경로로 연결하고 동기 대기로 교착을 만들지 않는다.
4. 모든 codec/호스트 라우트/API 동작에는 회귀 테스트를 추가한다. Java 단위 테스트,
   Rust 단위 테스트, 가능한 경우 실제 JVM 플러그인 통합 테스트를 함께 추가한다.
5. 실제 Minecraft 26.3 클라이언트에서 패킷과 화면을 검증한다. 테스트가 끝나면 이 문서와
   README의 상태를 코드 기준으로 갱신한다.

## 2. NMS 내부 연결 작업 — 정확한 미완료 항목

여기서 NMS는 한 덩어리로 처리하지 말고 다음 각각을 구현한다.

### 2.1 `CraftItemStack.asNMSCopy` / `asBukkitCopy`

- **현재 서명과 변환 호출은 이미 존재한다.** 아래는 그 구현을 완성하는 작업이다.
- Bukkit `ItemStack`의 material, amount, damage, enchantments, display name/lore,
  custom model data, item flags, attribute modifiers, potion data, book data,
  banner/pattern, firework, map data, skull profile, recipe remainder, PDC를
  서버 내부 아이템 모델로 변환한다.
- 반대 방향 변환도 구현해 Bukkit 플러그인이 수정한 메타가 실제 슬롯·드롭·컨테이너·
  저장 NBT에 반영되게 한다.
- 현재 Rust 아이템이 `material + amount`만 왕복하는 경로를 확장하고, 지원하지 않는
  메타는 버리지 말고 명시적으로 타입별 오류를 낸다.
- 인벤토리 클릭, 퀵무브, 드래그, 드롭, 제작, 컨테이너 저장/복원, 플레이어 재접속에
  round-trip 테스트를 추가한다.

### 2.2 `CraftWorld#getHandle`

- **현재 `CraftWorld#getHandle()`과 `ServerLevel` 래퍼는 이미 존재한다.**
  이를 실제 안전한 브리지로 확장한다.
- 핸들은 월드 UUID/키/환경/시드/시간/날씨/난이도/보더/스폰 위치와 청크·엔티티·
  블록·블록 엔티티·스케줄러 조회를 실제 월드 상태에 연결해야 한다.
- 기본 월드, 네더, 엔드, 런타임 월드를 구분하고 잘못된 월드 핸들로 타 월드 상태를
  읽거나 쓰지 못하게 한다.
- 핸들에서 호출되는 NMS 경로도 리전 소유권을 지키며, 직접 Java 객체를 노출해 데이터
  레이스를 만들지 않는다.

### 2.3 `CraftMagicNumbers`

- 현재 저장소의 `CraftMagicNumbers` 클래스/메서드 존재 여부를 먼저 확인하고,
  없는 서명만 추가한다. 이미 Bukkit 레지스트리로 처리되는 변환을 중복 구현하지 않는다.
- Bukkit `Material`/`BlockData`/아이템과 내부 블록·아이템 레지스트리 사이 변환을
  구현한다.
- material id, namespaced key, block state, item stack, default state, legacy 변환
  호출을 26.3 레지스트리와 일치시킨다.
- 존재하지 않거나 현재 버전에 없는 값은 조용히 공기/빈 아이템으로 바꾸지 말고
  명시적 오류를 낸다.

### 2.4 NMS 메뉴·패킷 경로

- `ServerGamePacketListenerImpl`의 현재 NMS 컨테이너 지원을 모든 메뉴 타입으로
  확장한다. 현재 `rows == 0`인 미지원 메뉴, 잘못된 slot/item 수, 닫힌 view 처리를
  실제 메뉴 수명주기와 맞춘다.
- 플레이어가 NMS 방식으로 여는 화면도 Bukkit `InventoryOpenEvent`, 클릭/드래그,
  `InventoryCloseEvent`와 동일하게 동작해야 한다.
- 슬롯 동기화, carried item, transaction/state id, 창 닫기, 서버 재동기화를 클라이언트로
  검증한다.

### 2.5 NMS 엔티티·데이터·월드 연결

- NMS 엔티티 핸들에서 위치, 회전, 속도, UUID, 승객/차량, 장비, 속성, 메타데이터,
  제거/스폰 상태를 실제 엔티티 상태와 연결한다.
- `CraftEntity`/`CraftPlayer`가 내부 엔티티를 직접 노출하지 않고 안전한 스냅샷 또는
  소유 리전 명령을 사용한다.
- 엔티티 저장 NBT는 런타임 월드뿐 아니라 기본 월드·네더·엔드에서도 안전한 저장 경계를
  제공해 구조물 캡처/배치가 동일하게 작동하게 한다.

### 2.6 NMS/Unsafe가 필요한 API의 명시적 처리

- `FerriteUnsafe`에 이미 있는 registry/version/advancement/serialization 경로를
  먼저 실제 Rust 데이터와 대조하고, 메모리·월드·청크·entity 접근은 실제 안전한
  Ferrite 브리지로 대체한다. JVM 메모리 주소를 반환하는 가짜 구현은 금지한다.
- `CraftItemStack.asNMSCopy`, `CraftWorld#getHandle`, `CraftMagicNumbers` 외에도
  NMS 타입을 인자로 받거나 반환하는 모든 API를 catalog에서 찾아 처리한다.
- `NoSuchMethodError`를 없애는 것이 목표가 아니라, 실제 동작을 구현할 수 없는 서명은
  왜 불가능한지 테스트로 고정하고 대체 Bukkit 경로를 제공한다.

## 3. 남은 Player API

다음 서명을 실제 서버 동작까지 구현한다.

- 리소스 팩: `addResourcePack(UUID,String,byte[],String,boolean)`,
  `setResourcePack(UUID,String,byte[],Component,boolean)`,
  `setResourcePack(UUID,String,byte[],String,boolean)`. 요청 id, hash, prompt,
  required, 상태 이벤트와 클라이언트 응답을 연결한다.
- 엔티티/전투: `canCriticalAttack(Entity)`, `copy()`, `copy(Location)`,
  `fireworkBoost(ItemStack)`, `sleep(Location,boolean)`, `spawnAt(Location,SpawnReason)`,
  `startRiptideAttack(int,float,ItemStack)`.
- 연결/상태: `getAsString()`, `getConnection()`, `getSentChunkKeys()`, `loadData()`,
  `postEffects()`, `getVirtualHost()`, `getHAProxyAddress()`, `getMemory(MemoryKey)`,
  `getData(DataComponentType.Valued)`, `getPickItemStack()`.
- 네트워크/시각: `spawnParticle(... 모든 overload ...)`, `sendBlockUpdate`의 모든
  블록 엔티티 타입과 item metadata, `EntityEquipment`의 남은 메서드.
- 투사체: 삼지창 발사와 `Player#launchProjectile`의 모든 지원 투사체를 엔티티 생성,
  중력/충돌/피해/픽업/저장/클라이언트 패킷까지 연결한다.
- 수면/리스폰/데이터: 침대 위치 검증, 수면 이벤트, 강제 기상, 리스폰 월드·침대
  저장/복원, 플레이어 데이터 load/save를 실제 playerdata와 연결한다.

## 4. 남은 World API

다음 메서드를 단순 예외 제거가 아닌 실제 월드·리전·패킷 동작으로 구현한다.

- 엔티티: `addEntity(Entity)`, `createEntity(Location,Class)`,
  `spawn(Location,Class,...)` 모든 overload, `createExplosion(...)` 모든 overload.
  폭발 블록 피해, 엔티티 피해, 화염, 이벤트 취소, 사운드/파티클, 다른 리전 라우팅을
  구현한다.
- 투사체/번개: `findLightningRod(Location)`, `findLightningTarget(Location)`.
- 나무/월드젠: `generateTree(...)` 모든 overload, `vanillaBiomeProvider()`,
  커스텀 generator와 `generateStructures(false)` 옵션. 생성 결과를 실제 블록 변경,
  이벤트, 저장에 반영한다.
- 블록/청크: `getBlockState`, `getFluidData`, `getChunkCount`,
  `getChunkGenerationState`, `getChunkGenerationStateAsync`, `getLoadedChunks`,
  `getTickableTileEntityCount`, `getTileEntityCount`, `getWorldPath`,
  `isChunkInUse`, `loadChunk`/`unloadChunk`/`unloadChunkRequest` 모든 overload.
  플레이어·forceload·플러그인 티켓과 충돌하지 않는 청크 수명주기를 구현한다.
- 비동기 청크: `getChunksAtAsync(...)`를 callback 실행 리전과 예외 전달까지 포함해
  구현한다.
- 구조물/POI: `getStructures`, `hasStructureAt`, `locateAllPoiInRange`,
  `locateNearestPoi`, `locateNearestStructure` 모든 overload. 구조물·POI 레지스트리,
  점유 상태, 차원 경계를 실제 데이터와 연결한다.
- 바이옴: `locateNearestBiome`, `setBiome`. 팔레트/저장/클라이언트 바이옴 패킷과
  월드젠 후속 동작을 구현한다.
- 규칙/환경: `getGameRuleValue(GameRule)`, `isVoidDamageEnabled`를 실제
  `WorldInfo`/게임 루프와 연결한다.
- 월드 생성 옵션: `NORMAL`, `NETHER`, `THE_END` 외 환경의 지원 여부를 결정하고,
  지원하지 않는 환경은 API 문서·테스트에 명시한다. `WorldCreator#generator`,
  seed, type, structures, biome provider의 조합을 저장·재오픈까지 검증한다.

## 5. 아이템·블록 엔티티·메타데이터

- `Player#sendBlockUpdate`를 표지판, 배너, 머리, 상자, 통, 셜커, 화로뿐 아니라
  주크박스, 모루, 베드, 벌집, 스컬크 센서, 구조물 블록, 명령 블록, 스포너 등
  26.3 블록 엔티티에 대해 구현한다.
- 머리 프로필의 properties/textures, 표지판 필터/색/글로우, 아이템 슬롯 메타를
  패킷과 저장 NBT에 모두 보존한다.
- 지원하지 않는 PDC 타입을 전부 Bukkit/Paper 허용 타입과 대조해 구현하고, 중첩
  컨테이너·리스트·배열·네임스페이스 키를 round-trip한다.
- `ItemMeta`, `DataComponentType`, `MemoryKey`, `GameRule` 값 객체를 생성 데이터와
  기본값까지 포함해 완성한다.

## 6. 지도·구조물·엔티티

- `MapCanvas` 기본 메서드, 픽셀/색상/월드/스케일, 커서 표시 여부, 스타일,
  번역/캡션을 실제 map item과 map packet에 연결한다.
- 숨김 커서와 styled caption을 26.3 decoration 레코드에 맞게 구현한다.
- 구조물 엔티티 캡처/배치를 기본 월드·네더·엔드에도 제공한다. `EntityTransformer`
  목록이 비어 있지 않아도 거부하지 않고, 변환 후 위치·회전·UUID 정책을 적용한다.
- 엔티티 종류별 metadata, attributes, passengers/vehicles, shoulder parrot,
  fishing bobber, area-effect-cloud를 클라이언트 패킷과 저장/복원까지 구현한다.

## 7. Paper/Bukkit 생명주기와 스케줄러

- `LifecycleEvents` 전체를 구현한다. 현재 동작하는 `COMMANDS` 외 bootstrap-only
  이벤트도 호스트 초기화 순서와 plugin bootstrap을 추가해 연결한다.
- plugin.yml의 help 토픽과 ignored plugins를 `HelpMap`에 로드한다.
- Bukkit 레시피 추가/삭제가 실제 제작 메뉴와 `ferrite-recipe` 매칭에 반영되게 한다.
- async scheduler, region/entity scheduler, 취소·플러그인 disable·서버 종료 시 task
  정리를 Paper 규칙과 맞춘다.
- Paper 전용 command registration, event priority, plugin lifecycle, reload/disable
  경계를 실제 플러그인 통합 테스트로 검증한다.

## 8. 게임 상호작용과 클라이언트 검증

- 유체 경계, redstone, unloaded chunk flow, neighbour update 순서,
  placement rules, crop growth, gravity block chain을 vanilla 동작과 대조한다.
- enchanting/anvil의 모든 edge case와 shulker/map/cartography 결과를 검증한다.
- 삼지창 외 모든 투사체, 낚싯대 bobber, 물약 구름, 몹 영향 범위를 구현한다.
- 실제 클라이언트에서 메뉴, 맵, 보스바, 파티클, 엔티티 메타데이터, 구조물,
  월드 이동, 리소스팩, NMS 메뉴를 확인하고 수동 체크리스트를 갱신한다.

## 9. 완료 조건

- 이 문서의 각 항목에 구현 파일과 테스트를 기록한다.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` 통과.
- `java/ferrite-api-impl`의 전체 Gradle 테스트와 plugin lifecycle 통합 테스트 통과.
- 호스트 없는 catalog의 `not implemented` 결과와 실제 서버의 지원/거부 목록을 분리해
  재생성한다. 실제 서버 경로에 남은 의도적 거부는 메서드명·이유·대체 API를 표로 쓴다.
- README에는 “NMS 미지원” 같은 포괄 문구를 남기지 말고, 위의 정확한 클래스·메서드와
  구현 상태를 반영한다.

## 10. 구현 현황 (2026-11-14, 브랜치 `yxshadxw/plugin-new`)

이 절은 위 지시서를 코드와 대조해 **끝낸 것**, **원래 구현돼 있던 것(문서가 낡음)**,
**아직 남은 것**을 가른다. 아래 "끝냄"의 각 줄은 같은 커밋에 테스트가 있다.

### 10.1 이번에 끝낸 것

| 항목 | 구현 | 테스트 |
|---|---|---|
| `World#getChunksAtAsync(int,int,int,int,boolean,Runnable)` | `FerriteWorld` (`getChunkAtAsync`와 같은 규칙: 청크 핸들은 뷰이므로 콜백은 즉시, 역순 범위는 `IllegalArgumentException`) | `FerriteChunkDataTest#chunkRectangleAsync...` |
| `Player#canCriticalAttack(Entity)` | `PlayerMisc` — 바닐라 `Player.attack`의 치명타 조건(쿨다운 > 0.9, 낙하, 공중, 사다리·물·실명·탑승·달리기 아님, 생명체 대상) | `PlayerBreadthTest#aCriticalAttack...` |
| `Player#getSentChunkKeys()` | `PlayerMisc` — `getSentChunks()`를 `Chunk#getChunkKey`로 변환 | `PlayerBreadthTest#sentChunkKeys...` |
| `Player#startRiptideAttack(int,float,ItemStack)` | Java `PlayerMisc` → `PlayerCommand::RiptideAttack`(`ferrite-plugin`) → `plugins/inventory.rs` → `game/mod.rs` → 리전 `Command::RiptideAttack` → `Region::spin_attacks`/`spin_victims`(회전 중 상자 주변 몹에 한 번씩 피해, 킬 크레딧·보복·피격 애니메이션) | Java `PlayerBreadthTest#aRiptideAttack...`, Rust `inventory::tests::riptide_attack_calls...`, `region::tests::a_riptide_spin_picks_each_mob_in_reach_once` |
| `Player#sleep(Location,boolean)` | Java `PlayerMisc` → `PlayerCommand::Sleep`(티켓으로 리전 판정을 기다림, `breakBlock`과 같은 방식) → `blocks::plugin_sleep`(침대가 아니거나, 점유 중이거나, 이미 자는 중이거나, 침대가 터지는 차원에서 `force=false`이면 `false`) | Java `PlayerBreadthTest#sleepSends...`, Rust `playerops::tests::sleep_waits...`, `blocks::tests::bed_halves...` |
| `sendBlockUpdate`: 디스펜서·드롭퍼·호퍼·양조기·크래프터·모닥불·장식된 책장 | `plugins/tile_update.rs` `FAMILIES` + `PlayerMisc#sendBlockUpdate` | Rust `tile_update::tests::the_item_container_families...`, Java `PlayerTileUpdateTest` |
| `sendBlockUpdate`: 머리 프로필 properties(텍스처) | `property,<이름>,<값>,<서명>` 항목 → `ResolvableProfile.properties` (개수·길이 상한은 바닐라 값) | Rust `a_skull_carries_its_texture_properties...`, Java `aSkullSendsItsProfileTextures...` |
| `World#spawn`(모든 overload), `createEntity`, `addEntity` | `FerriteWorld` — 클래스 → `EntityType`(정확히 한 타입이 그 클래스일 때만, `Monster`처럼 여럿이 공유하면 `IllegalArgumentException`), 기존 `spawnEntity` 호스트 라우트 재사용. `createEntity`가 만든 핸들만 `addEntity`로 넣을 수 있다. consumer는 생성 뒤 라이브 핸들에 불린다(Bukkit은 직전). 스폰 사유·`randomizeData`는 효과가 없고 `CreatureSpawnEvent`는 쏘지 않는다 | `FerriteWorldHostTest#spawnMaps...`, `#createEntityKeeps...` |
| `World#createExplosion`(모든 overload) | Java가 `(Location, power, fire, breakBlocks)`로 정규화(출처 엔티티는 위치가 없을 때 중심만 정한다) → `PlayerCommand::Explosion{world,..}` → 기본 월드 또는 런타임 월드의 소유 리전. 다른 월드의 위치는 `IllegalArgumentException` | Java `#everyExplosionOverload...`, Rust `blasts::tests::a_runtime_world_gets_its_own_explosion` |
| `Server#addRecipe`/`removeRecipe`/`clearRecipes`/`resetRecipes` | `FerriteRecipes`가 `ShapedRecipe`/`ShapelessRecipe`(`MaterialChoice`만)를 바닐라 레시피 JSON으로 호스트에 보냄 → `game/crafting.rs` 오버레이(바닐라 먼저, 제거된 id는 건너뜀, 그다음 플러그인 레시피) → `ferrite-recipe::match_crafting_in`. `ExactChoice`와 메타가 있는 결과는 `UnsupportedOperationException`. 제작 메뉴 결과에 반영되며, 클라이언트 레시피북·자동 채우기에는 나오지 않는다 | Rust `crafting::tests::a_plugin_recipe_crafts_until_it_is_removed`·`removing_a_vanilla_recipe...`·`clearing_hides...`, `plugins::craft::tests::adding_and_removing...`, `ferrite-recipe` `runtime_recipes_match...`, Java `PluginRecipeHostTest` |
| `sendBlockUpdate`: 스냅샷 아이템의 `ItemMeta` | `ItemMetaWire`(인벤토리 슬롯과 같은 직렬화) → `meta,<슬롯>,<hex>` → `with_meta` | Rust `a_stack_in_the_snapshot_can_carry_metadata`, Java `aStackWithMetadataSends...` |

### 10.2 문서가 낡았던 것 (코드에는 이미 있음)

- 배너 `patterns`와 머리 profile name/uuid는 이미 `sendBlockUpdate`로 간다.
- PDC의 컨테이너·컨테이너 배열·중첩 리스트 타입은 `FerriteDataContainer`가 구현하고
  `ItemPdcNestingTest`가 검증한다. 지원하지 않는 PDC 타입은 예외로 거부한다.
- `Player#launchProjectile`은 삼지창·눈덩이·달걀·진주·경험치 병·물약·바람 구슬·화염구를 지원한다.
- `World#getChunkGenerationState(Async)`, `unloadChunkRequest`, `isChunkInUse`, `getLoadedChunks`,
  `locateNearestBiome`/`Structure`, `hasStructureAt`, `getStructures`는 `FerriteWorld`가 처리한다.
  호스트 없는 카탈로그(`build/catalog/*-missing.txt`)에 나온다는 이유만으로 미구현이 아니다.
- 11-13 `commands/plugins` 후속(lane `blocked-x/y/z`, `plugins-z`)은 PR #24·#25로 통합돼 있고,
  PR #25 이후 브랜치에만 남은 커밋은 테스트·문서 3개뿐이어서 이 브랜치에 체리픽했다(`blocked-y`와 `blocked-z`는 남은 것이 없다).

`World#setVoidDamageEnabled/Amount/MinBuildHeightOffset`은 이제 호스트(`facts::VoidDamage`)를 거쳐 게임 루프의 void 피해에 반영된다
(바닐라 차원은 모두 기본 월드 설정을 공유한다).

### 10.3 아직 남은 것 (코드 근거가 있는 목록)

호스트 라우트도 Java 케이스도 없는 서명만 적었다(`World`는 카탈로그와 `crates/**`·Java `case`를 대조).

- **Player:** `fireworkBoost`(폭죽 엔티티 모델 없음), `loadData`(저장된 플레이어 데이터를 살아 있는 리전 상태에 다시 적용하는 경로 없음).
  `getConnection`(플레이어 메서드로 전달하는 프록시; `reenterConfiguration`·`sendReportDetails`는 `UnsupportedOperationException`)과
  `postEffects`(`/posteffect`를 콘솔로 실행하고 목록은 Java에 미러링 — 명령으로 바꾼 상태는 `values()`에 보이지 않는다)는 구현했다.
- **World:** `locateNearestPoi`/`locateAllPoiInRange`는 POI 저장소가 없어 로드된 청크를 스캔한다(`Occupancy.ANY`만 지원,
  호스트가 블록 속성을 주지 않으면 침대 머리/발 같은 구분이 안 된다). `getTileEntityCount`는 로드된 청크의 블록 엔티티 수,
  `getTickableTileEntityCount`는 바닐라에서 틱 처리를 하는 타입 목록(`TICKING_BLOCK_ENTITIES`) 기준이다.
  `getWorldPath`는 기본 월드만 된다. `getFluidData`, `spawnAt`, `vanillaBiomeProvider`는 컴파일 대상 Paper API(26.3 build 227)에
  존재하지 않아 구현 대상이 아니다.
- **블록 엔티티:** 주크박스·교탁·명령 블록·스포너·벌집·스컬크 센서·구조물 블록·침대. 26.3의 NBT 키 이름을
  확인할 수 있는 자료(서버 코드)가 로컬에 없어서 추측으로 구현하지 않았다. `UnsupportedOperationException`을 유지한다.
- **레시피(§7):** 조리·대장간·석재 절단 레시피는 런타임에 추가할 수 없다(Java 목록에만 남는다). 결과 아이템의 메타,
  `ExactChoice`, 클라이언트 레시피북 표시는 없다.
- **NMS 내부 연결(§2), 맵/구조물/엔티티(§6), 그 밖의 Paper 생명주기·스케줄러(§7), 게임 상호작용 검증(§8):** 이 세션에서 손대지 않았다.
- 실제 26.3 클라이언트 검증(§8)은 하지 못했다. 위 항목은 단위 테스트와 호스트 라우트 테스트까지만 확인했다.

