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

> 상태(2026-11-14): 구현·거부 항목과 테스트는 §10.6의 표, 남은 한계는 §10.6 끝의 목록.

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
| `Server#addRecipe`/`removeRecipe`/`clearRecipes`/`resetRecipes` | `FerriteRecipes`가 성형·비성형·`FurnaceRecipe`/`BlastingRecipe`/`SmokingRecipe`/`CampfireRecipe`·`StonecuttingRecipe`·`SmithingTransformRecipe`/`SmithingTrimRecipe`를 바닐라 레시피 JSON(+결과 메타·내구도)으로 호스트에 보냄 → `game/crafting.rs` 오버레이(바닐라 먼저, 제거된 id는 건너뜀, 그다음 플러그인 레시피) → `ferrite-recipe` 매칭. 클라이언트 레시피북·자동 채우기에도 나온다(§10.6). `ExactChoice`/`PredicateChoice`·`TransmuteRecipe`·특수 제작은 이유와 대체 API를 담은 예외 | §10.6 표 |
| `sendBlockUpdate`: 스냅샷 아이템의 `ItemMeta` | `ItemMetaWire`(인벤토리 슬롯과 같은 직렬화) → `meta,<슬롯>,<hex>` → `with_meta` | Rust `a_stack_in_the_snapshot_can_carry_metadata`, Java `aStackWithMetadataSends...` |
| Paper 부트스트래퍼 `DATAPACK_DISCOVERY` | `FerriteBootstrap` — `paper-plugin.yml`의 `bootstrapper`를 플러그인 생성 전에 실행, 데이터팩을 `<월드>/datapacks/<플러그인>-<id>`로 복사. `TAGS` 등 다른 라이프사이클 이벤트는 이유를 밝히고 거부 | Java `FerriteBootstrapTest` |
| `help.yml` | `FerriteHelpMap(server, File)` — `ignore-plugins`, `amended-topics`, `index-topics` | Java `ApiSurfaceTest` |
| 지도 커서 스타일 캡션 | `ferrite-nbt::read_network_prefix`, `NetworkText{Plain,Rich}`, `MapCaption`; 캡션 모드 `0/1/2` (**Java가 `0/1`을 보내는데 Rust가 bool로 읽던 잠재 버그도 수정**) | Rust `styled_and_numeric_caption_modes_are_parsed`·play.rs 왕복, Java `styledMapCaptionsTravelAsComponentNbt` |
| `sendBlockUpdate`: 주크박스·교탁·명령 블록·스포너·스컬크 센서·벌집·구조물 블록 | `tile_update.rs` `SETTINGS`/`KEYED_STACK` (키는 로컬 `server-26.3.jar`를 `javap`로 읽어 확인). 침대는 26.3에 블록 엔티티가 없어 `UnsupportedOperationException` | Rust `tile_update` 테스트, Java `PlayerTileUpdateTest` |
| 기본 월드 엔티티 스냅샷 | `facts::publish_entities`가 오버월드도 SNBT 저장(20틱 주기) → `CapturedStructure`/`getStructures` | Rust facts·snapshot 테스트 |
| `CraftMagicNumbers` | `INSTANCE`, `getMaterial`(모르는 키는 예외), `getKey`, `getDefaultBlockData`, `isLegacy`, `fromLegacy` | Java `BootstrapTest` |
| `World#setVoidDamage…`, `Player#getConnection`/`postEffects`, `World#getWorldPath`, POI 검색, 블록 엔티티 수 | §10.2 아래·10.3 참고 | Java `PlayerExtrasTest`, Rust `craft::tests::void_rules...` |
| `Player#fireworkBoost` | 호스트에 폭죽 엔티티 추가(`ferrite-entity::firework`: 수명 `10*비행+rand(6)+rand(7)`, 비행 중 `×1.15, +0.04`, 블록 충돌·수명 종료 시 상태 17 폭발, 별이 있으면 반경 5칸에 `5+2*별` 피해). 활강 상태는 `START_FALL_FLYING`부터 착지까지 추적(`isGliding()`)하고, 붙은 폭죽은 매 틱 시선 방향으로 `SetEntityMotion`을 보낸다. `World#spawn`의 `firework_rocket`도 지원 | Rust `firework::tests::*`, `motion::tests::a_firework_boost...`, `facts::tests::gliding_follows_the_registry` |

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

- **Player:** 남은 Player API의 구현/거부 표와 한계는 §10.7로 옮겼다(`fireworkBoost` 핸들, `loadData`, 리소스 팩 프롬프트, `spawnParticle` 데이터, `getVirtualHost`, `EntityEquipment`, `copy`/`spawnAt` 거부 등).
  `getConnection`(플레이어 메서드로 전달하는 프록시; `reenterConfiguration`·`sendReportDetails`는 `UnsupportedOperationException`)과
  `postEffects`(`/posteffect`를 콘솔로 실행하고 목록은 Java에 미러링 — 명령으로 바꾼 상태는 `values()`에 보이지 않는다)는 구현했다.
- **World:** `locateNearestPoi`/`locateAllPoiInRange`는 POI 저장소가 없어 로드된 청크를 스캔한다(`Occupancy.ANY`만 지원,
  호스트가 블록 속성을 주지 않으면 침대 머리/발 같은 구분이 안 된다). `getTileEntityCount`는 로드된 청크의 블록 엔티티 수,
  `getTickableTileEntityCount`는 바닐라에서 틱 처리를 하는 타입 목록(`TICKING_BLOCK_ENTITIES`) 기준이다.
  `getWorldPath`는 기본 월드만 된다. `getFluidData`, `spawnAt`, `vanillaBiomeProvider`는 컴파일 대상 Paper API(26.3 build 227)에
  존재하지 않아 구현 대상이 아니다.
