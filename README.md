# Ferrite

Rust로 작성된 **Minecraft Java 서버**입니다. Folia 스타일의 리전(region) 기반 멀티스레딩을
목표로 하며, 내장 JVM으로 Bukkit/Spigot/Paper/Folia 플러그인을 그대로 실행하는 것을 최종
목표로 합니다.

- **대상**: Minecraft Java **26.3**, 프로토콜 **777**
- **언어**: Rust (edition 2024)
- **상태**: 봇·Java 대조 테스트로 검증된 생존 기본기 동작. **실제 26.3 클라이언트 접속은 아직
  검증하지 않았습니다.**
- **라이선스**: GPL-3.0-or-later

> **English summary** — Ferrite is a Minecraft Java server written in Rust (target 26.3,
> protocol 777) with Folia-style regionized multithreading and an embedded JVM for
> Bukkit/Paper plugins. It currently runs a bot-verified survival server: login/encryption,
> signed chat, server-list ping, blocks and items, inventory/crafting/chests/furnaces/anvils, villagers and trading,
> hoppers and dispensers, survival, redstone, command blocks, brewing stands and beacons, the nether
> dimension and portals, mobs (passive and hostile) with combat,
> ~70 commands, plugin events, a live Bukkit `Server` surface and a
> Folia-style scheduler, vanilla world generation on a dedicated thread, and a 26.3 → 26.2
> protocol translation layer. **It has not yet been tested with a real 26.3 client.** No Mojang
> code or assets are distributed here; vanilla data is generated locally from the official jar.

## 현재 상태 한눈에

| 구분 | 내용 |
|---|---|
| ✅ **완료 (봇 검증)** | 접속·암호화·서명 채팅, 서버 목록, 리전 멀티스레딩, 월드 IO·조명, 블록/아이템, 인벤토리·제작·컨테이너, 생존, 전투·몹, 주민·거래, 레드스톤·호퍼·디스펜서, 명령 블록, 양조·비컨, 네더 차원·포탈, 유체·화재, 명령 약 70개, 플러그인 브리지, 엔티티 영속화, 바닐라 지형 생성 |
| 🔧 **진행 중** | 월드젠 처리량(1스레드 4.6→6.2, 8스레드 23.5→38.9 chunks/s, 10x 목표 미달), 나무·구조물, 피스톤 |
| ⛔ **아직 없음** | 실제 클라이언트 검증, 엔드 차원, 구조물 |
| 🔬 **검증 수준** | 전체 테스트 105개 바이너리 통과, `clippy -D warnings`·`fmt --check` 클린, 월드젠은 바닐라 Java와 대조 |

## 무엇이 되나

### 접속과 네트워킹
- 오프라인/온라인 로그인(RSA/AES-CFB8, AES-NI), 암호화, 압축
- 서명 채팅, 레지스트리·태그 동기화
- 서버 목록: 실제 접속자 수와 샘플(최대 12명), 64x64 favicon, MOTD, `enforcesSecureChat`
- Velocity 모던 포워딩, BungeeCord 레거시 포워딩
- 연결별 writer task로 패킷 압축·암호화를 틱 스레드 밖에서 처리
- **다중 버전**: `ferrite-translate`가 26.3 ↔ 26.2 패킷·블록 상태·레지스트리를 변환
  (`supported-versions = all | native`). 네이티브 클라이언트는 변환 경로를 타지 않습니다.

### 월드와 게임플레이
- Anvil IO, 청크 패킷 캐시, Starlight 스타일 조명(바닐라 대비 불일치 0)
- 랜덤 틱(잔디 번짐·나뭇잎 소멸), 유체(물·용암), 화재 번짐·점화
- 블록 캐기/설치와 드롭, 아이템 엔티티와 줍기, 실크터치
- 인벤토리 클릭, 2x2·작업대 크래프팅, 상자·통
- **제작대**: 용광로·용해로·훈연기 제련(연료·조리 진행도·경험치), 모루 수리·이름·레벨 비용,
  마법 부여대, 숫돌, 석재 절단기, 대장장이 작업대, 베틀
- **호퍼·디스펜서**: 8틱 아이템 이동(위에서 당기고 앞으로 밀기, 레드스톤 차단, 비교기 출력),
  드로퍼/디스펜서(화살·눈덩이·달걀 발사)
