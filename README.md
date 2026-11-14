**한국어** · [English](README.en.md)

# Ferrite

Rust로 작성한 **Minecraft Java 서버**. Folia 스타일 리전 멀티스레딩을 쓰고, 내장 JVM으로
Bukkit/Spigot/Paper/Folia 플러그인을 실행하는 것이 목표입니다.

- **대상**: Minecraft Java **26.3**, 프로토콜 **777**
- **상태 (2026-11-14)**: 생존·게임 시스템과 플러그인 API 일부를 프로토콜 봇과 Rust/Java 테스트로 검증했습니다. 기본 월드는 `world/`에 Anvil 파일로 저장되고 재시작 뒤에도 유지됩니다. **실제 26.3 클라이언트 접속과 전체 플러그인 생태계 호환은 아직 검증하지 않았습니다.**
- **라이선스**: GPL-3.0-or-later

> **English** — Ferrite is a Minecraft Java server in Rust (26.3, protocol 777) with Folia-style
> regionized multithreading and an embedded JVM for Bukkit/Paper plugins. It runs a bot-verified
> survival server and has **not** been tested with a real 26.3 client. No Mojang code or assets are
> distributed; vanilla data is generated locally from the official jar.

## 상태

| 구분 | 내용 |
|---|---|
| ✅ 동작 (봇 검증) | 접속·암호화·서명 채팅, 서버 목록, 리전 멀티스레딩, 월드 IO·조명과 기본 월드 저장(`world/`), 블록·아이템, 인벤토리·제작·레시피북·컨테이너, 생존·전투·몹(바이옴별 자연 스폰, 번식, 늑대 길들이기), 스폰 알, 몹·아이템 추적, 주민 거래, 떠돌이 상인 스폰 규칙·거래 중 수명·라마 동반 생명주기(서버 회귀 테스트), 주크박스·밀랍·로드스톤 같은 블록 사용과 그 발전과제, 레드스톤·호퍼·명령 블록, 양조·비컨, 네더와 포탈, 유체·화재, 명령 대부분(`/give`·`/tick`·`/forceload`·`/place`·`/damage` 등, 상세는 [`docs/COMMANDS-STATUS.md`](docs/COMMANDS-STATUS.md)) |
| 🟡 부분 구현 | 피스톤(움직이는 블록·엔티티 이동과 클라이언트 오버레이; 애니메이션은 실제 클라이언트 미검증, [`docs/pistons.md`](docs/pistons.md)), 엔드 전투(아래, [`docs/end-dimension.md`](docs/end-dimension.md)) |
| 🔧 진행 중 | 플러그인 호환(Paper/Bukkit API 일부, NMS 일부; 미지원 호출은 명시적으로 실패) |
| ⛔ 아직 없음 | 실제 클라이언트 검증 |

**엔드**: 포탈 활성화와 차원 이동, 도착 플랫폼, 바닐라 월드젠 호스트, 공허 판정, 드래곤 보스바·전투·수정 회복/파괴, 출구 포탈과 게이트웨이를 구현했습니다. 드래곤의 선회·돌진(strafe)·착지/대기/이륙·숨결·화염구 행동, 8개 서브 히트박스, 4개 수정 기반 단계별 재소환, 전투/단계 저장·복원, 게이트웨이 생성 빔(`Age` 블록 엔티티; 서버가 나이를 올리고 저장해 재로드 때 빔이 처음부터 다시 시작되지 않음), 첫 귀환 때의 크레딧 신호(`WIN_GAME`)와 "Free the End" 업적도 서버 쪽에는 구현되어 있습니다. 스캔/공격/불꽃 타이밍과 게이트웨이의 2400틱 자동 빔, 사용 중 텔레포트 거부는 26.3 jar와 대조했습니다. 남은 서버 쪽 차이는 착지 후 스캔을 포기할 때의 돌진과 공격 방향, 외곽 섬 탐색 반경, guarded spike 선택입니다. 빔 렌더링, 드래곤 움직임, 보스바, 크레딧 화면 등 실제 클라이언트 화면은 확인하지 않았습니다([`docs/end-dimension.md`](docs/end-dimension.md), 수동 점검표 [`docs/manual-checklist.md`](docs/manual-checklist.md)).

