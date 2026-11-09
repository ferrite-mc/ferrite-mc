# 플러그인 API 스텁 현황 (세션 기록)

브랜치 `yxshadxw/plan1104`에서 Spigot/Paper/Folia API 스텁을 구현한 기록이다.
**실제 마인크래프트 클라이언트로 확인한 것은 없다.** 대신 두 가지로 검증했다.

- 패킷 형식: 서버 jar(`vanilla/26.3/lib/server-26.3.jar`)를 `javap`로 열어 보스바, 목표/점수/표시 슬롯, 팀,
  속성 갱신, 쿠키, 리소스팩 응답의 필드 순서와 타입을 대조했다. 팀 패킷만 틀려서 바닐라 26.3 형식으로 고쳤다
  (메서드 0 추가·1 제거·2 변경·3 참가·4 탈퇴, 가시성/충돌은 id, 색은 선택값). 이 오류는 기존 `/team`
  명령에도 있었다.
- 종단 테스트(`plugin_api_calls_reach_the_client_as_packets`): 실제 서버+JVM+테스트 플러그인을 띄우고
  봇 클라이언트가 번개 엔티티·천둥 소리·스코어보드·팀·보스바·장비 패킷을 받는지 확인한다.

단위 테스트(Rust, JUnit)도 있다.
CMI가 동작한다고 말할 수 없다.

## 2026-11-09 — fb-plugin (InventoryDragEvent + Server 경계 정리)

- **`InventoryDragEvent` 발화.** `container.rs`가 `ClickInput::QuickCraft`를 건너뛰지 않는다.
  드래그 시작(status 0)에 메뉴/인벤토리 스냅샷을 잡고, 끝(status 2)에 기존 `quick_craft` 분배를
  적용한 뒤 바뀐 슬롯만 모아 `ServerHooks::inventory_dragged`를 한 번 부른다. 취소되면 스냅샷을
  복원하고 `container_set_content`로 전체 상태를 재전송한다. 트레이트 메서드는 기본 `Ok(true)`라
  기존 구현체가 깨지지 않는다. 호출은 플레이어를 소유한 리전 스레드에서만 일어나고 새 락/대기는 없다.
- 검증: JUnit `BootstrapTest`(9번 슬롯이 포함된 드래그를 리스너가 취소 → `false`, 10번만 있으면
  `true`, 슬롯 집합과 old cursor 확인), `ApiSurfaceTest.inventoryDragEventCarriesSlotsAndCursors`;
  실제 서버+JVM 통합 테스트 `plugin_api_calls_reach_the_client_as_packets`가 봇의 QuickCraft
  드래그 뒤 `TestPlugin/events.txt`에 `drag 9,10`을 확인한다.
- **Server 경계 정리.** help map(명령 맵 기반 + `registerHelpTopicFactory`), recipes(바닐라
  데이터팩), maps(메모리 `MapView`), tags(block/item/fluid/entity_type/game_event)는 기존 경계로
  구현되어 있음을 확인하고 JUnit `serverSurfaceImplementsTheBoundedCallsAndDocumentsTheRest`로
  고정했다. `createWorld`(런타임 월드 없음)와 `EntityFactory#createEntitySnapshot`(비플레이어
  엔티티 NBT 경계 없음)은 구현하지 않았고 소유자/이유/선행 작업을 `docs/plugin-api-stubs.md`에 적었다.
- **구조물 엔티티는 계속 미지원.** `Structure#getEntities`는 빈 목록, `EntityTransformer` 목록은
  명시적 거부다. 필요한 선행 작업 3가지(호스트 비플레이어 엔티티 조회 경계, 템플릿 `entities`
  저장, 변환 위치 스폰 경로)를 `docs/plugin-api-stubs.md`에 정확히 적었다.
- **Paper/NMS 전체 호환이 아니다.** `CraftItemStack.asNMSCopy`, `CraftWorld#getHandle`,
  `CraftMagicNumbers`는 계속 `NoSuchMethodError`이고, 실제 클라이언트로 드래그 화면을 확인한
  것은 아니다.

## 2026-11-08 — fb-32 (Bukkit 보스바 연결 + 스텁 인벤토리)

