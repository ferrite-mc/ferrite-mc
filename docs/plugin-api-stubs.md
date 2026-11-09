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

### 경계가 없어 구현하지 않은 것 (소유자와 이유)

| API | 소유자 | 왜 지금 구현할 수 없는가 | 필요한 선행 작업 |
|---|---|---|---|
| `Server#createWorld(WorldCreator)` | `ferrite-server` 월드 수명주기 + 리전라이저 | 이미 등록된 이름은 그 월드를 그대로 돌려주지만, 새 런타임 월드는 거부한다. Ferrite는 로드된 차원 집합이 고정되어 있고, 새 월드는 청크 소스·Anvil 저장·스폰·리전라이저 등록을 새로 만들어야 한다. | (1) 월드 등록/언로드 수명주기와 `WorldLoadEvent` 순서, (2) 새 차원의 청크 소스·저장·스폰, (3) 리전라이저의 차원 추가/제거 프로토콜. |
| `EntityFactory#createEntitySnapshot` | `ferrite-server` 엔티티 스냅샷 + `ferrite-entity` | 호스트에 접속 플레이어 외 엔티티의 타입·위치·NBT 스냅샷을 주는 경계가 없다. NMS 파싱을 가장하지 않는다. | (1) 리전 소유 엔티티의 읽기 전용 NBT/위치 스냅샷 op, (2) 스냅샷에서 엔티티를 다시 스폰하는 경로, (3) 두 경로의 리전 핸드오프/취소 규칙. |
| `Structure#getEntities`, `Structure#place(..., EntityTransformer)` | `ferrite-server/src/plugins/worldgen.rs` + Java 구조물 | 구조물이 엔티티를 담지 않고, 템플릿 파일의 `entities` 목록도 읽지 않는다. 콜백을 부를 엔티티 자체가 없다. | 아래 "구조물 엔티티 선행 작업 3가지" 참고. |

### 구조물 엔티티 선행 작업 3가지

1. **호스트 비플레이어 엔티티 조회 경계** — 구조물 영역(청크 범위)을 소유한 리전 스레드에서
   플레이어 외 엔티티의 UUID, 타입, 위치/회전, 전체 NBT를 읽는 `worldgen.rs` op가 필요하다.
   현재 `World#getEntities*`는 접속 플레이어만 돌려준다. 구조물이 여러 리전에 걸치면 리전별
   스냅샷을 모아야 하므로 `Signal::EntityTransfer`/`RegionData` 같은 명시적 핸드오프 규칙과
   같은-리전/교차-리전 테스트가 함께 필요하다.
2. **구조물/템플릿의 엔티티 저장** — `CapturedStructure`가 캡처한 엔티티 목록을 보관하고
   `StructureNbt`가 템플릿 파일의 `entities`(pos/blockPos/nbt)를 읽고 써야 하며,
   `Structure#getEntities`/`getEntityCount`가 그 스냅샷을 돌려줘야 한다. `fill(..., true)`도
   엔티티를 수집해야 한다.
3. **변환된 위치에 스폰하는 경로** — `EntityTransformer` 콜백을 소유 리전 스레드에서 호출한 뒤,
   변환된 위치에 NBT로 엔티티를 스폰하는 호스트 op가 필요하다. 회전/반전/integrity가 위치와
   엔티티 NBT에 함께 적용되어야 하고, 콜백이 엔티티를 거부/취소하는 규칙, 구조물이 여러 리전에
   걸칠 때의 스폰 스케줄링도 정해야 한다.

## 그 밖에 남은 명시적 미지원 경로와 소유자