- **블록 엔티티:** 위 표의 종류는 구현했다. 그 밖의 `sendBlockUpdate` 대상은 `UnsupportedOperationException`을 유지한다.
- **레시피(§7):** §10.6에 구현/거부 표와 한계를 옮겼다(모닥불 레시피는 일치만 하고 호스트에 모닥불 요리가 없음, `smithing_trim`은 클라이언트 레시피북에 안 나옴, `ExactChoice`는 거부).
- **투사체 종류:** `launchProjectile(Firework)`는 구현했다(호스트 폭죽이 눈 위치에서 위로 난다, 속도 인자 무시). `launchProjectile(BreezeWindCharge)`도 구현했다(`Kind::BreezeWindCharge`, 일반 바람 구슬과 같은 비행·폭발). `launchProjectile(WitherSkull)`도 구현했다(직선 비행, 닿으면 위력 1 폭발, 불은 없음). `launchProjectile(ShulkerBullet)`도 구현했다(유도 없이 직선, 닿은 몹에 4 피해, 공중 부양 효과 없음). `launchProjectile(SpectralArrow)`도 구현했다(화살처럼 날고 맞은 몹이 10초간 발광). 낚싯찌(`FishHook`)는 아래 항목대로 `launchProjectile`이 지원하고, 눈 엔더는 `launchProjectile` 대상이 아니다(바닐라 `EyeOfEnder`도 플레이어가 아이템으로만 던진다; Bukkit에는 눈 엔더 `Projectile` 인터페이스가 없다). `World#getEnderDragonBattle`은 End 월드에서 `FerriteDragonBattle`을 돌려준다(호스트가 틱마다 올리는 전투 스냅샷을 읽고, 변경은 큐에 넣어 End 리전의 다음 틱에 적용 — 플레이어가 End에 있을 때만 움직인다. 보스바는 분리된 객체, `resetCrystals`(스파이크 크리스털을 지우고 배치 패스가 전부 다시 놓음)와 `spawnNewGateway(Position)`(그 자리에 관문을 열고, 바닐라처럼 개수는 올리지 않음)도 구현했다, 이름이 `_the_end`로 끝나는 런타임 월드도 같은 호스트 전투를 본다).
- **`Player#loadData`/`saveData`:** `PlayerCommand::PlayerData` → 리전 `Command::PluginLoadData/PluginSaveData`. load는 저장 파일의 인벤토리·선택 슬롯·게임 모드를 적용하고, 같은 월드에서 저장된 파일이면 위치도 되돌린다(월드가 다르면 위치는 건드리지 않는다). 체력·흡수·허기·포화·소모도·경험치는 이제 저장 파일(`vitals`)에 들어가고 load가 되돌린다(§10.7). 상태 효과·불·산소·낙하 거리는 바닐라처럼 저장하지 않는 쪽이라 되돌리지 않는다.
- **NMS 메뉴(§2.4):** 상자 계열과 `SHULKER_BOX`는 그대로 열고, 작업대·모루·마법부여·숫돌·베틀·대장간·석재 절단기·지도 제작대는 호스트 스테이션으로 연다(내용 패킷은 명시적 예외). 호퍼·발사기·화로·양조기·신호기·교탁·상인·크래프터는 구체적인 이유와 대체 API를 담은 예외를 유지한다.
- **TAGS 라이프사이클 이벤트:** 태그 레지스트리가 시작 후 고정이라 거부한다. `fromLegacy`의 데이터 바이트는 지원하지 않는다.
- **NMS 엔티티·월드 핸들(§2.2/2.5):** `net.minecraft.world.entity.Entity`는 이제 델타 이동(`getDeltaMovement/setDeltaMovement`), `setPos/teleportTo`, 회전, 지면·불·물 상태, 크기, 커스텀 이름, 스코어보드 태그, 승객·탑승 관계(`getPassengers/getVehicle/startRiding/stopRiding`)를 Bukkit 호출로 읽고 쓴다(Entity.of로 플레이어/생명체 래퍼 선택). `LivingEntity`는 흡수 체력·수면·활강·아이템 사용·박힌 화살·효과 확인, `ServerLevel`은 시드·스폰·차원 키·엔티티 목록·UUID 조회, `Vec3`는 벡터 연산을 갖는다. 모두 Bukkit(→호스트 라우트)을 거치므로 리전 소유권을 우회하지 않는다. 테스트 `NmsEntityHandleTest`. 직접 NMS 필드 접근(`entityData`, 속성 맵 등)은 노출하지 않는다.
- **눈 엔더·낚싯찌:** 눈 엔더는 `ferrite-entity::eye`(80틱 비행, 5분의 4는 아이템으로 떨어지고 나머지는 부서짐, 목표가 12칸 밖이면 12칸 앞·8칸 위로 조준)와 리전 `tick_eyes`(생성 직후 `locate_structure("minecraft:stronghold", …, 100)`로 목표 탐색, 없으면 즉시 부서짐)로 동작하고, 공중에서 눈을 쓰면 던져지며(`EnderEyeItem.use`), `World#spawnEntity(EYE_OF_ENDER)`도 된다. 이동식은 바닐라 `EyeOfEnder.tick`/`updateDeltaMovement` 바이트코드와 대조해 고쳤다(§10.7: 이전 속도로 먼저 움직이고 새 위치에서 속도를 구한다). 낚싯찌는 호스트에 이미 있던 `bobber`를 플러그인이 `launchProjectile(FishHook)`로 던질 수 있게 연결했다(이미 찌가 있으면 무시). `launchProjectile(FishHook)`은 Java가 정한 UUID를 호스트가 찌에 붙여, 핸들(`FISHING_BOBBER`)을 돌려준다(이미 찌가 나가 있으면 호스트가 무시하므로 그 핸들은 실체가 없다).
- **§8 바닐라 대조(모루):** 바닐라 `AnvilMenu.createResult`와 대조해 세 가지를 고쳤다 — 같은 아이템 합치기 보너스는 최대 내구도의 12%(5%였음), 이름만 바꾸는 경우 비용은 39에서 멈추고 수리 비용(`RepairCost`)을 올리지 않는다, 창작 모드(무한 재료)는 40레벨 상한이 없다(`Menu::set_infinite_materials`). 테스트 `anvil_rename_only_keeps_the_repair_cost_and_creative_lifts_the_cap`. 유체 경계·레드스톤·청크 언로드 흐름·이웃 갱신 순서·작물 성장·중력 블록 연쇄·마법부여대·지도/세공대 결과는 이번에 대조하지 못했다(로컬 서버 jar를 `javap`로 읽을 수는 있지만 항목마다 바이트코드를 따라가야 해서 시간이 들었다).
- **NMS 내부 연결(§2), 맵/구조물/엔티티(§6), 게임 상호작용 검증(§8):** 이 세션에서 손대지 않았다. §7은 §10.6.
- 실제 26.3 클라이언트 검증(§8)은 하지 못했다. 위 항목은 단위 테스트와 호스트 라우트 테스트까지만 확인했다.


### 10.4 실제 서드파티 플러그인 호환 (2026-11-14)

하니스: `cargo run -p ferrite-plugin --example load_plugins -- <플러그인 폴더> [이름…]`(먼저 `gradle shadedJar`, Java 25 필요).
서드파티 jar는 커밋하지 않는다. 고친 것은 모두 `RealPluginCompatTest`/`BootstrapTest`에 회귀 테스트가 있다.