플러그인 API는 Paper `Server`/`Player`의 일부만 처리합니다. 처리하지 않는 호출은
`UnsupportedOperationException("Ferrite: not implemented: …")` 또는 명시적인 `NoSuchMethodError`로
드러납니다. EssentialsX 같은 대형 플러그인의 모든 경로와 NMS 의존 플러그인은 아직 동작하지 않습니다.

### 플러그인 API 현황 (코드 대조: 2026-11-14)

`java/ferrite-api-impl`, `crates/ferrite-server/src/plugins`, 게임 루프와 회귀 테스트를
대조한 결과다. 호스트가 없는 JUnit 폴백 프로브의 `not implemented` 목록은 실제 서버
미구현 목록과 같지 않다. 아래는 호출 가능 여부가 아니라 **게임 상태·패킷·저장에 실제
효과가 있는지**를 기준으로 분류했다. 항목별 상세는 지시서
[`docs/plugin-api-completion-directive.md`](docs/plugin-api-completion-directive.md) §10을 본다.

**구현되어 테스트로 고정된 범위**

- `Server#createWorld`/`unloadWorld`: `WorldId::Runtime`별 청크 소스·리전 스케줄러·
  Anvil 저장을 만들고, 저장·언로드·재오픈한다. 기본 월드는 언로드하지 않으며,
  플레이어가 남아 있거나 `WorldUnloadEvent`가 취소되면 월드를 유지한다. 저장 실패 시
  스케줄러와 카탈로그를 복구하는 트랜잭션 경로도 있다.
- 런타임 월드 재접속: `game/playerdata.rs`가 위치·게임모드·인벤토리·선택 슬롯·탑승
  차량·런타임 월드 UUID·마법부여 시드를 저장하고, 재접속 시 해당 월드가 로드되어 있으면 복원한다.
- 마법부여대: 책장 개수, 플레이어별 시드, 버튼 비용, 가중치 선택과 배타 집합, 단서와 재료 소모를
  바닐라 `EnchantmentMenu` 규칙에 맞췄다. 난수는 바닐라 원본과 같지 않아 같은 시드가 같은 버튼을 내지는 않는다.
- 모루: 같은 아이템 합치기 보너스(최대 내구도의 12%), 이름만 바꾸는 경우의 수리 비용, 창작 모드 상한을 바닐라와 맞춘다.
- 중력 블록·유체·레드스톤: `dragon_egg` 추가, 콘크리트 가루 굳음 규칙, 떨어지는 도중 물에 들어간 착지, 용암 지연 난수,
  압력판의 엔티티 감지와 가중판 출력. 작물 성장은 바닐라와 일치함을 확인했다.
- 청크: 접속 때 클라이언트가 요청한 뷰 거리를 `clamp(요청, 2, 서버)`로 적용하고, 뷰 사각형 밖 청크는 즉시 저장·언로드한다.
- 플러그인 로딩: 플러그인 하나가 로드·활성화에 실패해도 나머지는 계속 로드한다. Vault, EssentialsX, PlaceholderAPI,
  BlueMap, Multiverse-Core, AuthMe, DiscordSRV, SkinsRestorer, ChestShop, Skript 등은 로드·활성화를 확인했다(기능 동작은 아님).