- **Bukkit `org.bukkit.boss.BossBar`가 실제 클라이언트에 표시된다.** `FerriteBossBar`가 Adventure
  `BossBar`를 소유하고, 뷰어 추가/제거는 `Player#showBossBar`/`hideBossBar`(기존
  `bossBarAdd`/`bossBarRemove` → `PluginPacket::BossBar` 경로)로 나간다. 제목·진행도·색·스타일·
  플래그 변경도 같은 경로로 전달되고, `Server#removeBossBar`는 레지스트리에서 지우기 전에 모든
  뷰어에게 숨긴다. 보스바별 Java 상태만 다루며 월드/엔티티 리전 상태는 읽지 않는다(플레이어별
  패킷 경로라 리전을 직렬화하지 않음).
- 검증: `FerriteBossBarTest`(JUnit, 뷰어/속성/숨김·재표시), `typical_plugin_uses_config_commands_listener_and_repeating_task`
  (실제 서버+JVM+봇이 `/typical surface` 뒤 `BossBar` 추가·제거 패킷 수신), `./gradlew test` 전체 통과.
- 남은 스텁의 정확한 생성 목록과 소유자는 [`docs/plugin-api-stubs.md`](plugin-api-stubs.md)에 있다.
  2026-11-08 프로브 기준 Player 106, Server 5, World 61개가 호스트 없이 실패했고, 이 중 상당수는
  실제 Rust 호스트(`facts.rs`, `network.rs`, `extra.rs`)가 답한다. 실제 서버 기준 남은 경로는
  `plugin-api-stubs.md`의 "남은 명시적 미지원 경로" 표와 아래 "오늘 못한 것"을 따른다.
- **Paper/NMS 호환 전체가 완료된 것은 아니다.** NMS-타입 `CraftItemStack.asNMSCopy`,
  `CraftWorld#getHandle`, `CraftMagicNumbers`는 계속 의도적 미지원이며, 비플레이어 엔티티 조회와
  구조물 엔티티는 호스트 경계가 아직 없다.

## 2026-11-08 — fb-32 후속 (advancement/potion/timed bans)

- **Advancement 표시 정보:** `data/minecraft/advancement`의 `display`를 읽어
  `Advancement#getDisplay`와 `displayName`이 동작한다. frame/title/description/icon/show_toast/
  announce_to_chat/hidden/background를 Paper `AdvancementDisplay`로 돌려준다. 표시가 없는
  advancement는 `getDisplay()`가 null이고, `displayName`은 key 문자열을 쓴다.
- **포션 효과:** `PotionBrewer#getEffects`가 vanilla 26.3의 base/strong/long 효과를 돌려준다.
  `UnsafeValues#getInternalPotionData`를 `FerritePotionTypeData`가 Java에서 답하며, 46개
  `PotionType`의 effect/duration/amplifier와 upgradeable/extendable/max level을 제공한다.
  `PotionEffectType#isInstant`도 instant_health/instant_damage를 Java에서 판별한다.
- **기간 지정 프로필 밴:** `Player#ban(..., Date/Instant/Duration, ...)`가
  `FerriteProfileBanList`에 만료 시각을 저장하고, daemon 만료 스레드가 로컬 밴을 지운 뒤 호스트에
  `pardonProfile`을 보낸다. `setExpiration`/`save`도 스케줄을 갱신하고, 명시적 `pardon`은 예약을
  취소한다.
- 검증: JUnit `FerriteAdvancementsTest`, `FerriteServerObjectsTest`(potion, timed ban), 전체
  `./gradlew test` **170개 통과**. 이번 라운드에 Rust 변경은 없었다.
- 실제 클라이언트에서 advancement 화면·포션 효과 표시·밴 만료를 확인한 것은 아니다.

## 구현됨 (게임에 실제 효과가 있는 것)

- 최대 체력(`setMaxHealth`/`getMaxHealth`): 클라이언트에 `UpdateAttributes`를 보낸다.
- 권한 API 전체: Bukkit `PermissibleBase`에 연결.
- 클라이언트 설정: `getClientOption`, `getLocale`, `getMainHand`, 시야 거리 등.
- 보스바: `showBossBar`/`hideBossBar`, 실시간 갱신, `activeBossBars`. 이름은 일반 텍스트만(색·서식 없음).
- 스코어보드 표시: `setScoreboard`로 list/sidebar/below-name을 클라이언트에 보낸다(변경분만 전송).
- 아이템 드롭/스폰: `World#dropItem`, `dropItemNaturally`, `Player#dropItem` 변형, `give(dropIfFull)`,
  `spawnEntity`(TNT와 게임이 아는 몹), `spawnFallingBlock`, `launchProjectile`(화살만).
  CraftBukkit 호환 캐스트 경로의 `CraftWorld#dropItem`/`dropItemNaturally`(콜백 포함)도 같은
  라이브 월드 드롭 경로로 위임한다 — Java 테스트로 호스트 호출까지 확인했다.