- **활성화 확인:** Vault, EssentialsX 2.22.0(자기 jar가 26.x 이전이라 "지원하지 않는 서버 버전" 경고를 낸다), PlaceholderAPI, Votifier, BlueMap, Multiverse-Core 5.8.2, AuthMe, DiscordSRV, SkinsRestorer, ChestShop, Skript 2.17.1.
- **고친 것:** 플러그인 하나가 로드·활성화에 실패해도 호스트 전체가 멈추지 않는다(`loadOrdered`가 플러그인마다 격리, 실패 로그에 파일명). 정적 `@EventHandler` 메서드(바인딩 안 함). `CraftServer#getCommandMap` 선언(ACF/Multiverse가 `getDeclaredMethod`로 찾음). `VanillaCommandWrapper` 스텁. `GameProfile` 속성(`Property`/`PropertyMap`). `VanillaArgumentProvider`(`FerriteArgumentProvider`, 일부 인자 종류는 `UnsupportedOperationException`). `Bukkit.createBlockData(material, "[…]")`가 속성 문자열을 받는다. `UnsafeValues#modifyItemStack`(`ItemComponents`: 이름·설명·모델 데이터·내구도·불괴·수리 비용·마법부여·포션 기본 효과). `PotionMeta`·`EnchantmentStorageMeta`는 Java 쪽에만 있다. 메타 필드가 없는 컴포넌트(`fireworks` 등)는 매칭용으로 값을 보관하되 **호스트로 보내는 순간 `UnsupportedOperationException`**으로 거부한다(조용히 버리지 않는다). `log4j-core`·`netty-all` 번들(Skript·AuthMe가 필요로 함).
- **동작하지 않는 것(정직한 한계):** packetevents·ProtocolLib류와 ViaVersion의 채널 주입은 실제 NMS 네트워크 스택(`ServerConnection`, Netty 채널)이 필요해 로드는 되지만 주입에 실패한다. TAB 6.2.1 jar는 26.x를 지원하지 않는다. ProtocolLib·WorldEdit·LuckPerms·CoreProtect는 받을 수 있는 릴리스가 없어 시험하지 못했다. 로드 성공이 곧 기능 동작을 뜻하지는 않는다 — 명령·이벤트를 실제 클라이언트로 돌려 보지 않았다.
- **계약 변경:** 이전에는 한 파일이 실패하면 로드된 플러그인을 모두 되돌렸다. 이제는 실패한 파일만 건너뛴다(`BootstrapTest.paperOnlyLoadsAndOneFailingFileDoesNotStopTheOthers`).

- **§8 바닐라 대조(작물 성장):** `CropBlock.getGrowthSpeed`/`randomTick` 바이트코드(`nextInt((int)(25/속도)+1)`, 농지 1.0·젖은 농지 3.0·이웃 1/4, 같은 작물이 직각 두 쪽 또는 대각선에 있으면 1/2)와 `random_tick.rs`를 대조했고 `grows_crops` 태그(농지뿐)까지 일치한다. 고칠 것은 없었다. 유체·레드스톤·청크 언로드·이웃 갱신 순서·중력 블록 연쇄·마법부여대·지도/세공대는 여전히 대조하지 못했다.

### 10.5 §8 바닐라 대조 결과 (2026-11-14)

서버 jar를 `javap -c`로 읽어 구현과 대조했다. 고친 것과 확인만 한 것, 못 한 것을 구분한다.