| 영역 | 예시 (정확한 이름) | 소유자 / 선행 작업 |
|---|---|---|
| 다른 구현 인벤토리 | `Player#openInventory(Inventory)` (Ferrite 인벤토리 아님) | Java API (직렬화 불가) |
| 맵 캔버스 | `MapCanvas`의 기본 메서드 | Java `FerriteMapView` |
| 아이템 핸들 | `DroppedItem#<미지원 Item 메서드>` | Java 핸들 |
| HostBridge 반환 변환 | `HostBridge#call`의 미지원 반환 타입 | Java 브리지 |
| BlockData | `BlockData#<미지원 메서드>` (재질별) | Java `FerriteBlockData` |
| 청크 스냅샷/청크 데이터 | `ChunkSnapshot#contains(Biome)`; `ChunkData#getBiome` | 호스트 청크/바이옴 데이터 |
| Paper 생명주기 | `InternalAPIBridge#createPluginLifecycleEventManager` | Paper lifecycle API |
| 포션/인챈트/레지스트리 | `PotionEffectType` 속성(색·카테고리·지속시간 보정·attribute), `Enchantment` 속성, `RegistryAccess` 미지원 레지스트리 | `ferrite-data` 데이터/호스트 |
| 구조물 | `StructureManager#<미지원>`, `Template#<미지원>`, `Structure#getStructureType` | 구조물 월드젠 호스트 |
| NMS/Unsafe | `UnsafeValues`의 NMS 의존 멤버, `CraftItemStack.asNMSCopy`, `CraftWorld#getHandle`, `CraftMagicNumbers` | 의도적 미지원 (가짜 구현 금지); 계속 `NoSuchMethodError` |
| 투사체/타일/장비/효과 | `Player#launchProjectile`(화살 외), `sendBlockUpdate`(표지판 외), `EntityEquipment`(미지원), `Player#playEffect`(데이터 있는 효과) | Rust 호스트/프로토콜 |
| 아이템 메타 | `PlayerInventory#setItem`이 지원하지 않는 메타, 미지원 장비 슬롯 | `ferrite-item` 컴포넌트 |
| 엔티티 조회 | `World#getEntities*`는 접속 플레이어만 반환, 구조물 엔티티 빈 목록 | Folia 리전 소유권 + 호스트 비플레이어 엔티티 조회 |
| 월드젠 호스트 | `locateNearestBiome`, `locateNearestStructure`, `hasStructureAt`, `Registry.STRUCTURE` | vanilla 월드젠 호스트 (EULA 때문에 개발자 로컬만) |

## 생성된 전체 목록 (호스트 없는 폴백 프로브)

<details><summary>Player (106)</summary>

```
addPassenger(Entity)
addResourcePack(UUID,String,byte[],String,boolean)
banIp(String,Date,String,boolean)
banIp(String,Duration,String,boolean)
banIp(String,Instant,String,boolean)
breakBlock(Block)
broadcastHurtAnimation(Collection)
broadcastSlotBreak(EquipmentSlot)
broadcastSlotBreak(EquipmentSlot,Collection)
canCriticalAttack(Entity)
clearActiveItem()
completeUsingActiveItem()
copy()
copy(Location)
createSnapshot()
damage(double)
damage(double,DamageSource)
damage(double,Entity)
fireworkBoost(ItemStack)
getAsString()
getClientBrandName()
getConnection()
getCooldown(ItemStack)
getCooldown(Key)
getCooldown(Material)
getEntityId()
getFirstPlayed()
getLastLogin()
getLastPlayed()
getLastSeen()
getPing()
getProtocolVersion()
getSentChunkKeys()
hasActiveItem()
hasCooldown(ItemStack)
hasCooldown(Material)
hasPlayedBefore()
hideTitle()
isChunkSent(long)
isHandRaised()
isInWorld()
isOnGround()
isTicking()
kill(DamageSource)
knockback(double,double,double)
loadData()
lookAt(double,double,double,LookAnchor)
openAnvil(Location,boolean)
openCartographyTable(Location,boolean)
openEnchanting(Location,boolean)
openGrindstone(Location,boolean)
openLoom(Location,boolean)
openSmithingTable(Location,boolean)
openStonecutter(Location,boolean)
openWorkbench(Location,boolean)
playEffect(EntityEffect)
playHurtAnimation(float)
playPickupItemAnimation(Item,int)
playSound(Entity,Sound,SoundCategory,float,float)
playSound(Entity,Sound,SoundCategory,float,float,long)
playSound(Entity,String,SoundCategory,float,float)
playSound(Entity,String,SoundCategory,float,float,long)
postEffects()
remove()
removeResourcePack(UUID)
removeResourcePacks()
saveData()
sendBlockChange(Location,BlockData)
sendBlockChange(Location,Material,byte)
sendBlockDamage(Location,float,int)
sendEntityEffect(EntityEffect,Entity)
sendEquipmentChange(LivingEntity,EquipmentSlot,ItemStack)
sendExperienceChange(float)
sendExperienceChange(float,int)
sendHealthUpdate(double,int,float)
sendOpLevel(byte)
sendPluginMessage(Plugin,String,byte[])
sendPotionEffectChange(LivingEntity,PotionEffect)
sendPotionEffectChangeRemove(LivingEntity,PotionEffectType)
setCooldown(ItemStack,int)
setCooldown(Key,int)
setResourcePack(String,byte[],String,boolean)
setResourcePack(UUID,String,byte[],Component,boolean)
setResourcePack(UUID,String,byte[],String,boolean)
setRotation(float,float)
setTitleTimes(int,int,int)
showDemoScreen()
showElderGuardian(boolean)
showWinScreen()
sleep(Location,boolean)
spawnAt(Location,SpawnReason)
spawnParticle(Particle,double,double,double,int,double,double,double,double,double,double,Object,boolean,RandomizationType)
startRiptideAttack(int,float,ItemStack)
startUsingItem(EquipmentSlot)
stopAllSounds()
stopSound(SoundCategory)
stopSound(String,SoundCategory)
swingHand(EquipmentSlot,SwingAnimation)
swingMainHand()
swingOffHand()
teleport(Entity)
teleport(Entity,TeleportCause)
transfer(String,int)
updateCommands()
updateInventory()
wakeup(boolean)
```