- 엔티티 핸들: 스폰·드롭이 돌려주는 핸들은 실제 엔티티의 id를 가지며 위치·유효성·속도·체력 읽기와
  `remove`/`setVelocity`/`teleport`/`damage`가 실제 엔티티에 닿는다. `Player#attack`은 1점 피해.
- 클라이언트가 가진 청크: `Player#isChunkSent`, `getSentChunks`는 실제로 보낸/버린 청크를 따라간다.
- 광선 추적: `World#rayTraceBlocks`/`rayTraceEntities`/`rayTrace`는 칸 단위로 따라가 정확한 적중 위치·면을 준다. 고체 블록은 꽉 찬 정육면체로 본다(반블록·계단 모양은 모델링하지 않음). 엔티티는 접속한 플레이어만 대상이다. `Player#collidesAt`/`wouldCollideUsing`도 같은 방식이다.
- 바이옴: `World#setBiome`은 `/fillbiome`으로 연결되고(4×4×4 칸 단위, 기본 월드만), `getComputedBiome`은 `getBiome`과 같다.
- `World#sendGameEvent`는 받아서 아무 일도 하지 않는다(스컬크 센서 같은 청취자가 게임에 없다).
- 고친 기존 버그: `Material#getMaxDurability`가 `ClassCastException`을 던졌고, `PlayerInventory#getItem/setItem(EquipmentSlot, …)`가 지원되지 않았다.
- 번개: `strikeLightning`/`strikeLightningEffect`는 번개 엔티티와 천둥 소리를 주변 오버월드 플레이어에게만 보낸다(불·피해 없음, 실제 클라이언트로 미확인).
- 그 밖: `createExplosion`, `updateCommands`, `retrieveCookie`, `storeCookie`, `openMerchant`, `openSign`,
  `setWindowProperty`, 입력/스프린트/수면/낙하 상태 읽기, 리소스팩 상태, 날씨 지속시간 등.

## 읽고 쓰기만 되는 것 (게임이 쓰지 않음)

`setCustomName`, `setCollidable`, `setHealthScale`, `setPVP`, `setHardcore`, `setCanPickupItems`,
청크 티켓/강제 로드 표시, 공허 피해 값, `setBodyYaw`, `setStarvationRate`, `setEnchantmentSeed`,
웨이포인트, 워든 경고 레벨 등 — 플러그인이 넣은 값을 되돌려 줄 뿐 게임 동작은 바뀌지 않는다.

## 알려진 한계

- `createExplosion`의 `setFire`는 폭발 중심에 불 하나만 놓는다(바닐라처럼 여러 곳에 흩뿌리지 않음).
- `openMerchant`의 거래 횟수가 플러그인 레시피에 반영되지 않는다.
- 스코어보드: 접속할 때 메인 스코어보드를 보여 준다. 팀은 `setScoreboard` 때 클라이언트에 보내고 변경을 따라간다(이름 색·충돌·네임태그 설정, 접두/접미사는 일반 텍스트만).
- 핸들 읽기는 소유 리전이 한 틱 뒤 공개하는 값이라, 스폰 직후에는 처음 위치를 돌려줄 수 있다.
- 엔티티 연산은 오버월드만 지원한다.
- 패킷 구조는 바닐라 jar와 대조했지만, 실제 클라이언트가 그려 주는 모습(색·위치·소리 크기)은 확인하지 못했다.

## 오늘 못한 것 (구현 안 됨)

서버 쪽 기능이나 데이터가 먼저 필요하다.

- **연결됨(이번 라운드):** 아이템 사용 상태, 마모 동기화, 탑승·승객(`/ride` 탑승표), `openBook`(제목·저자·일반 텍스트 쪽), `sendMap`(렌더러를 캔버스에 그려 `map_item_data` 전송; 보이는 커서와 일반 텍스트 캡션 전송),
  `refreshChunk`/`getChunkGenerationState`/`unloadChunk`, 기온·습도(바이옴 데이터, 높이 보정은 노이즈 없이), 전투 추적기(호스트가 보고한 피해를 기록, 낙하 위치 종류는 GENERIC),
  눈덩이·달걀·엔더 진주·작은 화염구(플레이어가 던질 수 있고 `launchProjectile`로도 발사; 진주는 같은 리전의 던진 사람만 순간이동 + 낙하 피해 5, 달걀은 닭을 낳지 않음; 작은 화염구는 직선 비행, 착탄 지점의 몹에 5 피해와 100틱 점화, 불 블록 생성).