- **마법부여대(고침):** 이전 구현은 아이템 종류마다 마법부여 하나를 고정하고 비용을 `선택 번호 + 1`로 정해 둔 자리표시자였다. 이제 `EnchantmentMenu`/`EnchantmentHelper`를 따른다: 책장 개수(`EnchantingTableBlock.BOOKSHELF_OFFSETS`, 사이 칸이 `#replaceable`일 때만), 플레이어별 시드(`Player.enchant_seed`, 마법부여 후 바뀜), 버튼 비용(`getEnchantmentCost`), 마법부여 가능도(`minecraft:enchantable`)로 정한 힘, 가중치 선택과 배타 집합(`ferrite-loot`의 `selectEnchantment`), 책은 마법부여된 책으로 바뀌고 목록 한 개가 빠짐, 단서(클라이언트 레지스트리 순서의 마법부여 id)와 단서 레벨, 라피스 `선택+1`개와 레벨 `선택+1` 소모(창작 모드는 무한). 테스트 `enchant_table::tests`, 인벤토리 `stonecutter_grindstone_enchanting_...`. 한계: 난수는 `java.util.Random`이지만 바닐라의 난수 원본과 같지 않아 같은 시드가 바닐라와 같은 버튼을 내지는 않는다. 시드는 플레이어 데이터(`Saved.enchant_seed`)에 저장되어 재접속·재시작 뒤에도 유지된다(이전 저장 파일은 접속 때 새로 정한다; 테스트 `playerdata::tests::enchant_seed_round_trips_...`). 책장은 바닐라 `slotsChanged`처럼 메뉴에서 슬롯을 만질 때마다 다시 센다(`Menu::set_bookshelves`); 슬롯을 건드리지 않고 가만히 열어 둔 동안은 갱신하지 않는데, 바닐라도 그렇다.
- **중력 블록(고침):** `FallingBlock`을 상속하는 클래스를 jar에서 전부 찾았다. `dragon_egg`(`DragonEggBlock`)가 빠져 있어 추가했다. 콘크리트 가루는 물에 닿아도 굳지 않았다 — `ConcretePowderBlock.shouldSolidify`(자기 칸이 젖었거나, 아래를 뺀 다섯 방향의 물이 단단한 면 뒤에 있지 않음)를 구현했다. 떨어지는 도중 물에 들어가면 `FallingBlockEntity.tick`처럼 착지로 친다: 자기 칸이 물이면 그 자리에, 한 틱에 1칸보다 빨리 떨어질 때는 이동 경로의 첫 물에서 멈춰 블록으로 놓이고 곧바로 굳는다(테스트 `falling::tests::concrete_powder_lands_in_water_before_reaching_the_floor`).
- **유체(고침):** 물·용암의 틱 지연(5, 용암 30/지옥 10)·퍼짐 거리·감쇠는 일치한다. `LavaFluid.getSpreadDelay`의 난수(용암 높이가 오르면 4분의 3 확률로 지연 4배)가 빠져 있어 구현했다. 나머지 `FlowingFluid`(경사 거리 탐색 등)는 상수만 대조했다.
- **레드스톤(고침):** 횃불 탄화(160틱)·창문(60틱 8회)·버튼(20/30틱)·관찰자·대상 블록의 지연은 일치한다. 압력판은 플레이어만 감지하고 즉시 풀렸다 — 이제 모든 엔티티를 감지하고(돌·검은돌 판은 생명체만), 눌리면 20틱(가중판 10틱) 뒤에야 다시 보며, 가중판(`power`)은 엔티티 수에 비례한 출력을 낸다(경철 150, 금 15). 테스트 `plates_sense_items_by_material_and_weighted_plates_count_entities`. 엔티티 상자는 바닐라와 같이 회전하지 않는 축 정렬 상자라 방향은 영향이 없고, 플레이어의 높이는 자세(웅크림 1.5, 수면 0.2×0.2)를 따른다(`Occupant::player_in_pose`). 수영·활공 자세의 상자(0.6 높이)는 서버가 그 자세를 아직 추적하지 않아 서 있는 상자로 센다.
- **이웃 갱신 순서(확인만):** `SHAPE_ORDER`가 `Block.UPDATE_SHAPE_ORDER`(서·동·하·상·북·남)와 같다.
- **작물 성장·지도 제작대(확인만):** 바이트코드와 일치한다(§10.3).
- **청크 언로드 흐름(대조함, T1):** `ChunkLevel`·`DistanceManager`(`PlayerTicketTracker`, `FixedPlayerDistanceChunkTracker`)·`TicketType`·`ChunkMap`(`setServerViewDistance`, `getPlayerViewDistance`, `processUnloads`)을 바이트코드로 읽었다. 확인한 값: 풀 청크 레벨 33, 블록 틱 32, 엔티티 틱 31(`PLAYER_TICKET_LEVEL`도 31); 플레이어는 뷰 거리 안(체비쇼프)의 모든 청크에 만료 없는 `PLAYER_LOADING` 티켓을 갖고 자기 칸에 `max(0, 31 − 시뮬레이션 거리)` 레벨의 `PLAYER_SIMULATION` 티켓을 갖는다; 서버 뷰 거리는 2~32, 플레이어 뷰 거리는 `clamp(클라이언트 요청, 2, 서버)`; 레벨이 33을 넘은 청크는 다음 `processUnloads`에서 유예 없이 저장·언로드된다(`TicketType`의 `player_spawn` 20틱, `portal` 300틱, `ender_pearl` 40틱 만료는 플레이어 티켓과 무관). 고친 차이: 클라이언트가 요청한 뷰 거리를 무시하고 서버 값을 썼다 — 이제 `chunk_level::effective_view`로 접속 때 `clamp(요청, 2, 서버)`를 쓴다(접속 중 `ClientInformation` 변경은 아직 반영하지 않는다). 같은 것으로 확인한 것: 뷰 사각형 경계(`chunk_level::in_view`)와 마지막 뷰어가 놓는 즉시 언로드·저장(회귀 테스트 `chunk_level::tests`, `region::event_policy_tests::{the_view_window_is_the_vanilla_square_..., releasing_every_hold_...}`, 기존 `shared_chunk_uses_one_region_holder_until_last_viewer_leaves`, `two_viewers_one_leaving_first_then_reentry_keeps_one_entity`). 구조상 다르게 둔 것: ① 바닐라는 뷰 사각형 밖 두 칸까지(레벨 32·33) 낮은 상태로 청크를 더 불러 두지만 Ferrite는 클라이언트에 보낸 사각형만 리전이 쥔다; ② 엔티티·블록 틱은 시뮬레이션 거리가 아니라 쥐고 있는 청크 전체에서 돌고(`simulation-distance` 설정은 틱 범위에 쓰이지 않는다), 로그인 패킷의 시뮬레이션 거리도 뷰 거리와 같다; ③ 티켓 종류별 만료(포탈 300틱 등)는 모델링하지 않는다.
- **레드스톤 신호 모델(대조함·고침, T3):** `Level.getSignal`/`getDirectSignalTo`, `BlockState.getSignal`/`getDirectSignal`(레버·버튼·압력판·횃불·관찰자·리피터·비교기·레드스톤 선), `DiodeBlock`/`RepeaterBlock`/`ComparatorBlock`, `SignalGetter.getControlInputSignal`, `RedStoneWireBlock`(`getConnectingSide`, `shouldConnectTo`, `calculateTargetStrength`)을 바이트코드로 따라갔다. 고친 것(바닐라 / 이전 Ferrite / 테스트):
  - **신호에 방향이 없었다** → 방향별 신호(`signal_toward`/`direct_toward`)와 컨덕터의 강한 신호(`Probe::signal`/`direct_to`). 레드스톤 블록·횃불이 컨덕터를 강하게 켜던 것, 횃불이 자기 받침 블록을 켜서 꺼지던 것(`update_torch`의 `neighbour==pos` 우회), 램프가 인접한 컨테이너 신호를 읽던 것을 없앴다. 테스트 `a_torch_powers_the_block_above_it_but_not_the_one_it_stands_on`, `repeater_and_observer_power_only_the_block_in_front_of_them`.
  - **리피터·비교기 방향이 뒤집혀 있었다** (`FACING`은 입력 쪽, 출력은 `FACING.opposite()`) → 입력은 `pos.relative(FACING)`, 좌우 입력은 시계/반시계 방향. 기존 테스트도 거꾸로 짜여 있어 모두 바로잡았다.
  - **리피터 펄스 늘이기가 없었다**(`DiodeBlock.tick`: 꺼졌을 때 켜고, 입력이 없으면 지연 뒤 다시 끈다) → 구현. `checkTickOnNeighbor`의 틱 우선순위(`EXTREMELY_HIGH`/`VERY_HIGH`/`HIGH`)와 `willTickThisTick`, 이미 예약된 틱은 시각을 바꾸지 않는 동작, 같은 틱 안의 실행 순서(시각·우선순위·좌표)도 따른다. 테스트 `repeater_stretches_a_pulse_shorter_than_its_delay`.
  - **비교기는 잠기지 않는다**(`ComparatorBlock`에는 잠금이 없고 리피터 등은 15짜리 측면 입력일 뿐): 잘못 넣어 둔 잠금을 없앴다. 입력은 뒤의 컨테이너·작물 상자·가마솥·케이크·퇴비통·꿀벌집 등(`hasAnalogOutputSignal`)이나 컨덕터 한 칸 뒤의 것을 읽는다. 테스트 `a_powered_repeater_is_a_side_input_to_a_comparator_not_a_lock`, `comparator_output_subtracts_a_wire_side_input_and_reads_what_it_faces`.
  - **레드스톤 선:** 전체 이웃 6칸을 한 덩어리로 묶던 것을 바닐라의 `calculateTargetStrength` 연결(수평 이웃, 컨덕터 위로 한 칸, 비컨덕터 아래로 한 칸 — 계단식)로 바꿨다. 모양은 `getConnectingSide`/`shouldConnectTo`(돌·램프에는 붙지 않고, 리피터는 일직선일 때만, 관찰자는 뒷면만)와 `getConnectionState`(점은 점으로 남고, 한 축에 연결이 없으면 양쪽으로 뻗는다)를 따른다. 점 모양 선은 옆 블록을 켜지 않는다. 테스트 `wire_networks_follow_staircases_and_dots_do_not_power_sideways`, `wire_does_not_connect_to_plain_blocks_or_lamps`.
  - **램프:** 켜질 땐 즉시, 꺼질 땐 4틱 뒤(`RedstoneLampBlock`) — 이전엔 즉시 꺼졌다. **소리 블록:** 신호로 음이 바뀌지 않는다(`NoteBlock.neighborChanged`는 음을 연주만 한다) — 이전엔 켜질 때마다 +1. **문:** 아래·위 어느 반쪽의 이웃 신호든 받는다(`DoorBlock.neighborChanged`). 컨덕터이면서 소비자인 블록(램프·소리 블록)이 컨덕터를 거친 강한 신호를 받지 못하던 것도 고쳤다. 이웃 갱신 순서는 `NeighborUpdater.UPDATE_ORDER`(서·동·하·상·북·남)로 맞췄다.
  - **설치 방향:** 문·울타리 문·계단은 `getHorizontalDirection`(보는 쪽), 리피터·비교기는 플레이어를 향한다. 이전에는 모두 플레이어를 향했다. 테스트 `doors_and_gates_face_the_look_while_diodes_face_back_at_the_player`.