</details>

<details><summary>Server (5)</summary>

```
createExplorerMap(World,Location,StructureType,Type,int,boolean)
reload()
reloadData()
reloadWhitelist()
sendPluginMessage(Plugin,String,byte[])
```

</details>

<details><summary>World (61)</summary>

```
addEntity(Entity)
createEntity(Location,Class)
createExplosion(Entity,Location,float,boolean,boolean,boolean)
createExplosion(Location,float,boolean,boolean,Entity)
createExplosion(double,double,double,float,boolean,boolean,Entity)
findLightningRod(Location)
findLightningTarget(Location)
generateTree(Location,Random,TreeType)
generateTree(Location,Random,TreeType,Consumer)
generateTree(Location,Random,TreeType,Predicate)
generateTree(Location,TreeType)
generateTree(Location,TreeType,BlockChangeDelegate)
getBlockState(int,int,int)
getChunkCount()
getChunkGenerationState(int,int)
getChunkGenerationStateAsync(int,int)
getChunksAtAsync(int,int,int,int,boolean,Runnable)
getFluidData(int,int,int)
getGameRuleValue(GameRule)
getGameRuleValue(String)
getGameRules()
getHighestBlockYAt(int,int,HeightMap)
getInfiniburn()
getLoadedChunks()
getStructures(int,int)
getStructures(int,int,Structure)
getTickableTileEntityCount()
getTileEntityCount()
getWorldPath()
hasStructureAt(Position,Structure)
isChunkGenerated(int,int)
isChunkInUse(int,int)
isChunkLoaded(int,int)
isGameRule(String)
loadChunk(int,int,boolean)
locateAllPoiInRange(Location,Predicate,int,Occupancy)
locateNearestBiome(Location,int,int,int,Biome[])
locateNearestPoi(Location,PoiType,int,Occupancy)
locateNearestStructure(Location,Structure,int,boolean)
locateNearestStructure(Location,StructureType,int,boolean)
playSound(Entity,Sound,SoundCategory,float,float)
playSound(Entity,Sound,SoundCategory,float,float,long)
playSound(Entity,String,SoundCategory,float,float)
playSound(Entity,String,SoundCategory,float,float,long)
playSound(Location,Sound,SoundCategory,float,float)
playSound(Location,Sound,SoundCategory,float,float,long)
playSound(Location,String,SoundCategory,float,float)
playSound(Location,String,SoundCategory,float,float,long)
refreshChunk(int,int)
save(boolean)
setBiome(int,int,Biome)
setBiome(int,int,int,Biome)
setSpawnLocation(int,int,int,float)
spawn(Location,Class,Consumer,SpawnReason)
spawn(Location,Class,SpawnReason,boolean,Consumer)
spawn(Location,Class,boolean,Consumer)
spawnArrow(Location,Vector,float,float,Class)
spawnParticle(Particle,List,Player,double,double,double,int,double,double,double,double,double,double,Object,boolean,RandomizationType)
unloadChunk(int,int,boolean)
unloadChunkRequest(int,int)
vanillaBiomeProvider()
```

</details>