- **장부만 있는 것:** 어깨 앵무새(`set/get/releaseShoulderEntity*`는 Java에서 기억만 하고 게임엔 앵무새도, 클라이언트 표시도 없다).
- **마지막에 붙인 것:** 큰 화염구(직선 비행, 닿는 곳에서 불 붙는 위력 1 폭발), `findLightningRod/Target`(불러온 열의 맨 위 블록만 보고 128칸 안에서 가장 가까운 피뢰침을 찾는다), `sendBlockUpdate`는 표지판만(다른 타일 상태는 `UnsupportedOperationException`).
- **아이템 메타:** `PlayerInventory.setItem` 등이 이름·로어·인챈트·불괴·커스텀 모델 데이터·수리 비용·내구도를 실제 슬롯 컴포넌트로 보낸다(`setItemMeta`). 아이템 PDC는 `BYTE`·`SHORT`·`INTEGER`·`LONG`·`FLOAT`·`DOUBLE`·`BOOLEAN`·`STRING`·`BYTE_ARRAY`·`INTEGER_ARRAY`·`LONG_ARRAY`를 키와 타입을 보존해 `minecraft:custom_data`로 보내고, `serializeToBytes`/`readFromBytes`도 지원한다. 아이템 플래그는 `minecraft:tooltip_display`로 보내며 `HIDE_ADDITIONAL_TOOLTIP`은 26.3에 개별 컴포넌트가 없어 전체 툴팁 숨김으로 근사한다. 컨테이너/리스트 PDC 타입과 책 같은 나머지 메타는 조용히 버리지 않고 `UnsupportedOperationException`. 실제 클라이언트 렌더링은 미검증.
- **월드젠 쪽:** `World#getStructures(x,z[,Structure])` 구현됨(호스트 op 10 `structure starts` → `getStructureStarts(int,int)` → `GeneratedStructure`, 조각(`getPieces`)은 비어 있음). `createExplorerMap`(Server 3개)은 가장 가까운 구조물을 호스트로 찾아 그 중심의 `MapView`가 달린 FILLED_MAP을 돌려준다(못 찾으면 null). 지형 렌더링과 마커 커서는 없고, 맵 연결(`MapMeta`)은 Java 쪽 메타에만 있어 Rust 아이템(material+amount)로는 전달되지 않는다.
  (구현됨, vanilla 월드젠 호스트 필요: `World#locateNearestBiome`, `locateNearestStructure`, `hasStructureAt`, `Registry.STRUCTURE`.
  호스트가 없으면 "not implemented"를 던진다. 호스트 실행 검증은 EULA 때문에 개발자 로컬에서만 가능.)
- **청크:** `unloadChunk`는 서버가 홀더 없는 청크를 스스로 내리므로 "이미 내려가 있으면 true, 플레이어·스폰이 잡고 있으면 false"로 답한다(강제 언로드는 없음).
  `isChunkGenerated` = 로드됨 또는 리전 파일에 저장됨(오버월드만). `loadChunk`는 월드 뷰로 블록 하나를 읽어 생성을 유도한다.
- 위 API는 호출하면 `UnsupportedOperationException("Ferrite: not implemented: ...")`이 난다.

## 2026-11-09 — 드래그 이벤트 (`InventoryDragEvent`)

- 바닐라 드래그(`QUICK_CRAFT`)는 슬롯마다 클릭 패킷이 오지만 Bukkit은 드래그 전체에 대해
  `InventoryDragEvent`를 한 번 발화한다. `game/container.rs`가 시작(0)/추가(1)/끝(2) 단계를
  모아 끝 단계에서 한 번 보내고, 리스너가 취소하면 클릭을 적용하지 않고 메뉴 전체 상태를
  다시 보낸다(드래그 시작 전 상태 그대로).
- `getRawSlots()`/`getInventorySlots()`는 실제로 드래그한 슬롯을 담는다. 다만 `getCursor()`,
  `getOldCursor()`와 `getNewItems()`의 값은 빈 아이템이다. 드래그 결과 스택을 미리 계산하지
  않으므로 리스너가 `getNewItems()`를 고쳐도 서버에는 적용되지 않는다.
- 검증: Rust 단위 테스트(`drag_buttons_split_into_phase_and_even_spread`), `cargo check`,
  scoped clippy, `./gradlew compileJava`. `./gradlew test`와 실제 26.3 클라이언트 드래그 화면은
  아직 확인하지 않았다.