- Paper 부트스트래퍼 `DATAPACK_DISCOVERY`, `help.yml`(`HelpMap`), 레시피 추가·삭제(제작 메뉴에 반영, 클라이언트 레시피북에는 나오지 않음).
- 투사체와 전투: 삼지창 발사와 리프티드 공격(`startRiptideAttack`), `launchProjectile`의 삼지창·눈덩이·달걀·진주·경험치 병·물약·바람 구슬·화염구·폭죽·위더 해골·셜커 탄환·스펙트럴 화살·낚싯찌, 눈 엔더. `canCriticalAttack`, 폭죽 부스트(`fireworkBoost`).
- 수면: `Player#sleep(Location,boolean)`(침대 검증, 점유 확인, 차원별 폭발 처리). 엔드: `getEnderDragonBattle`의 `resetCrystals`, `spawnNewGateway(Position)`.
- `World#createExplosion`(모든 overload, 다른 월드는 거부), `World#spawn`·`createEntity`·`addEntity`(클래스→엔티티 타입 매핑, 모호하면 거부), `getChunksAtAsync`, `Player#getSentChunkKeys`.
- `sendBlockUpdate`: 디스펜서·드롭퍼·호퍼·양조기·크래프터·모닥불·장식된 책장·주크박스·교탁·명령 블록·스포너·스컬크 센서·벌집·구조물 블록 등 블록 엔티티와 머리 텍스처 속성, 아이템 메타 스냅샷.
- 지도 커서 스타일 캡션(`styled`/`numeric` 모드 0·1·2), `CraftMagicNumbers`(`getMaterial`, `getKey`, `getDefaultBlockData`, `isLegacy`, `fromLegacy`).
- `Player#loadData`/`saveData`의 인벤토리·선택 슬롯·게임 모드 적용(체력·허기·경험치는 되돌리지 않음), 기본 월드 엔티티 스냅샷과 `CapturedStructure`/`getStructures`.
- `NmsEntityHandle`: 위치·속도·회전·상태·승객/차량 등을 Bukkit 호출을 거쳐 읽고 쓴다(리전 소유권을 우회하지 않음).
- Worldgen/World API: POI 검색이 기본·네더·엔드·런타임 월드에서 로드된 청크의 실제 블록 상태를 본다(점유 상태 포함). 구조물 시작·`hasStructureAt`·`locateNearestStructure`의 `Structure`/`StructureType`/deprecated overload, `locateNearestBiome`, `setBiome`(오버월드·네더·엔드), 서버 전역 게임룰, 런타임 월드 환경 저장을 구현했다.

**이전 기록과 달라진 점**

- 이전 문서가 미지원으로 적었던 `Player#launchProjectile(Trident)`, 지도 커서 스타일 캡션, 마법부여대의 자리표시자 구현은 지금 구현되어 있다.
- 이전 문서가 미지원으로 적었던 POI `Occupancy`, `StructureType`/deprecated `locateNearestStructure` overload, 비기본 차원의 바이옴·게임룰·로드된 청크 조회는 지금 구현되어 있다.

**한계와 남은 것 (지시서 §10.3)**

- `Player`: 폭죽 전용 핸들 메서드, 폭죽 색·모양 표시, 붙은 폭죽 가속의 근사, `getConnection`의 일부 메서드, `postEffects`로 바꾼 상태의 반영 한계.
- `World`: POI 검색은 POI 저장소 대신 로드된 청크의 블록 상태를 스캔한다(네더·엔드·런타임 월드 포함). POI 티켓을 점유하는 AI가 없어 `IS_OCCUPIED`는 항상 빈 결과이고 `HAS_SPACE`는 해당 타입 중 공간이 있는 POI만 돌려준다. `locateNearestBiome`은 호스트의 y=64·8블록 간격 근사를 쓰고 `horizontalInterval`/`verticalInterval` 인자는 반영하지 않는다. 구조물 조각(`getPieces`)은 비어 있다. `getWorldPath`는 기본 월드만 된다. `getFluidData`, `spawnAt`, `vanillaBiomeProvider`는 컴파일 대상 Paper API에 없다.
- `sendBlockUpdate`: 침대는 26.3에 블록 엔티티가 없어 거부한다. 목록 밖 대상은 `UnsupportedOperationException`.
- 레시피: 조리·대장간·석재 절단 레시피의 런타임 추가가 없다. 결과 아이템 메타, `ExactChoice`, 클라이언트 레시피북 표시도 없다.
- NMS 메뉴: 호퍼·발사기·화로·양조기·신호기·교탁·상인·크래프터는 의도적 거부(이유와 대체 API는 지시서 §10.3).
- `TAGS` 라이프사이클 이벤트는 태그 레지스트리가 시작 후 고정이라 거부한다. `fromLegacy`의 데이터 바이트는 지원하지 않는다.
- 눈 엔더의 속도식은 바닐라 `EyeOfEnder.tick`을 기억에 의존해 옮긴 근사다.
- 청크 틱은 시뮬레이션 거리 설정이 아니라 쥐고 있는 청크 전체에서 돈다. 티켓 종류별 만료(포탈 300틱 등)는 모델링하지 않는다.
- 실제 26.3 클라이언트 화면(메뉴·맵·보스바·파티클·구조물·리소스 팩)과 대형 플러그인의 기능 동작은 확인하지 않았다. ViaVersion류 채널 주입과 TAB 6.2.1은 동작하지 않는다.
- 생존: 체력·최대 체력·흡수 체력·허기·포화도·탈진·자연 회복·섭취·화염·용암·익사·
  낙하·공허·기아·효과 피해·사망·죽음 메시지·경험치/아이템 드롭·`keepInventory`·
  즉시 리스폰을 리전 소유 상태로 처리하고 관련 패킷을 보낸다.
