# 플러그인 API 스텁 현황 (세션 기록)

브랜치 `yxshadxw/plan1104`에서 Spigot/Paper/Folia API 스텁을 구현한 기록이다.
**실제 클라이언트로 확인한 것은 없다.** 테스트는 Rust 단위 테스트와 Java(JUnit) 테스트뿐이다.
CMI가 동작한다고 말할 수 없다.

## 구현됨 (게임에 실제 효과가 있는 것)

- 최대 체력(`setMaxHealth`/`getMaxHealth`): 클라이언트에 `UpdateAttributes`를 보낸다.
- 권한 API 전체: Bukkit `PermissibleBase`에 연결.
- 클라이언트 설정: `getClientOption`, `getLocale`, `getMainHand`, 시야 거리 등.
- 보스바: `showBossBar`/`hideBossBar`, 실시간 갱신, `activeBossBars`. 이름은 일반 텍스트만(색·서식 없음).
- 스코어보드 표시: `setScoreboard`로 list/sidebar/below-name을 클라이언트에 보낸다(변경분만 전송).
- 아이템 드롭/스폰: `World#dropItem`, `dropItemNaturally`, `Player#dropItem` 변형, `give(dropIfFull)`,
  `spawnEntity`(TNT와 게임이 아는 몹), `spawnFallingBlock`, `launchProjectile`(화살만).
- 엔티티 핸들: 스폰·드롭이 돌려주는 핸들은 실제 엔티티의 id를 가지며 위치·유효성·속도·체력 읽기와
  `remove`/`setVelocity`/`teleport`/`damage`가 실제 엔티티에 닿는다. `Player#attack`은 1점 피해.
- 클라이언트가 가진 청크: `Player#isChunkSent`, `getSentChunks`는 실제로 보낸/버린 청크를 따라간다.
- 번개: `strikeLightning`/`strikeLightningEffect`는 번개 엔티티와 천둥 소리를 주변 오버월드 플레이어에게만 보낸다(불·피해 없음, 실제 클라이언트로 미확인).
- 그 밖: `createExplosion`, `updateCommands`, `retrieveCookie`, `storeCookie`, `openMerchant`, `openSign`,
  `setWindowProperty`, 입력/스프린트/수면/낙하 상태 읽기, 리소스팩 상태, 날씨 지속시간 등.

## 읽고 쓰기만 되는 것 (게임이 쓰지 않음)

`setCustomName`, `setCollidable`, `setHealthScale`, `setPVP`, `setHardcore`, `setCanPickupItems`,
청크 티켓/강제 로드 표시, 공허 피해 값, `setBodyYaw`, `setStarvationRate`, `setEnchantmentSeed`,
웨이포인트, 워든 경고 레벨 등 — 플러그인이 넣은 값을 되돌려 줄 뿐 게임 동작은 바뀌지 않는다.

## 알려진 한계

- `createExplosion`은 `breakBlocks=false`를 거절한다(블록 파괴를 끌 수 없음).
- `openMerchant`의 거래 횟수가 플러그인 레시피에 반영되지 않는다.
- 스코어보드: 접속 시 메인 스코어보드는 자동으로 보이지 않는다(`setScoreboard`를 불러야 보인다). 팀은 `setScoreboard` 때 클라이언트에 보내고 변경을 따라간다(이름 색·충돌·네임태그 설정, 접두/접미사는 일반 텍스트만).
- 핸들 읽기는 소유 리전이 한 틱 뒤 공개하는 값이라, 스폰 직후에는 처음 위치를 돌려줄 수 있다.
- 엔티티 연산은 오버월드만 지원한다.
- 패킷 구조(보스바, 스코어보드, 쿠키 등)는 바닐라 형식에 맞춘 것이며 실제 클라이언트로 검증하지 않았다.

## 오늘 못한 것 (구현 안 됨)

서버 쪽 기능이나 데이터가 먼저 필요하다.

- **엔티티 모델 확장 필요:** 아이템 사용 상태(`startUsingItem`, `completeUsingActiveItem`),
  내구도(`damageItemStack`, `applyMending`), 탑승·어깨 엔티티, 전투 추적기, 화살 외 발사체.
- **지형·충돌 데이터 필요:** `rayTrace*`, `collidesAt`, `wouldCollideUsing`.
- **바이옴·구조물 데이터 필요:** `setBiome`, `getComputedBiome`, 기온·습도, `getStructures`(구조물 영역 조회가 호스트에 없다),
  `createExplorerMap`(Server 3개), `generateTree`.
  (구현됨, vanilla 월드젠 호스트 필요: `World#locateNearestBiome`, `locateNearestStructure`, `hasStructureAt`, `Registry.STRUCTURE`.
  호스트가 없으면 "not implemented"를 던진다. 호스트 실행 검증은 EULA 때문에 개발자 로컬에서만 가능.)
- **청크 관리:** `unloadChunk`, `refreshChunk`, `getChunkGenerationState`.
  (구현됨: `isChunkGenerated` = 로드됨 또는 리전 파일에 저장됨(오버월드만). `loadChunk` = 생성돼 있으면 true,
  `generate`면 월드 뷰로 블록 하나를 읽어 생성을 유도한 뒤 true. Java 쪽은 이 환경에 Java 25가 없어 컴파일을 확인하지 못했다.
  청크를 실제로 로드 상태로 붙잡아 두지는 않는다.)
- **기타:** `openBook`(아이템에 written_book 구성요소 없음),
  `sendGameEvent`, `findLightningRod/Target`,
  `sendMap`/`sendBlockUpdate(TileState)`.
- 위 API는 호출하면 `UnsupportedOperationException("Ferrite: not implemented: ...")`이 난다.

## 다음 단계 제안

1. 실제 클라이언트로 위 기능(보스바, 스코어보드, 아이템 드롭, 화살)을 확인한다.
2. 실제 플러그인을 넣어 부팅하고 로그의 `not implemented` 이름을 모아 우선순위를 정한다.
3. 엔티티 연결 장치를 넓힌다(다른 발사체, 아이템 사용 상태), 그 뒤 충돌/바이옴 데이터.
