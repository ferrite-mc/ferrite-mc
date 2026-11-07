# 플러그인 API 남은 스텁 인벤토리 (2026-11-08, fb-32)

이 문서는 **남아 있는 미지원 호출과 그 소유자**를 기록한다. 두 종류를 섞지 않는다.

1. **생성된 폴백 프로브** — `ferrite.native=false`(호스트 없음)에서 각 API를 기본 인자로 호출해
   `UnsupportedOperationException("Ferrite: not implemented: ...")`이 난 목록이다. 실제 서버에서는
   Rust 호스트가 답하는 메서드도 포함되므로, 이 목록만으로 "구현 안 됨"이라고 단정하지 않는다.
2. **코드에 남은 명시적 미지원 경로** — 구현 파일이 의도적으로 거부하는 메서드다. 소유자와 필요한
   선행 작업을 함께 적는다.

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

## 남은 명시적 미지원 경로와 소유자

| 영역 | 예시 (정확한 이름) | 소유자 / 선행 작업 |
|---|---|---|
| 엔티티 스냅샷 | `EntityFactory#createEntitySnapshot` | Java API + 호스트 엔티티 NBT 스냅샷 |
| 플레이어 respawn | `Player.Spigot#respawn` | Java API + respawn 흐름 |
| 다른 구현 인벤토리 | `Player#openInventory(Inventory)` (Ferrite 인벤토리 아님) | Java API (직렬화 불가) |
| 기간 지정 프로필 밴 | `Player#ban(String,Duration,String,...)` | Rust 밴 목록의 만료 표현 |
| 맵 캔버스 | `MapCanvas`의 기본 메서드 | Java `FerriteMapView` |
| 아이템 핸들 | `DroppedItem#<미지원 Item 메서드>` | Java 핸들 |
| HostBridge 반환 변환 | `HostBridge#call`의 미지원 반환 타입 | Java 브리지 |
| BlockData | `BlockData#<미지원 메서드>` (재질별) | Java `FerriteBlockData` |
| 청크 스냅샷/청크 데이터 | `ChunkSnapshot#getWorldKey`, `getBiome`, `getRawBiomeTemperature`, `contains`; `ChunkData#getBiome` | 호스트 청크/바이옴 데이터 |
| 데이터팩 | `DatapackManager#refreshPacks` | Rust 데이터팩 재로드 |
| Paper 생명주기 | `InternalAPIBridge#createPluginLifecycleEventManager` | Paper lifecycle API |
| 포션/인챈트/레지스트리 | `PotionBrewer#getEffects`, `PotionEffectType`/`Enchantment` 속성, `RegistryAccess` 미지원 레지스트리 | `ferrite-data` 데이터 |
| 루트 테이블 | `LootTable#populateLoot`, `fillInventory` | Rust 루트 + 인벤토리 |
| 구조물 | `StructureManager#<미지원>`, `Template#<미지원>`, `Structure#getStructureType` | 구조물 월드젠 호스트 |
| 구조물 엔티티 | `Structure#place(..., EntityTransformer)` | 비플레이어 엔티티 수집/복원 |
| NMS/Unsafe | `UnsafeValues`의 NMS 의존 멤버, `CraftItemStack.asNMSCopy`, `CraftWorld#getHandle`, `CraftMagicNumbers` | 의도적 미지원 (가짜 구현 금지) |
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
