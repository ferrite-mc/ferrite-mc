# Ferrite 플러그인 API 인수인계서

작성일: 2026-11-13  
기준 커밋: `03bf650`  
기준 브랜치 상태: `fix/release-new-tag-checkout`  
기준 문서: [`plugin-api-completion-directive.md`](../plugin-api-completion-directive.md)

## 1. 현재까지 완료된 것

- PR #24, #25, #26, #27을 병합했다.
- PR #25의 `Chunk#getChunkSnapshot(boolean,boolean,boolean)` 동적 프록시 오류를
  `FerriteChunkSnapshot` 반환으로 수정했다.
- README의 플러그인 API 현황을 실제 Java/Rust 코드와 대조해 갱신했다.
- 새 태그를 아직 checkout할 수 없던 Release workflow를 수정했다.
- `v0.3` 릴리즈를 실제로 생성했다.
  - Release run: `43492408134`
  - Ubuntu 빌드 성공
  - Windows 빌드 성공
  - publish 성공
  - 산출물: `Ferrite-v0.3-ubuntu-x86_64.tar.gz`,
    `Ferrite-v0.3-windows-x86_64.zip`
- 남은 플러그인 API를 Claude에게 넘길 수 있도록 정확한 메서드·클래스 단위 지시서를
  [`docs/plugin-api-completion-directive.md`](../plugin-api-completion-directive.md)에
  작성했다.

## 2. 구현 상태를 읽는 방법

호스트 없는 Java catalog의 `not implemented` 결과를 실제 미구현으로 단정하지 않는다.
실제 구현 여부는 다음 세 층을 모두 확인해야 한다.

1. Java API/프록시가 올바른 서명으로 호출하는가.
2. `crates/ferrite-server/src/plugins`에 실제 HostBridge 라우트가 있는가.
3. 게임 루프의 리전 소유 상태, 클라이언트 패킷, 저장/복원이 연결되어 있는가.

현재 `CraftItemStack`, `CraftWorld#getHandle`, `FerriteUnsafe`, `FerriteWorld`,
`PlayerStubs`에는 이미 일부 서명과 래퍼가 있다. 따라서 다음 담당자는 클래스를
새로 만드는 대신 **껍데기·부분 구현을 실제 상태 연결로 완성**해야 한다.

## 3. Claude에게 맡길 전체 잔여 범위

### 3.1 NMS 내부 연결

- `CraftItemStack.asNMSCopy`/`asBukkitCopy`: 현재 호출은 있으나 material/amount 중심이다.
  enchantment, lore, component, PDC, profile, map, potion, book, banner, firework,
  attribute modifier를 양방향 보존한다.
- `CraftWorld#getHandle`: 현재 `ServerLevel` 래퍼가 있으나 실제 청크·엔티티·월드
  저장·리전 명령을 안전하게 연결하지 않는다.
- `CraftMagicNumbers`: Material/BlockData/아이템과 26.3 내부 레지스트리 변환,
  namespaced key, default state, legacy 변환을 완성한다.
- `ServerGamePacketListenerImpl`: 현재 기본 검증과 일부 메뉴 전달만 있다.
  `rows == 0` 메뉴, 모든 menu type, transaction/state id, carried item, slot 동기화,
  열기/닫기 이벤트를 연결한다.
- `FerriteUnsafe`: registry/version 일부는 동작하지만 item/entity JSON·NBT 직렬화,
  `modifyItemStack`, legacy 변환과 실제 Rust 데이터 연결을 완성한다.
- NMS 엔티티 핸들: 위치, 회전, 속도, UUID, 승객/차량, 장비, 속성, metadata,
  spawn/remove, 저장 NBT를 실제 엔티티와 연결한다.
- NMS/Unsafe 호출에 대해 가짜 메모리 주소, 조용한 공기/빈 아이템, 성공형 no-op을
  금지한다.

### 3.2 Player

다음 API는 현재 `PlayerStubs` 선언만 있거나 실제 라우트가 부족하므로 하나씩 구현한다.

- 리소스팩: `addResourcePack`, UUID/hash/prompt/required, 상태 이벤트와 클라이언트 응답.
- 전투/상태: `canCriticalAttack`, `copy`, `copy(Location)`, `fireworkBoost`,
  `sleep`, `spawnAt`, `startRiptideAttack`.