- 인벤토리와 메뉴: 플레이어 인벤토리 저장/복원, 클릭·퀵무브·드래그·핫바 교체·드롭,
  제작·레시피북, 상자/통/화로/양조/비컨/모루/마법부여/분쇄석/석재 절단/대장장이/
  지도 제작대/셜커 상자/말 인벤토리 메뉴를 지원한다. `InventoryDragEvent` 취소 시
  시작 상태를 복원한다.
- `Player#breakBlock`, 플레이어 권한으로 실행하는 `Player#chat("/명령")`,
  `Player#performCommand`, 슬롯 파괴·피격·아이템 줍기 애니메이션과 일부 네트워크
  패킷 API가 실제 리전 명령 또는 클라이언트 패킷으로 연결된다.
- `RegistryAccess`의 데이터 기반 레지스트리, `PotionEffectType`, `playEffect`, 플레이어
  장비 슬롯 6개, 보스바·스코어보드·팀·태그·레시피·맵 API를 구현하고 테스트한다.
- 구조물의 블록·블록 엔티티 캡처/배치, 회전·반전·무결성·`BlockTransformer`, 런타임
  월드의 비플레이어 엔티티 스냅샷과 `EntityTransformer` 배치를 지원한다.

**아직 미지원이거나 제한이 있는 것**

- 구조물 엔티티 스냅샷 열거는 **런타임 월드만** NBT를 제공한다. 기본 오버월드와
  네더·엔드의 비플레이어 엔티티는 NBT 기반 구조물 캡처/재생성 대상에서 제외된다(T7 작업 대상).
- 아이템 메타 중 지원 표에 없는 타입과 지원하지 않는 PDC 타입은 명시적 오류를 낸다.
- `RegistryAccess`의 `GameRule`·`MemoryKey` 값 객체 중 생성 데이터에 타입·기본값이 없는 항목.
- NMS/Unsafe 의존 API(`CraftItemStack.asNMSCopy`, `CraftWorld#getHandle`, `CraftMagicNumbers` 일부)와
  Paper 전체 생명주기 API(§2, §7). 전체 목록과 진행 상태는 지시서 §10.3을 본다.
- 런타임 월드 생성: `NORMAL`, `NETHER`, `THE_END` 환경과 seed를 `level.dat`의 `Data.FerriteDimension`/`Data.WorldGenSettings.seed`에 저장하고 재오픈한다. 커스텀 `WorldCreator#generator`, `generateStructures(false)`, `WorldType`(NORMAL 외), `biomeProvider`, `CUSTOM` 환경은 명시적 `UnsupportedOperationException`으로 거부한다(호스트 월드젠이 커스텀 생성기·구조물 생성 토글·바이옴 소스를 바꾸는 훅을 노출하지 않는다).

상세한 메서드별 소유자·근거·테스트는 [`docs/plugin-api-stubs.md`](docs/plugin-api-stubs.md),
[`docs/plugin-stubs-status.md`](docs/plugin-stubs-status.md), [`docs/plugin-api-audit.md`](docs/plugin-api-audit.md)에
기록한다. 실제 마인크래프트 클라이언트 화면과 실제 대형 플러그인 부팅은 아직 확인하지 않았다.

## 빌드와 실행

필요: **Rust stable**(`rust-toolchain.toml`이 Windows MSVC를 고정, Linux/macOS는 `RUSTUP_TOOLCHAIN=stable`),
**Java 25**, **Python 3**. Windows GUI에는 **Microsoft Edge WebView2 Runtime**이 필요합니다
(Windows 10/11에는 기본 포함).

```sh
source tools/env.sh                        # Git Bash (PowerShell: . .\tools\env.ps1)
python tools/fetch_vanilla.py              # 바닐라 데이터 생성 (최초 1회, 네트워크 + Java 25)
cargo run --release -p ferrite-server      # 오프라인 모드 로컬 서버
```