- **피스톤(확인만, T3):** `PistonStructureResolver`(12칸 한도, 끈끈이 선 확장·분기·`reorderListAtCollision`, 슬라임↔꿀 비접착, `PUSH_ONLY`/`DESTROY`)와 `PistonBaseBlock.isPushable`, `getNeighborSignal`(준연결 포함)을 `piston.rs`와 대조했고 일치한다.
- **중력 블록 연쇄(확인만, T3):** `FallingBlock.tick`/`isFree`/`onPlace`/`updateShape`의 2틱 지연은 `gravity.rs`와 같고, 아래 블록이 사라지면 위 블록이 2틱 간격으로 이어 떨어진다. 다만 바닐라는 아래가 막혀 있어도 이웃 갱신마다 2틱 예약을 걸고 그때 검사하지만, Ferrite는 갱신 시점에 이미 비어 있을 때만 예약한다(결과는 같고 예약 수만 적다).
- **지도 제작대·숫돌·석재 절단기·셜커 상자(고침, T3):** 지도 제작대(`CartographyTableMenu.setupResultSlot`)는 일치. **숫돌**은 `GrindstoneMenu.computeResult`와 달랐다 — 낱개 아이템은 마법이 없어도 결과가 나왔고, 저주도 지웠고, 합칠 때 5% 보너스가 없었고, 경험치는 마법 하나당 2로 고정이었다. 이제 마법이 없는 낱개·2개 이상 쌓인 스택은 결과 없음, 저주만 남기며 수리 비용을 저주 수만큼 올리고(`calculateIncreasedRepairCost`), 마법 책은 비면 책으로 돌아오고, 같은 아이템 합치기는 `max − (남은1 + 남은2 + max×5/100)`(내구도 없는 같은 스택 둘은 2개), 경험치는 `Enchantment.getMinCost` 합의 절반(올림) + 난수, 입력 칸은 내구도 있는 것이나 마법 부여된 것만 받는다. 테스트 `a_grindstone_follows_vanilla_compute_result`, `grindstone_experience_is_half_the_minimum_costs_rounded_up`, `grindstone_slots_take_only_damageable_or_enchanted_items`. **석재 절단기**는 재료 종류가 바뀌면 선택을 −1로 되돌리고 결과를 비우며(`setupRecipeList`) 버튼을 눌러야 결과가 나온다 — 이전엔 0번 레시피가 자동 선택됐다. **셜커 상자 염색**(`crafting_transmute`)은 결과가 아이템 id와 개수뿐이라 내용물이 사라졌다 — `TransmuteRecipe.assemble`처럼 입력 스택의 컴포넌트를 모두 옮기고, 지도 복제(`result: {}`, `add_material_count_to_result`)도 지도 id를 유지한 채 `1 + 빈 지도 수`개를 낸다. 테스트 `dyeing_a_shulker_box_keeps_its_contents`, `cloning_a_map_keeps_the_filled_map_and_counts_the_blank_ones`.
- **레드스톤 한계 7건(고침, T3b):** 앞서 남겨 둔 7건을 바이트코드와 대조해 고쳤다.
  - **횃불 탄화** — `RedstoneTorchBlock.tick`/`isToggledTooFrequently`: 60틱(`clock − first > 60`)보다 오래된 기록은 버리고, 꺼질 때(`add=true`) 기록하며 같은 칸 기록이 8개 이상이면 160틱 뒤로 예약하고, 꺼진 채 8개 이상이면 다시 켜지 않는다. 이전엔 켜질 때 기록했다. 테스트 `torch_burnout_window_is_sixty_ticks_and_counts_turn_offs`, `torch_burns_out_after_repeated_toggles_and_recovers`.
  - **전원 레일 연쇄** — `PoweredRailBlock.updateState`/`findPoweredRailSignal`/`isSameRailWithPower`: 같은 축 레일을 따라 깊이 8까지(신호를 받은 레일 포함 9칸), 오르막은 위·아래 칸, 모양이 어긋난 레일은 건너지 않는다. 바뀌면 아래 칸(오르막은 위 칸도)을 갱신한다. 테스트 `a_powered_rail_line_carries_power_eight_rails_along_the_track`, `a_powered_rail_does_not_carry_power_across_a_perpendicular_track`.
  - **아이템 액자 → 비교기** — `ComparatorBlock.getInputSignal`/`getItemFrame`, `ItemFrame.getAnalogOutput`: 도체 뒤 한 칸 너머에서, 비교기와 같은 방향을 보는 액자가 정확히 하나일 때 `회전 % 8 + 1`(빈 액자 0), 블록의 비교기 출력과는 최댓값을 쓴다. 지역이 액자 엔티티(`spawn_data` 방향, 메타 8·9)에서 매 틱 계산해 넘기고 바뀐 칸은 이웃을 갱신한다. 테스트 `comparator_reads_an_item_frame_behind_a_conductor`, `item_frames_output_rotation_plus_one_and_zero_when_empty`.
  - **감지 레일·트립와이어 갈고리·함정 상자 출력** — `DetectorRailBlock`(`getSignal`=켜짐 15, `getDirectSignal`=켜짐이고 위 방향만), `TripWireHookBlock`(켜짐이면 15, 직접 신호는 `FACING`==방향), `TrappedChestBlock`(열람자 수 0..15, 직접 신호는 위 방향만; 열람자는 `ContainerOpenersCounter`처럼 큰 상자 두 칸 모두에 센다). 테스트 `detector_rail_tripwire_hook_and_trapped_chest_emit_like_vanilla`, `trapped_chest_viewers_power_a_lamp_below_through_direct_signal`, `trapped_chest_viewers_count_each_open_menu_on_both_halves`.
  - **같은 틱·같은 우선순위의 실행 순서** — `LevelTicks`(`ScheduledTick.DRAIN_ORDER`: 시각 → 우선순위 → `subTickOrder`)처럼 예약 순서를 `sequence`로 기록해 `(시각, 우선순위, 예약 순서)`로 정렬한다. 분할은 순서를 옮기고, 합칠 땐 기존 틱 뒤에 원래 상대 순서로 붙인다. 테스트 `scheduled_ticks_with_equal_time_and_priority_run_in_insertion_order`.
  - **숫돌 경험치** — `GrindstoneMenu$4.onTake`: 이제 `ExperienceOrb.award`처럼 블록 중심에 오브 엔티티로 떨어뜨리고 `levelEvent(1042)`를 보낸다(블록 없이 연 숫돌은 `ContainerLevelAccess.NULL`처럼 지급 없음). 테스트 `grindstone_experience_is_half_the_minimum_costs_rounded_up`, `stonecutter_grindstone_enchanting_smithing_and_loom_work`.
  - **영역 경계** — 전원이 두 칸 거리의 도체를 거쳐 다른 영역 블록에 닿을 때도 그 영역을 깨우고, 깨움에 변경 출처를 실어 경계 너머를 보는 관찰자가 펄스를 낸다(`Wake`). 반대편이 스스로 바꾼 블록은 이쪽을 다시 깨우지만, 받은 깨움은 출처로 돌려보내지 않는다. 테스트 `boundary_circuits_wake_the_other_region`, `a_repeater_next_to_the_boundary_reacts_and_wakes_back`.
