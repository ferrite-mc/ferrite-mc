# 플러그인 API 감사 (2026-11-12, W6 플러그인 API 레인)

기준: 브랜치 `w6-bukkit-api`. 이 문서는 `docs/plugin-api-stubs.md`/`plugin-stubs-status.md`와 함께 읽는다.

## 방법과 한계

1. `./gradlew test`가 만드는 `build/catalog/*-missing.txt`(호스트 없이 호출해 "not implemented"가 난 메서드)를 뽑았다:
   Player 102, World 71, Server 4.
2. 각 서명 문자열이 Rust 크레이트(`crates/**/*.rs`)에 정확히 나오는지 문자열 일치로 걸렀다. **이 걸러내기는 휴리스틱이다**:
   Rust가 `starts_with("createExplosion(")`처럼 접두사로 받거나 Java가 다른 서명으로 호출하는 메서드는
   "호스트 처리기 없음"으로 잘못 분류될 수 있다. 아래 목록은 "확인 필요 후보"이지 확정된 미구현이 아니다.
3. Java 쪽 의도적 거부 `UnsupportedOperationException`은 114곳이며 대부분 이유가 적힌 정밀 거부다(파일별 개수는 맨 아래).

## 이 레인에서 구현한 항목

| 항목 | 커밋 | 테스트 |
|---|---|---|
| `InventoryType.SHULKER_BOX` 플러그인 인벤토리 열기 (`OpenInventory.shulker`) | `7c642e6` | Rust `opens_a_plugin_shulker_box_inventory_as_a_shulker_menu`, Java `aPluginShulkerBoxInventoryIsSentToTheHostAsAShulkerBox` |
| 서버 lib 테스트 11개 실패 | (코드 변경 없음) | 원인: 워크트리에 gitignore된 `vanilla/26.3/fixture-world`가 없었음. 심볼릭 링크 후 11개 모두 통과 |
| `World#spawnArrow(Location,Vector,float,float,Class)` | `0a147eb` | `spawnArrowShootsAlongTheNormalisedDirectionAtTheGivenSpeed` |
| `World#getInfiniburn` (차원 타입의 `infiniburn` 블록 태그) | `0a147eb` | `infiniburnIsTheDimensionTypesBlockTag` |

## 남은 Player 후보 (호스트 처리기 문자열 없음)

```
addResourcePack(UUID,String,byte[],String,boolean)
canCriticalAttack(Entity)
copy()
copy(Location)
fireworkBoost(ItemStack)
getAsString()
getConnection()
getSentChunkKeys()
loadData()
postEffects()
setResourcePack(UUID,String,byte[],Component,boolean)
setResourcePack(UUID,String,byte[],String,boolean)
sleep(Location,boolean)
spawnAt(Location,SpawnReason)
spawnParticle(Particle,double,double,double,int,double,double,double,double,double,double,Object,boolean,RandomizationType)
startRiptideAttack(int,float,ItemStack)
```

## 남은 World 후보 (호스트 처리기 문자열 없음)

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
getLoadedChunks()
getStructures(int,int)
getStructures(int,int,Structure)
getTickableTileEntityCount()
getTileEntityCount()
getWorldPath()
hasStructureAt(Position,Structure)
isChunkInUse(int,int)
isVoidDamageEnabled()
loadChunk(int,int,boolean)
locateAllPoiInRange(Location,Predicate,int,Occupancy)
locateNearestBiome(Location,int,int,int,Biome[])
locateNearestPoi(Location,PoiType,int,Occupancy)
locateNearestStructure(Location,Structure,int,boolean)
locateNearestStructure(Location,StructureType,int,boolean)
setBiome(int,int,Biome)
spawn(Location,Class,Consumer,SpawnReason)
spawn(Location,Class,SpawnReason,boolean,Consumer)
spawn(Location,Class,boolean,Consumer)
spawnParticle(Particle,List,Player,double,double,double,int,double,double,double,double,double,double,Object,boolean,RandomizationType)
unloadChunk(int,int,boolean)
unloadChunkRequest(int,int)
vanillaBiomeProvider()
```

Server는 4개가 호스트 없이 실패하지만 모두 Rust 처리기가 있다.

## Java 쪽 명시적 거부 개수 (파일별)

```
dev/ferrite/server/FerriteWorld.java:17
dev/ferrite/server/FerriteEnchantmentData.java:12
dev/ferrite/server/PlayerMisc.java:8
dev/ferrite/server/CapturedStructure.java:8
dev/ferrite/server/FerriteServer.java:5
dev/ferrite/server/DroppedItem.java:5
dev/ferrite/server/Bootstrap.java:5
dev/ferrite/server/FerriteMusicInstrument.java:4
dev/ferrite/server/PlayerExtras.java:3
dev/ferrite/server/HostBridge.java:3
dev/ferrite/server/FerriteUnsafe.java:3
dev/ferrite/server/FerriteStructure.java:3
dev/ferrite/server/FerritePotionEffectData.java:3
dev/ferrite/server/FerriteMapView.java:3
dev/ferrite/server/FerriteEntitySnapshot.java:3
dev/ferrite/server/FerriteEffectData.java:3
dev/ferrite/server/StructureEntity.java:2
dev/ferrite/server/SpawnedEntity.java:2
dev/ferrite/server/FerriteTemplate.java:2
dev/ferrite/server/FerriteStructureManager.java:2
dev/ferrite/server/FerritePlayerInventory.java:2
dev/ferrite/server/FerritePlayer.java:2
dev/ferrite/server/FerriteInventoryView.java:2
dev/ferrite/server/FerriteChunkSnapshot.java:2
net/minecraft/server/network/ServerGamePacketListenerImpl.java:1
dev/ferrite/server/WorldEntities.java:1
dev/ferrite/server/FerriteTags.java:1
dev/ferrite/server/FerriteRegistryAccess.java:1
dev/ferrite/server/FerriteProfileBanList.java:1
dev/ferrite/server/FerritePotionEffectType.java:1
dev/ferrite/server/FerriteInternalBridge.java:1
dev/ferrite/server/FerriteEntityFactory.java:1
dev/ferrite/server/FerriteEnchantment.java:1
dev/ferrite/server/FerriteDataContainer.java:1
dev/ferrite/server/FerriteCrafting.java:1
dev/ferrite/server/FerriteBlockData.java:1
```