## 다음 단계 제안

1. 실제 클라이언트로 위 기능(보스바, 스코어보드, 아이템 드롭, 화살)을 확인한다.
2. 실제 플러그인을 넣어 부팅하고 로그의 `not implemented` 이름을 모아 우선순위를 정한다.
3. 화염구 같은 나머지 발사체와 피뢰침 색인을 붙인다.

## 아직 안 한 것 / 알려진 한계

- 작은 화염구 밖의 나머지 투사체는 `UnsupportedOperationException`.
- 엔더 진주는 같은 리전의 던진 사람만 순간이동시킨다(리전 간 이동 없음). 달걀은 맞은 자리에서 병아리가 나오지만(8분의 1 확률, 그중 32분의 1은 네 마리), 눈덩이는 피해를 주지 않는다.
- `sendMap`은 보이는 커서 장식을 보낸다. 숨김 커서는 26.3 decoration 레코드에 visibility 비트가 없어 보내지 않으며, 스타일·번역 캡션은 거부한다. 기후 높이 보정은 노이즈를 무시한다.
- 어깨 앵무새는 장부만 있고 화면에는 안 나온다.
- 아이템 메타: 책 메타처럼 이름·로어·인챈트·불괴·모델·수리비·내구도·PDC·아이템 플래그 밖의 메타를 `setItem`으로 쓰는 경로는 거부한다. PDC는 위 원시/배열 타입만 지원하며, `HIDE_ADDITIONAL_TOOLTIP`은 전체 툴팁 숨김으로 근사한다.
- `unloadChunk` 강제 언로드 없음. `findLightningRod`는 불러온 열의 맨 위 블록만 본다. `sendBlockUpdate`는 표지판만.
- 실제 마인크래프트 클라이언트에서 렌더링은 한 번도 확인하지 못했다(단위·Gradle 테스트만).
- 전체 `cargo test --workspace`에서 `chest_contents_are_shared_saved_and_spilled_when_broken`가 부하 시 한 번 실패했고 단독 실행은 통과했다(불안정 테스트로 의심, 원인 미조사).
- 엔드와 피스톤은 `main`에 병합되어 서버 측 동작과 봇/단위 테스트가 있다. 남은 것은 실제 클라이언트
  화면 검증과 문서에 적힌 근사 동작(드래곤 선회, 게이트웨이 빔, 피스톤 애니메이션)이다.
- **구조물 API:** `StructureManager.createStructure()`가 메모리 구조물을 돌려준다. `fill`(영역 복사), `place`(회전·반전·무결성·팔레트), `getPalettes`, `saveStructure`/`loadStructure`(파일·스트림·키), `deleteStructure`, `getStructureFile`, `copy` 구현됨. 회전·반전 시 `facing`, `axis`, 표지판 `rotation`, 계단/레일 `shape`, 문 `hinge`, 수평 연결 키(`north`/`east`/`south`/`west`)가 함께 돈다. `BlockTransformer`는 회전/반전된 상태에서 호출되고, 블록 엔티티 데이터(상자·표지판 등)는 호스트에서 한 번에 읽어 최종 변환 위치에 `/data merge block`으로 되돌린다. 엔티티는 담지 않으며(플러그인이 플레이어 외 엔티티를 못 본다), 월드가 블록 상태 속성까지 주고받아 `fill`/`place`가 상태를 보존한다. 키로 읽은 템플릿은 그대로 서버(`/place template`)가 놓으며 `fill`은 할 수 없다.
- **구조물 엔티티/`EntityTransformer`:** `Structure#getEntities()`는 항상 빈 목록이고, `fill`과 파일 로드 모두 구조물 영역의 엔티티를 수집하지 않는다. 현재 호스트의 `World#getEntities*`는 접속 플레이어만 돌려주고 비플레이어 엔티티의 타입·위치·NBT 스냅샷을 주는 API가 없으며, 템플릿 파일의 `entities` 목록도 Java 쪽에서 읽지 않는다. 따라서 콜백을 호출할 엔티티가 없어 비어 있지 않은 `EntityTransformer` 목록은 `UnsupportedOperationException`으로 명시적으로 거부한다. 이를 구현하려면 먼저 (1) 호스트의 구조물 영역 비플레이어 엔티티 조회/복원, (2) 템플릿 `entities` 로드와 저장, (3) 변환된 위치에 엔티티를 스폰하는 경로가 필요하다.