- **남은 한계(레드스톤):** 감지 레일의 `powered`와 트립와이어·갈고리의 상태 전이를 만드는 쪽(광산 수레 엔티티, 트립와이어 줄 검사)이 Ferrite에 없어 출력 규칙만 맞췄고 상태는 직접 설정해야 켜진다. 아이템 액자는 배치 로직이 없어 `item_frame` 엔티티가 있을 때만 읽는다. 함정 상자 열람자는 그 영역이 가진 플레이어만 센다(경계 건너편에서 같은 상자를 연 경우 못 셈). 분산 틱 큐의 `UPDATE_LIMIT`(4096 고정)과 `RepeaterBlock` 외 `DiodeBlock` 하위 클래스의 우선순위 미세 차이는 그대로다. 한 영역이 다른 영역 블록을 직접 쓰지는 않으며, 두 영역이 서로 깨우는 방식으로만 수렴한다(§10.2).

### 10.6 §7 생명주기·레시피·스케줄러 (2026-11-14, 브랜치 `yxshadxw/plugin-api-lifecycle`)

| §7 항목 | 결과 | 위치 | 테스트 | 이유·대체 API |
|---|---|---|---|---|
| 부트스트래퍼의 `COMMANDS` 핸들러 | 구현 | `FerriteBootstrap#handOver`, `FerriteLifecycleManager#adopt` | `PaperLifecycleIntegrationTest.aBootstrapperCommandsHandlerIsHandedToThePluginFiresInitialAndReloadAndGoesOnDisable` | 플러그인 객체가 생기면 넘겨 INITIAL/RELOAD로 발화, disable 때 명령과 함께 제거 |
| `TAGS`(`pre_flatten/…`, `post_flatten/…`) | 거부 | `FerriteBootstrap` | `FerriteBootstrapTest`(태그 거부 메시지) | 태그는 호스트 시작 때 데이터 팩에서 읽고 이후 고정. 대체: `DATAPACK_DISCOVERY`로 데이터 팩을 제공 |
| 그 밖의 부트스트랩 이벤트 | 거부(원래대로) | `FerriteBootstrap`, `FerriteLifecycleManager` | `FerriteLifecycleTest.bootstrapOnlyEventsAreRefusedBecauseTheyCouldNeverFire` | 호스트가 발화할 수 없는 이벤트는 등록 시점에 이유와 함께 예외 |
| `PluginEnableEvent`/`PluginDisableEvent` | 구현 | `FerritePluginManager` | `...pluginEnableAndDisableEventsAreDeliveredWhileTheSubjectIsStillEnabledOnDisable`, `...disablingEverythingDisablesInReverseOrderAndAnnouncesEachPlugin` | 비활성 이벤트는 대상이 아직 enabled일 때 전달 |
| 비활성 플러그인의 리스너/태스크 등록 | 구현(`IllegalPluginAccessException`) | `FerritePluginManager#requireEnabled`, `FerriteScheduler` | `...registeringListenersWhileNotEnabledIsRefusedAndAReenableDoesNotDuplicateHandlers`, `...schedulingWhileDisabledIsRefusedButAlreadyCancelledTasksStayCancelled` | `onDisable` 안에서도 거부(Paper와 같음) |
| disable 시 정리(리스너·서비스·태스크) | 구현 | `FerritePluginManager#releasePlugin` | `...disablingAPluginCancelsItsBukkitGlobalRegionEntityAndAsyncTasks`, `...servicesAClosingPluginRegisteredAreRemovedWithIt`, `...shutdownCancelsEveryPluginTaskAndForgetsTheCommands` | Bukkit·global·region·entity·async 전부 취소, 재활성 시 핸들러 중복 없음 |
| 태스크 예외 | 구현 | `FerriteAsyncScheduler`, `FerriteRegionScheduler`, `FerriteEntitySchedulers` | `...aFailingRepeatingTaskIsLoggedAndKeepsItsSchedule` | 예외는 SEVERE로 기록하고 반복 태스크는 계속 돈다 |
| `getActiveWorkers` | 구현 | `FerriteScheduler.AsyncTask` | `...aRunningAsyncTaskShowsUpAsAWorkerUntilItEnds` | 실행 중인 비동기 태스크와 그 스레드 |
| event priority·`ignoreCancelled` | 검증 | `FerritePluginManager#callEvent` | `...eventHandlersRunLowestToMonitorAndIgnoreCancelledSkipsAfterACancel` | 실제 컴파일한 플러그인 jar |
| `reloadData` 경계 | 검증 | `FerriteServer#reloadData` | 위 부트스트래퍼 테스트(RELOAD 재발화, 중복 없음) | 호스트 호출 자체는 단위 테스트에서 호출할 수 없어 발화 부분만 직접 호출 |
| 조리/석재 절단/대장간 레시피 추가·삭제 | 구현 | `FerriteRecipes`, `game/crafting.rs`, `plugins/craft.rs` | 아래 레시피 표 | |
| 레시피 결과 메타 | 구현 | `FerriteRecipes`(`ItemMetaWire`), `PluginRecipes::dress` | `a_crafting_result_keeps_its_name_lore_enchantments_and_wear`, `PluginRecipeHostTest` | 이름·로어·인챈트·내구도 |
| `ExactChoice` | 거부 | `FerriteRecipes#ingredient` | `PluginRecipeHostTest`(거부 이유) | 26.3 `Ingredient`는 `HolderSet<Item>`이라 컴포넌트 조건이 없다(`javap`로 확인). 대체: `MaterialChoice` + 결과 슬롯의 `InventoryClickEvent`에서 메타 검사. Ferrite에는 `PrepareItemCraftEvent`/`CraftItemEvent`가 없다 |
| `PredicateChoice`, `TransmuteRecipe`, 특수 제작 | 거부 | `FerriteRecipes`, `add_plugin_recipe` | `PluginRecipeHostTest`, `transmute_and_special_recipes_are_refused_and_bad_json_says_why` | 데이터가 아니라 코드인 레시피. 대체: `MaterialChoice`나 이벤트 |
| 클라이언트 레시피북·자동 채우기 | 구현 | `game/recipe_wire.rs`, `recipe_packets.rs` | `a_plugin_recipe_reaches_the_clients_book_and_leaves_it_when_removed`, `a_player_who_joins_later_is_sent_the_plugin_recipes_with_the_first_book`, 바닐라 전 레시피 바이트 대조 테스트 | 성형·비성형·화로류·석재 절단·대장간 변형 |