- 게임룰 59종과 저장

### 생존과 전투
- 체력·피해(낙하/익사/용암/불/공허), 허기·포화도, 음식, 사망·리스폰(`keepInventory`), 경험치
- 공격 패킷과 무기 공격 속도·공격력, 치명타(1.5배), 휩쓸기, 넉백, 방패 방어
- **몹**: 프레임워크(체력·피해·전리품·디스폰), 수동 몹(돼지·소·양·닭)과 적대 몹(좀비·스켈레톤·
  거미·크리퍼·엔더맨), 목표 기반 AI, 블록 격자 길찾기, 화살, 몬스터 자연 스폰(밝기·몹 캡),
  `/summon`
- **주민**: 직업과 작업대 연동, UUID별 결정적 거래 목록, 가격·사용 횟수·XP, 상인 메뉴

### 블록과 레드스톤
- 이웃 업데이트와 `canSurvive`, 낙하 블록(모래·자갈·콘크리트 가루·모루)
- 바닐라 폭발 알고리즘(TNT, 크리퍼), 문·레버·침대·표지판 상호작용, 양동이·가마솥
- 와이어 신호 감쇠, 레버·버튼·압력판·횃불(번아웃)·관찰자·반복기·비교기, 램프·문·트랩도어·
  펜스 게이트·노트 블록
- **양조·비컨**: 양조 스탠드(5슬롯, 블레이즈 파우더 연료 20회, 400틱, 바닐라 포션 조합·물병·
  발효 거미 눈·화약·드래곤 브레스), 비컨(피라미드 4단계 판정, 빔, 효과 선택·범위, 재생/주효과)
- **차원**: 오버월드·네더 레지스트리(차원별 저장 경로·스폰·리전 스케줄러), 네더 지형은 자체
  노이즈·바이옴 프리셋으로 생성, `ChangeDimension`/`Respawn` 전환, 8:1 좌표 포탈·포탈 생성·복귀
- **명령 블록**: 임펄스·체인·반복 모드, 조건·자동 실행, 바닐라 NBT(`Command`, `LastExecution` 등)와
  `/data`·청크 저장 왕복, `SetCommandBlock`/`BlockEntityData` 동기화, 재귀 깊이 제한

### 명령 (약 70개)
- 기본: `gamemode`, `tp`, `teleport`, `time`, `weather`, `difficulty`, `gamerule`, `seed`,
  `list`, `say`, `me`, `msg`/`tell`/`w`
- 아이템·상태: `give`, `clear`, `xp`, `effect`, `enchant`, `title`, `tellraw`, `playsound`,
  `particle`, `spawnpoint`
- 월드: `setblock`, `fill`, `clone`, `worldborder`
- 스코어보드: `scoreboard`, `bossbar`, `tag`, `team`
- 서버: `locate`(생물군계·POI, 구조물은 문서화된 스텁), `datapack`(list/enable/disable),
  `help`, `schedule`, `trigger`, `reload`, `op`, `ban`, `whitelist`, `stop`, `function`
- 고급: `execute`(as/at/if/unless/store/run 등), `data`(get/merge/modify/remove),
  데이터팩 `.mcfunction`(load/tick 태그, 매크로, `return`)
- 선택자는 권한 레벨 2 이상, `@e`는 리전 엔티티 저장소에서 type(부정·태그)/distance/
  dx·dy·dz/scores/tag/team/level/gamemode/x_rotation/y_rotation/nbt/limit/sort 해석

### 플러그인 (내장 JVM)
- `plugin.yml`·`paper-plugin.yml` 로딩, Java 25 내장 JVM에서 `onEnable`/`onDisable` 실행
- 이벤트: 플레이어 이동·상호작용·블록 파괴/설치·명령, 인벤토리 클릭/닫기, 사망·리스폰,
  엔티티 피해/사망, 청크 로드/언로드, 날씨·월드 로드, 레벨/경험치(취소 가능한 이벤트는 취소 반영), 오류 정책
  `plugins.on-event-error = allow | deny`