- 연결/상태: `getAsString`, `getConnection`, `getSentChunkKeys`, `loadData`,
  `postEffects`, `getVirtualHost`, `getHAProxyAddress`.
- Paper 데이터: `getMemory`, `getData`, `getDataOrDefault`, `getPickItemStack`.
- 시각/네트워크: 모든 `spawnParticle` overload, 남은 `EntityEquipment`,
  `sendBlockUpdate`의 전체 TileState와 item metadata.
- 투사체: 삼지창을 포함한 `launchProjectile` 지원 종류의 엔티티 생성, 충돌, 피해,
  중력, 픽업, 저장, client packet.

### 3.3 World

- 엔티티/폭발: `addEntity`, `createEntity`, `spawn` 전체 overload,
  `createExplosion` 전체 overload, 블록 피해·엔티티 피해·화염·이벤트·사운드·파티클.
- 번개: `findLightningRod`, `findLightningTarget`.
- 월드젠: `generateTree` 전체 overload, custom generator,
  `generateStructures(false)`, biome provider 조합과 저장/재오픈.
- 청크: `getBlockState`, `getFluidData`, `getChunkCount`,
  `getChunkGenerationState`/async, `getLoadedChunks`, tile entity count,
  `getWorldPath`, `isChunkInUse`, `loadChunk`, `unloadChunk`,
  `unloadChunkRequest`, `getChunksAtAsync`.
- 구조물/POI: `getStructures`, `hasStructureAt`, `locateAllPoiInRange`,
  `locateNearestPoi`, `locateNearestStructure`.
- 바이옴/규칙: `locateNearestBiome`, `setBiome`, `getGameRuleValue`,
  `isVoidDamageEnabled`.
- 현재 `FerriteWorld`에는 일부 즉시 완료 future, 제한된 overload, `NONE`/`null`,
  저장되지 않는 로컬 폴백이 있으므로 실제 비동기·저장·이벤트 동작으로 교체한다.

### 3.4 Item/BlockEntity/Metadata

- 표지판·배너·머리·상자·통·셜커·화로 외 주크박스, 모루, 베드, 벌집,
  스컬크 센서, 구조물 블록, 명령 블록, 스포너 등의 `sendBlockUpdate`.
- 머리 profile properties/textures, 표지판 필터·색·글로우, item slot metadata.
- PDC의 모든 Bukkit/Paper 허용 타입, 중첩 컨테이너·리스트·배열·네임스페이스 key.
- `DataComponentType`, `MemoryKey`, `GameRule` 값 객체와 기본값.

### 3.5 지도/구조물/엔티티

- `MapCanvas` 픽셀/색상/월드/스케일/커서 표시/스타일/번역 caption.
- 숨김 커서와 styled caption의 26.3 map decoration packet.
- 기본 월드·네더·엔드에서도 구조물 엔티티 NBT 캡처/배치.
- `EntityTransformer`, metadata, attributes, passengers/vehicles, shoulder parrot,
  fishing bobber, area-effect-cloud의 패킷·저장·복원.

### 3.6 Paper/Bukkit lifecycle

- `LifecycleEvents.COMMANDS` 외 bootstrap-only lifecycle event.
- plugin.yml help topic과 ignored plugins.
- Bukkit `addRecipe`가 실제 `ferrite-recipe` 제작 매칭에 반영되도록 연결.
- async/region/entity scheduler의 취소, plugin disable, 서버 종료 정리.
- Paper command registration, event priority, lifecycle, reload/disable 경계.

## 4. 구현 시 반드시 남길 증거

각 항목마다 다음을 같은 변경에 포함한다.

- Java API regression test.
- Rust host/game-loop 단위 테스트.
- 가능한 경우 실제 JVM plugin lifecycle 통합 테스트.
- 리전 소유권·패킷·저장/복원 검증.
- 실패 시 `UnsupportedOperationException` 또는 명시적 오류를 유지하되,
  메서드명·이유·대체 API를 문서화한다.

## 5. 전체 완료 검증

```sh
source tools/env.sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

cd java/ferrite-api-impl
./gradlew test --no-daemon --console=plain
```

추가로 실제 26.3 클라이언트에서 메뉴, 맵, 보스바, 파티클, 엔티티 metadata,
구조물, 리소스팩, 월드 이동, NMS 메뉴를 확인한다. 완료 후 이 문서,
`docs/plugin-api-completion-directive.md`, `README.md`의 상태를 함께 갱신한다.