레시피 종류별 테스트(추가 / 삭제 / 일치 / 결과 메타):

| 종류 | 테스트 |
|---|---|
| 성형·비성형 | `a_plugin_recipe_crafts_until_it_is_removed`, `a_crafting_result_keeps_its_name_lore_enchantments_and_wear` |
| 화로·용광로·훈연기 | `furnace_blast_furnace_and_smoker_recipes_are_added_matched_dressed_and_removed`, `removing_a_vanilla_smelting_recipe_stops_the_furnace_and_a_reset_restores_it`, `runtime_cooking_recipes_match_by_kind_and_stonecutting_lists_every_option` |
| 모닥불 | `a_campfire_recipe_is_added_matched_and_removed` |
| 석재 절단 | `stonecutter_recipes_are_added_listed_dressed_and_removed` |
| 대장간 변형·장식 | `smithing_recipes_are_added_matched_and_keep_the_base_components`, `a_smithing_trim_recipe_from_a_plugin_trims_with_its_own_pattern`, `a_netherite_upgrade_keeps_the_enchantments_of_the_diamond_tool` |
| 호스트 호출 | `plugins::craft::tests::add_recipe_takes_any_kind_with_its_result_metadata_and_refuses_what_it_cannot_read` |

**남은 것과 한계**

고친 것: 대장간 변형이 베이스의 컴포넌트(인챈트 등)를 버리던 것(바닐라 `TransmuteRecipe.createWithOriginalComponents`와 같게), 비활성 플러그인의 리스너 등록, disable 뒤 남던 리스너·서비스, 예외로 끝나던 반복 태스크, 재활성 시 핸들러 중복.

고칠 수 없거나 고치지 않은 것:
- 모닥불 레시피는 `Server#getRecipe`와 일치 조회만 된다. 호스트에는 모닥불 블록 엔티티 요리 자체가 없다.
- `smithing_trim`·`TransmuteRecipe`·특수 제작은 서버 쪽에서만 일치하고 클라이언트 레시피북에는 안 나온다(장식 표시에 장식 패턴 레지스트리 id가 필요).
- 플러그인 레시피는 모든 플레이어의 레시피북에 나온다. Java `discoverRecipe`는 호스트에 연결돼 있지 않다.
- 태그와 아이템이 섞인 재료의 표시 순서는 바닐라 태그 파일 순서와 다르다(집합 의미는 같고 클라이언트는 순서에 둔감).
- 같은 재료의 바닐라 레시피가 플러그인 레시피보다 먼저 일치한다.
- `ExactChoice`/`PredicateChoice`/`TAGS`는 설계상 거부(위 표).
- `BrewingRecipe`(`PotionMix`)는 Java 목록에만 남고 호스트 양조대는 바닐라 표를 쓴다.
- Folia 방식 region/entity/async 스케줄러는 비활성 플러그인에도 예외를 던지지 않는다(Bukkit 스케줄러만 던진다). 이미 취소된 태스크가 되는 것은 같다.
- 실제 클라이언트로 레시피북 패킷을 확인하지 못했다(바닐라 레시피 바이트와의 대조까지).
- `cargo clippy --workspace --all-targets -- -D warnings`는 `plugin-new` 병합 뒤 경고 없이 통과한다(`game/chunk_level.rs`는 T3b의 `cfg_attr`, `redstone.rs::rail_chain`은 `needless_late_init` 허용).

### 10.7 T4 — 남은 Player API·투사체·구름·폭죽 (2026-11-14, 브랜치 `yxshadxw/plugin-new`)

**§3 Player 서명별 상태**

| 서명 | 상태 | 호스트 구현 위치 |
|---|---|---|
| `setResourcePack(String,String,boolean[,Component])`, `setResourcePack(UUID,String,byte[],Component,boolean)` 등 3개 오버로드 | 구현(id·해시·프롬프트·required) | `plugins/network.rs::resource_pack`, `game/container.rs`(`ResourcePackPush.prompt`) |
| `PlayerResourcePackStatusEvent` | 구현(클라이언트 응답 → 이벤트) | `game/region.rs`(`ServerboundPlay::ResourcePackResponse`), `ferrite-plugin::fire_resource_pack_status` |
| `spawnParticle` 전 오버로드(데이터 포함) | 구현 — 블록·먼지·색 전이·색·주문·float·shriek·아이템·trail·vibration·geyser | `plugins/particles.rs`, `ferrite-protocol::packets::play::LevelParticles`(26.3 레이아웃으로 교정) |
| `getVirtualHost()` | 구현(핸드셰이크 호스트·포트, BungeeCord NUL 뒷부분 제거) | `ferrite-net::settings::record_virtual_host`, `plugins/facts.rs` |
| `getHAProxyAddress()` | null — PROXY 프로토콜을 받지 않는다 | — |
| `copy()`, `copy(Location)` | 거부(`UnsupportedOperationException`, Paper도 플레이어 복사 불가) | `PlayerMisc` |
| `spawnAt(Location,SpawnReason)` | 거부(플레이어는 스폰할 수 있는 엔티티가 아님) | `PlayerMisc` |
| `getAsString()` / `getPickItemStack()` / `getMemory(MemoryKey)` | null(바닐라 `Player.getPickResult`도 null, 플레이어는 기억 모듈이 없음) | `PlayerMisc` |
| `getData(DataComponentType.Valued)` / `hasData` | 미구현(null/false) — 플레이어에는 값 컴포넌트가 없고 아이템 컴포넌트는 `ItemStack`쪽 | — |
| `getEquipment()` 전체 | 구현(`clear()`는 방어구·손만, getter는 null이 아님, 드랍 확률은 항상 1.0이고 setter는 거부, BODY/SADDLE은 비어 있음) | `FerriteEntityEquipment.java` |
| `fireworkBoost` | 구현 — 이제 `Firework` 전용 핸들(`getLife/setLife/getMaxLife/setMaxLife/getTicksFlown/getTicksToDetonate/detonate/isDetonated/getFireworkMeta/getItem/getAttachedTo/getBoostedEntity/getSpawningEntity`) | `SpawnedEntity.firework`, `EntityOperation::SetFireworkLife/SetFireworkMaxLife`(`game/region.rs::plugin_entity_op`), `plugins/facts.rs`(`getFireworkLife()`) |
| `loadData()` | 구현 — 인벤토리·슬롯·게임 모드·위치(같은 월드) + 체력·흡수·허기·포화·소모도·경험치(`Vitals`) | `game/playerdata.rs`(`Vitals`), `game/region.rs::apply_vitals/load_player_data` |