- Bukkit `Server`/`Player`/`World`/`Inventory`/`Scheduler`/`ConfigurationSection`의 자주 쓰는
  표면: 레시피·태그·보스바·월드/블록 조회가 실제 서버 데이터를 반환하고, 체력·음식·경험치·
  텔레포트 쓰기는 소유 리전으로 라우팅됩니다. 최소 `org.bukkit.craftbukkit` shim 포함
- Bukkit 스케줄러는 Folia 규칙대로 리전/글로벌/async 스케줄러로 라우팅
- 월드·플레이어 위치/모드/OP 스냅샷, `Native.playerInventory`(인벤토리 쓰기 → 패킷·메뉴 재동기화)

### 월드 생성
- 밀도 함수, 노이즈 청크, aquifer, 표면 규칙, 동굴·협곡 카버가 바닐라 Java 참조와 일치
- 서버는 전용 스레드에서 바닐라 지형을 생성합니다(`generator = vanilla | superflat`,
  기본값 `vanilla`)
- 광물과 지면 식생 장식 포함

### 관리
- `/tps`, `/mspt`, `/profile`

## 아직 아닌 것 (솔직하게)

- **실제 26.3 클라이언트로 접속해 검증하지 않았습니다.** 지금까지는 프로토콜 봇과 Java
  대조 하네스로만 검증했습니다.
- 월드젠 처리량은 1스레드 6.2 chunks/s, 8스레드 38.9 chunks/s까지 개선됐지만 10x 목표에는
  못 미칩니다(청크 출력은 바이너리와 동일).
- 나무·구조물 생성과 피스톤은 진행 중입니다.
- 엔드 차원은 아직 없습니다(네더는 포탈까지 동작).
- 플러그인 API는 Paper `Server`의 일부만 구현했습니다. EssentialsX 같은 대형 플러그인의 모든
  호출 경로는 동작하지 않고, NMS 타입에 의존하는 메서드는 `UnsupportedOperationException`을
  던집니다.

## 빌드와 실행

필요한 것:

- **Rust stable** — `rust-toolchain.toml`이 Windows MSVC 채널을 고정합니다. Linux/macOS는
  `RUSTUP_TOOLCHAIN=stable`로 덮어쓰세요(CI가 그렇게 합니다).
- **Java 25** — 바닐라 데이터 생성기와 내장 JVM/플러그인 브리지용
- **Python 3** — `tools/fetch_vanilla.py` 실행용

```sh
# Git Bash: Rust가 PATH에 없다면 먼저 실행 (PowerShell: . .\tools\env.ps1)
source tools/env.sh

# 1) 바닐라 데이터 생성 (최초 1회, 네트워크 + Java 25)
python tools/fetch_vanilla.py            # 26.3
python tools/fetch_vanilla.py 26.2       # 다중 버전 변환용(선택)

# 2) 선택: 픽스처 월드 (fixture-world를 읽는 테스트용)
python tools/make_fixture_world.py

# 3) 실행 (오프라인 모드 로컬 서버)
cargo run --release -p ferrite-server
```

`vanilla/`는 git에 포함되지 않습니다. 패킷 ID·블록·레지스트리 같은 수치는 추측하지 말고
`vanilla/<버전>/generated/`의 리포트에서 읽어야 합니다. `fixture-world`가 없으면 픽스처를
읽는 테스트는 건너뜁니다(CI가 그 상태입니다).

설정 키와 자세한 실행 방법은 [`docs/CONFIG.md`](docs/CONFIG.md)에 정리되어 있습니다.

## 검증