처음 실행하면 `eula.txt`를 만들고 종료합니다. [Minecraft EULA](https://aka.ms/MinecraftEULA)를 읽고
직접 `eula=true`로 바꾼 뒤 다시 실행하세요. Ferrite가 대신 동의하지 않습니다.

`vanilla/`는 git에 포함되지 않습니다. 패킷 ID·블록·레지스트리 수치는 추측하지 말고
`vanilla/<버전>/generated/` 리포트에서 읽습니다. 설정 키는 [`docs/CONFIG.md`](docs/CONFIG.md).

기본 월드 디렉터리는 서버 작업 폴더의 `world/`입니다. 청크는 플레이 중 메모리 캐시에
유지되고, 60초마다 저장되며 언로드와 정상 종료 때도 Anvil 형식으로 기록됩니다. 엔티티는
로드된 청크별 스냅샷을, 접속 중인 플레이어 상태도 60초마다 저장합니다. 강제 종료 직전의
자동 저장 주기 안 변경분은 유실될 수 있습니다. Windows에서 `ferrite-server.exe`를
직접 실행하면 서버 시작·종료와 명령 입력, 로그 보기를 제공하는 Tauri 2 GUI가 열립니다.
실행 파일은 `vanilla/26.3/generated/`가 준비된 Ferrite 작업 폴더에 두어야 합니다.
`--headless`를 붙이거나 Linux에서 실행하면 GUI 없이 터미널의 표준 입력을 받는
헤드리스 서버로 실행됩니다. GUI 동작과 제약은 [`docs/windows-console.md`](docs/windows-console.md)에
정리되어 있습니다.
처음 실행 전 EULA를 읽고 동의한 경우에만 `eula.txt`를 직접 수정해야 합니다.

## 검증

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

서버 동작은 `crates/ferrite-server/tests`의 프로토콜 봇 세션으로 검증합니다. 2026-11-14 로컬
`main`에서 `cargo +stable test --workspace`, `cargo +stable clippy --workspace --all-targets -- -D warnings`,
`cargo +stable fmt --all -- --check`, `java/ferrite-api-impl`의 Gradle 테스트를 통과했습니다.
최신 CI 상태는 [GitHub Actions](https://github.com/ferrite-mc/ferrite-mc/actions)에서 확인합니다.
봇 테스트 성공은 실제 Minecraft 클라이언트 화면 검증을 의미하지 않습니다.

## 로드맵

| 단계 | 내용 | 상태 |
|---|---|---|
| 1 | 기반: 프로토콜 코덱, NBT, 네트워킹, 리전화 | 구현·자동 테스트 검증 |
| 2 | 월드 IO·이동·블록·채팅, 런타임 월드 생성·언로드·저장 | 구현·봇 검증, 실제 클라이언트 검증 남음 |
| 3 | 리전라이저 벤치마크 | 측정 기록 있음, 지속 검증·최적화 중 |
| 4 | 플러그인 브리지(Bukkit 이벤트·명령·스케줄러·월드 API) | 구현 확대 중, Java·JVM/봇 테스트 있음 |
| 5 | 게임플레이: 인벤토리·엔티티·전투·제작·명령어 | 구현 확대 중, 바닐라 동작 차이와 미구현 시스템 남음 |
| 6 | Paper 기능 동등성 | API 일부 구현·검증 중, 전체 동등성 미완료 |
| 7 | 플러그인 생태계 호환 확대 | 호환 패턴 테스트 진행 중, 실제 플러그인별 검증 필요 |

2026-11-14 기준. 구현 여부와 검증 범위는 [명령어 현황](docs/COMMANDS-STATUS.md)과
[플러그인 API 잔여 목록](docs/plugin-api-stubs.md)에 기록합니다. 실제 Minecraft 클라이언트
화면 검증, Paper/NMS 전체 호환, 모든 플러그인의 정상 동작은 아직 확인하지 않았습니다.

## 크레이트

| 크레이트 | 담당 |
|---|---|
| `ferrite-protocol`, `ferrite-net` | 패킷 코덱, 연결·암호화·로그인 |
| `ferrite-region` | Folia 스타일 리전라이저와 스케줄러 |
| `ferrite-world`, `ferrite-light`, `ferrite-nbt` | 청크·Anvil IO, 조명, NBT |
| `ferrite-entity`, `ferrite-fluid`, `ferrite-inventory`, `ferrite-item`, `ferrite-recipe`, `ferrite-loot` | 엔티티·몹, 유체, 메뉴·아이템·레시피·전리품 |
| `ferrite-command`, `ferrite-admin`, `ferrite-profiler` | 명령 디스패처, 게임룰·권한, `/tps`·`/profile` |
| `ferrite-plugin`, `ferrite-plugin-meta`, `ferrite-jvm` | 플러그인 로딩, `plugin.yml`, 내장 JVM |
| `ferrite-data`, `ferrite-text`, `ferrite-math`, `ferrite-config`, `ferrite-console` | 바닐라 데이터, 텍스트, 수학, 설정, 콘솔 |
| `ferrite-server` | 바이너리: 설정·콘솔·배선 |

## 법적 고지

이 저장소는 **Mojang의 코드나 에셋을 배포하지 않습니다.** 바닐라 데이터는 사용자의 환경에서 공식 서버 jar로
생성되어 `vanilla/`에 저장되고 git에서 제외됩니다. Minecraft는 Mojang Studios의 상표이며, 이 프로젝트는
Mojang·Microsoft와 무관한 비공식 프로젝트입니다.

## 라이선스

GPL-3.0-or-later ([`LICENSE`](LICENSE)). Paper·Folia·Lithium 등 GPL/LGPL 코드를 포팅하기 위해 MIT/Apache에서
GPLv3로 바꿨고, 출처는 [`NOTICE.md`](NOTICE.md)와 각 소스 파일 주석에 적습니다.

## 해야 할 일

- 남은 게임·플러그인 작업: 드래곤 착지 서브페이즈 타이밍과 보라색 텔레포트 빔, 기본 월드·
  네더·엔드의 비플레이어 엔티티 NBT 경계, 삼지창, Paper/Bukkit 및 NMS
  호환 범위 확대.
- 실제 클라이언트 검증: 차원 이동/도착 플랫폼/공허 사망, 엔드 보스바·드래곤·게이트웨이,
  피스톤 애니메이션, 동물 상호작용, 볼트·거래 UI, 플러그인 동작을 통합 서버에서 확인.
- 최적화 조사 참고: [서버 최적화 모드](https://wisehosting.com/blog/10-best-minecraft-server-optimization-mods),
  [최적화 모드팩](https://modrinth.com/modpack/server-optimizationn),
  [최적화 가이드](https://github.com/YouHaveTrouble/minecraft-optimization),
  [Pumpkin Rust 서버](https://github.com/Pumpkin-MC/Pumpkin),
  [Pumpkin PatchBukkit 브리지](https://github.com/Pumpkin-MC/PatchBukkit).
  최적화 조사 메모: [`OPTIMIZATIONS.md`](docs/OPTIMIZATIONS.md).
  최적화와 브리지 설계 참고 자료로 조사하고, 구현·호환성을 그대로 가정하지 않는다.

## 후속 로드맵

- [Advanced Slime World Manager (ASWM)](https://github.com/Paul19988/Advanced-Slime-World-Manager) /
  [Advanced Slime Paper](https://github.com/InfernalSuite/AdvancedSlimePaper) 참고해 Slime Region Format
  월드 로드·저장·전환 및 플러그인 연동을 구현한다.
- **마지막 단계:** Java 호스트를 대체할 Rust 월드젠을 구현한다. 바닐라 26.3 대조와 기존 사용자
  월드젠 호스트 병행을 완료한 뒤 진행한다. 성능 조사에는 [C2ME](https://github.com/RelativityMC/C2ME-fabric),
  [Moonrise](https://github.com/Tuinity/Moonrise), [Noisium](https://github.com/Steveplays28/noisium),
  C2ME의 실험적 [OpenCL 가속 모듈](https://github.com/RelativityMC/C2ME-fabric/tree/dev/26.3.0/c2me-opts-accel-opencl)
  등 월드젠 최적화 프로젝트를 참고한다. 기법과 병목을 조사해 Ferrite에 독립 구현하고, 바닐라 대조·성능
  측정으로 검증한다. OpenCL은 CPU 경로와 결과 동등성 및 장치 호환성을 확인한 뒤 선택적으로 평가한다.

Rust 월드젠을 제외한 작업은 독립된 파일 소유권과 명시적인 인수 조건을 정해 병렬 처리한다.
남은 작업 목록은 [`docs/BACKLOG.md`](docs/BACKLOG.md)가 유일한 기준이다.