- **loadData 판단:** 바닐라 플레이어 저장 형식은 `Health`·`AbsorptionAmount`·`foodLevel`·`foodSaturationLevel`·`foodExhaustionLevel`·`XpLevel`·`XpP`·`XpTotal`을 담고 로그인 때 되돌린다. 우리 저장 파일에도 같은 값을 넣었고(`vitals`), 접속 시에도 적용한다. 체력이 0 이하이거나 NaN이면 적용하지 않는다(죽은 상태로 되살리지 않는다). 상태 효과·불·산소는 저장하지 않는다.
- **`FireworkMeta`:** 폭죽 아이템의 `getItemMeta()`가 `FireworkMeta`(비행 시간 0..127, 별 목록)를 돌려준다. 이전에는 일반 `ItemMeta`라서 `fireworkBoost`가 비행·별 수를 읽지 못하고 기본값만 썼다. 폭죽 색·모양은 여전히 클라이언트에 보이지 않는다(호스트 `ItemStack`에 컴포넌트 없음).
- **붙은 폭죽 가속:** 시선 방향으로 정상 속도(≈0.85)를 보내던 근사를 지웠다. 바닐라 식 `delta + look*0.1 + (look*1.5 - delta)*0.5`에 쓰는 `delta`를 "폭죽이 마지막으로 붙은 위치에서 플레이어까지의 한 틱 이동"으로 추정한다(`firework::rider_velocity`). 서버가 플레이어 속도를 직접 적분하지 않기 때문에 추정이며, 폭죽 첫 틱은 델타 0으로 시작한다.
- **눈 엔더:** 바이트코드와 대조해 이동 순서를 고쳤다 — 바닐라는 현재 속도로 먼저 위치를 옮기고, 새 위치에서 목표까지의 거리로 다음 속도를 구한다(`lerp(0.0025, 수평 속도, 거리)`, 거리<1이면 0.8배, 위로 보는 판단은 이전 높이). 예전 코드는 새 속도로 먼저 움직였다. 테스트 `eye::tests::the_eye_moves_by_its_old_velocity_then_steers_from_the_new_position`.
- **투사체 점검:** `launchProjectile`이 다루는 것: (화살·눈덩이·달걀·엔더 진주·경험치 병·투척/잔류 포션·바람/브리즈 바람 구슬·작은/큰 화염구·위더 해골·셜커 탄·분광 화살·폭죽·낚싯찌·삼지창). 나머지 `Projectile`(`LlamaSpit`, `DragonFireball`, `EnderSignal` 등)은 `UnsupportedOperationException("not implemented")`로 거부한다 — 플레이어가 쏘는 바닐라 경로가 없고 호스트에 해당 비행 모델이 없다. 이미 구현된 비행·충돌·피해·줍기·저장은 다시 만들지 않았다.
- **포션 범위·구름(바이트코드 대조, `AbstractThrownPotion`/`ThrownSplashPotion`/`AreaEffectCloud`):**
  - 투척 포션: 대상 상자 `(4,2,4)` 확장, 거리는 포션 상자와 대상 상자(투사체 여백 0.3 포함) 사이의 간격이고(예전엔 발 위치까지 거리), 배율은 `1 - 거리/4`(예전의 최소 0.25 바닥 제거), 지속 시간은 `(int)(배율*지속+0.5)`이며 1초(20틱) 이하가 되면 적용하지 않는다. 즉시 효과(치유/해악)는 배율만큼의 세기로 들어간다(`(int)(배율*(4<<등급)+0.5)`, 해악은 6). 테스트 `potion::tests::a_splash_falls_off_with_the_gap_between_the_boxes_not_the_feet_distance`, `…rounds_the_duration…`, `region::tests::instant_effects_are_scaled_by_distance_and_halved_by_a_cloud`.
  - 잔류 구름: 대기(10틱) 동안은 아무것도 하지 않는다(예전엔 5틱에 이미 효과를 줬다), 대상 판정은 구름의 0.5칸 높이 상자와 대상 몸 상자가 겹치는지(예전엔 발 기준 -1..2), 즉시 효과는 절반 세기로 적용한다. 테스트 `a_waiting_cloud_hits_nobody_until_tick_ten`, `the_cloud_reaches_every_body_that_meets_its_half_block_box`.
  - 구름 색은 효과 색 평균이 아니라 기본 물색이다(효과 색 표가 없다). 바닐라의 `PotionContents.getColor`는 구현하지 않았다.
- **고치지 못한 것(이유):**
  - 리소스 팩 `required`는 클라이언트에 보낼 뿐 서버가 거부 응답으로 킥하지 않는다(바닐라도 서버 전체 필수 팩일 때만 킥).
  - `World#spawnParticle`은 데이터 없는 입자만 된다(콘솔 `/particle` 경로).
  - vibration 입자의 대상이 엔티티일 때 y 오프셋은 0.0 고정이다(바닐라는 엔티티 눈높이를 쓴다).
  - `getData`/`getMemory`/`getHAProxyAddress`는 위 표대로 null이다.
  - 폭죽 이동은 호스트 모델이고, 붙은 폭죽 가속의 플레이어 속도는 위 추정이다. 클라이언트 속도를 서버가 권위적으로 갖지 않는 한 정확히 같게 만들 수 없다.
  - 26.3 클라이언트로 입자·리소스 팩·폭죽을 눈으로 확인하지 않았다(지시에 따라 생략).