```sh
cargo test --workspace                          # 전체 테스트
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

- 최근 전체 실행: 전체 테스트 통과, clippy·fmt 클린.
- 월드젠은 `crates/ferrite-worldgen/tests/java`의 Java 대조 하네스로 바닐라와 비교합니다.
- 서버 동작은 `crates/ferrite-server/tests`의 프로토콜 봇 세션으로 검증합니다.
- `ferrite-translate`의 26.2 검증은 `vanilla/26.2` 리포트가 있을 때만 실행됩니다.
- CI(`.github/workflows/ci.yml`)는 Linux/Windows에서 같은 스크립트로 데이터를 생성한 뒤
  fmt/clippy/test를 돌리고, 바닐라 데이터가 커밋되지 않았는지도 확인합니다.

## 로드맵

| 단계 | 내용 | 상태 |
|---|---|---|
| 1 | **기반** — 워크스페이스, 프로토콜 코덱, NBT, 네트워킹/로그인, 리전화 뼈대 | 완료 |
| 2 | 월드 IO/청크/이동/블록/채팅 — 접속해 플레이 가능한 최소 서버 | 완료 (봇 검증) |
| 3 | 리전라이저 + 벤치마크 — Paper/Folia 대비 성능 측정 | 진행 중 |
| 4 | 플러그인 브리지 — 내장 JVM에서 Bukkit 이벤트/커맨드/스케줄러 실행 | 진행 중 |
| 5 | 게임플레이 — 인벤토리, 엔티티, 전투, 크래프팅, 컨테이너 | 진행 중 |
| 6 | 바닐라 동일 월드 생성 — 지형/구조물/시드 호환 | 진행 중 |
| 7 | 다중 버전 지원 — 하나의 바이너리로 여러 프로토콜 버전 | 진행 중 |
| 8 | Paper 기능 동등성 — Paper API/설정/최적화 정렬 | 예정 |
| 9 | 호환성 확장 — 플러그인 생태계 커버리지 확대 | 예정 |
| 10 | **선택** — 청크 렌더링 OpenCL GPU 가속(메시 빌드·조명·인코딩을 GPU로) | 미착수 (선택) |

## 크레이트 구성

| 크레이트 | 담당 |
|---|---|
| `ferrite-protocol` | `Encode`/`Decode`, `VarInt`, 프레이밍, 패킷 구조체 |
| `ferrite-net` | tokio 연결, 암호화, 로그인/설정/플레이, 서버 목록 |
| `ferrite-region` | Folia 스타일 리전라이저와 리전 스케줄러 |
| `ferrite-world` | 청크, 팔레트 컨테이너, Anvil IO |
| `ferrite-light` | Starlight 스타일 조명 엔진 |
| `ferrite-worldgen`, `ferrite-noise` | 밀도 함수, 노이즈, 청크 생성, 표면 규칙 |
| `ferrite-entity`, `ferrite-fluid` | 리전 로컬 엔티티 저장소와 몹, 유체 |
| `ferrite-translate` | 다중 버전 프로토콜 변환 계층 |
| `ferrite-inventory`, `ferrite-item`, `ferrite-recipe`, `ferrite-loot` | 메뉴, 아이템, 레시피, 전리품 |
| `ferrite-command`, `ferrite-admin` | 브리가디어식 명령 디스패처, 게임룰, 권한/관리 |
| `ferrite-plugin`, `ferrite-plugin-meta`, `ferrite-jvm` | 플러그인 로딩, `plugin.yml`, 내장 JVM |
| `ferrite-profiler` | 틱 프로파일러(`/tps`, `/mspt`, `/profile`) |
| `ferrite-nbt`, `ferrite-data`, `ferrite-text`, `ferrite-math`, `ferrite-config`, `ferrite-console` | NBT, 바닐라 데이터, 텍스트, 수학, 설정, 콘솔 |
| `ferrite-server` | 바이너리: 설정, 콘솔, 배선 |

## 법적 고지

이 저장소는 **Mojang의 코드나 에셋을 배포하지 않습니다.** 바닐라 데이터(패킷 리포트,
블록/레지스트리 목록, 월드젠 데이터 등)는 사용자의 로컬 환경에서 공식 서버 jar를 내려받아
생성되며 `vanilla/`에 저장되고 git에서 제외됩니다. 저장소에는 그 데이터를 읽는 코드만
포함됩니다. Minecraft는 Mojang Studios의 상표이며, 이 프로젝트는 Mojang 또는 Microsoft와
아무런 제휴 관계가 없는 비공식 프로젝트입니다.

## 라이선스

GPL-3.0-or-later. 전체 내용은 [`LICENSE`](LICENSE)에 있습니다.
Paper, Folia, Lithium 등 GPL/LGPL 프로젝트의 코드를 직접 포팅하기 위해 MIT/Apache에서
GPLv3로 바꿨습니다. 포팅한 코드의 출처는 [`NOTICE.md`](NOTICE.md)와 각 소스 파일 주석에 적습니다.
