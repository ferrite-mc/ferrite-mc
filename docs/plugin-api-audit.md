# 플러그인 API 감사 (2026-11-15, plugins-z)

Bukkit/Paper 플러그인 API를 실제 Rust 호스트, Java 구현, 게임 루프, 저장 형식과 대조한 기록이다.
**실제 26.3 클라이언트 화면, EULA 동의 환경, 외부 대형 플러그인 부팅은 검증하지 않았다.**
따라서 “API가 응답한다”와 “바닐라/Paper와 화면·동작이 완전히 같다”를 구분한다.

## 1. 재생성 결과

`java/ferrite-api-impl`에서 `./gradlew test --no-daemon --console=plain`을 실행해
`build/catalog/*.txt`를 다시 만들었다. 이 파일들은 `ferrite.native=false`에서 기본 인자로
호출한 폴백 프로브이며, 같은 이름의 Rust/Java 경로가 있는지는 별도로 문자열·서명을 대조했다.

| 카탈로그 | 줄 수 | 호스트/Java 경로 있음 | 실제로 열림 |
|---|---:|---:|---:|
| Player | 90 | 90 | 0 |
| Server | 4 | 4 | 0 |
| World | 57 | 56 | 1 |
| `stubs.txt` / `player-unoverridden.txt` | 192 | — | — |

Player와 Server에는 호스트/Java 경로가 없는 메서드가 없다. World에서 남은 것은
`vanillaBiomeProvider()` 하나다(컴파일 대상 Paper API에 없어 호출 자체가 드물다).

이 분류는 이름 기준이므로 “경로가 있다”가 모든 오버로드·인자 조합의 성공을 뜻하지 않는다.
실제 서버에서 효과가 확인된 경로는 아래처럼 회귀 테스트가 있는 것에 한정한다.

## 2. 이번에 확인하고 닫은 경로

### 청크 스냅샷

`Chunk#getChunkSnapshot`은 이제 호스트 캡처를 사용한다. `crates/ferrite-server/src/plugins/
worldquery.rs`가 한 시점에 다음을 읽는다.

- 블록 상태(전역 블록 상태 id)
- 하늘 조명과 블록 조명
- 4×4×4 바이옴
- 열별 최고 블록 높이

`FerriteChunkSnapshot.java`가 이 응답을 파싱하고 `FerriteWorld`의 `Chunk` 뷰가 이를 노출한다.
`FerriteChunkSnapshotTest`와 실제 서버+JVM 테스트
`plugin_lifecycle::typical_plugin_...`가 JVM 경로를 확인한다. `World#getEmptyChunkSnapshot`은
기존처럼 전부 공기인 빈 스냅샷을 만든다. `ChunkData#getBiome`은 `createChunkData` 버퍼에 청크
좌표가 없어 계속 `UnsupportedOperationException`이다(가짜 PLAINS를 만들지 않는다).

### 인벤토리 스냅샷 NBT

접속 시 `PlayerSnapshot.inventory_data`가 슬롯별 26.3 아이템 저장 NBT를 base64로 전달한다.
`Bootstrap.setInventorySnapshotNbt`와 `clearInventorySnapshot`이 JVM 쪽 진입점이며,
`FerritePlayer`/`FerritePlayerInventory`가 슬롯을 갱신한다. `FerriteItemNbt.java`가 지원하는
컴포넌트는 다음과 같다.

- `minecraft:custom_name`, `minecraft:lore`
- `minecraft:damage`, `minecraft:repair_cost`, `minecraft:unbreakable`
- `minecraft:enchantments`
- `minecraft:custom_model_data`
- `minecraft:map_id`
- `minecraft:custom_data`(PDC)

지원하지 않는 컴포넌트는 Rust 아이템 NBT에는 남지만 JVM `ItemStack` 뷰에는 반영되지 않는다.
`FerriteItemNbtTest`가 디코드와 JNI 메서드 존재를 확인하고, Rust
`plugin_inventory_snapshot_keeps_components_in_nbt`가 이름·피해·PDC 보존을 확인한다.

### 그 밖에 코드와 일치함을 확인한 경계

- `GameRule`: 59개 규칙을 데이터 기반으로 제공한다. `MemoryKey`만 값 타입/private 생성자 때문에
  거부한다.
- `Chunk#getChunkSnapshot`과 별개로 `MapCanvas`, 구조물 엔티티, `sendBlockUpdate`의 지원 범위는
  각각 `FerriteMapView`, `CapturedStructure`/`snapshot.rs`, `plugins/tile_update.rs`가 소유한다.
- `Player#sendBlockUpdate`: 표지판, 상자·통·셜커 상자·화로 계열, 배너 패턴, 머리 이름/UUID·
  텍스처 속성·`note_block_sound`. 모델이 없는 침대·종·비교기 등은 명시적으로 거부한다.
- 삼지창: `Player#launchProjectile(Trident)`가 실제 삼지창 엔티티를 만들고 비행 종료 시
  아이템으로 떨어뜨린다. 물약 콜백의 내용물 변경은 여전히 지원하지 않는다.
- 지도 제작대(`openCartographyTable`)는 지도 저장소/`idcounts`/타입된 지도 데이터 컴포넌트가
  없어 메뉴 자체를 거부한다.

## 3. 남은 명시적 미지원

- 기본 월드·네더·엔드의 비플레이어 엔티티 저장 NBT 경계. 구조물 엔티티 캡처/재생성은 런타임
  월드에서만 가능하다.
- `MemoryKey` 레지스트리, `ChunkData#getBiome`.
- 지원 표에 없는 아이템 메타와 일부 PDC 컨테이너/리스트 타입.
- 런타임 월드의 커스텀 `WorldCreator#generator`, `generateStructures(false)`, 일부
  `WorldType`/환경. 현재 `NORMAL`, `NETHER`, `THE_END`만 허용한다.
- NMS/Unsafe 의존 API(`CraftItemStack.asNMSCopy`, `CraftWorld#getHandle`,
  `CraftMagicNumbers`)와 Paper 전체 생명주기 API. 가짜 구현을 넣지 않는다.
- `vanillaBiomeProvider()`는 컴파일 대상 Paper API에 없어 계속 열려 있다. 나무 생성·청크 상태·
  타일 수·월드 경로·보이드 피해·구조물 검색은 호스트/Java 경로가 있다.

## 4. 검증과 한계

- **통과:** Java 전체 `./gradlew test` (`BUILD SUCCESSFUL`), Rust `ferrite-server` lib 1237개,
  `plugin_lifecycle` 5개, `plugin_command_world` 2개, `ferrite-plugin` 통합 1개,
  `cargo clippy -p ferrite-server -p ferrite-plugin --all-targets -- -D warnings`,
  `cargo fmt --all --check`.
- **집중 확인:** `FerriteItemNbtTest`, `FerriteChunkSnapshotTest`,
  `plugin_lifecycle::typical_plugin_...`, Rust 인벤토리 NBT 보존 테스트.
- **미검증:** 실제 26.3 클라이언트의 청크 스냅샷/인벤토리 표시, EULA가 설정된 외부 실행 환경,
  CMI 등 대형 플러그인의 전체 부팅. 이 문서는 그 범위를 통과했다고 주장하지 않는다.
