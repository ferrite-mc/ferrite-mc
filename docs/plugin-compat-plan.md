# 플러그인 호환 작업 계획서 (이어서 작업용)

## 1. 목표
Spigot / Paper / Folia 플러그인과 NMS(`net.minecraft.*`)에 의존하는 플러그인(예: CMI)이 Ferrite에서 동작하는 것.
기준: "메서드가 호출 가능"이 아니라 **게임에 실제 효과가 반영**되는 것. 구현 여부는 항상 솔직하게 구분해서 보고한다.

## 2. 현재 상태 (main = b671ba9)
- **라우팅 완료:** Player/Server/Scheduler/PluginManager 등 약 1000개 메서드가 `HostBridge`를 거쳐 Rust로 도달한다. 호스트가 처리하지 않으면 `UnsupportedOperationException("Ferrite: not implemented: Owner#method(types)")`.
- **Java 쪽 폴백:** 호스트가 `None`이면 `get/is/has` 조회와 한 인자 `set*`는 값만 저장/반환한다(`HostBridge.localProperty`, 기본값 표 `DEFAULTS`). **클라이언트 효과는 없다.**
- **호스트가 실제 처리하는 것:**
  - Server: broadcastMessage, savePlayers, shutdown, reloadWhitelist, setDefaultGameMode, reload
  - World(기본 월드만): setStorm, setThundering, setTime, setDifficulty, setSpawnLocation, save
  - Player: 포션 add/remove/clear, stopAllSounds, setBedSpawnLocation, 능력치(비행/무적/속도), `damage`(리전 직접 적용), kill, teleport(Entity), lookAt, setRotation, hideTitle, setTitleTimes, sendRawMessage, stopSound, 위치 지정 playSound, spawnParticle(데이터 없는 파티클), showDemoScreen/WinScreen/ElderGuardian, 플레이어별 비
  - 그 외 기존 `Native.*`(채팅, 타이틀, 액션바, 텔레포트, 체력/음식/경험/게임모드, 인벤토리 슬롯, 킥, OP)
- **NMS 호환 레이어(클린룸):** `java/ferrite-api-impl/src/main/java/net/minecraft/**` — ServerPlayer(`connection`), ServerGamePacketListenerImpl, Component, BlockPos, Vec3, ItemStack, Level/ServerLevel, MinecraftServer, GameProfile. `CraftPlayer.getHandle()`, `CraftWorld.getHandle()`, `CraftServer.getServer()`, `CraftItemStack.asNMSCopy` 연결됨. `connection.send`는 채팅/액션바 패킷만 실제 전송, 나머지는 이름이 적힌 예외.
- **도구:** `java/ferrite-api-impl/tools/Scan.java`(플러그인 jar가 필요로 하는데 없는 클래스/멤버 목록), `CatalogTest`(라우팅만 되는 메서드를 `build/catalog/stubs.txt`에 기록; 2026-11 기준 Player 530개).

## 3. 호출 경로 (코드 위치)
Java 스텁 → `HostBridge.callX(owner, subject, "name(Types)", args)` → `Native.hostCall`(인자 `\u001f` 구분, 복합값 `\u001d`, null=`\u001bnull`) → JNI `host_call`(`crates/ferrite-plugin/src/lib.rs`) → 핸들러(`crates/ferrite-server/src/plugins.rs`):
1. `apply_ability` / `damage_amount` / `game_events`: `PlayerCommand`로 게임 루프에 전달 (`game/mod.rs::route_plugin_command` → 리전 `Command::Plugin*` → `region.rs`).
2. `host_action` + `plugins/extra.rs`: 바닐라 콘솔 명령으로 변환 (`settings.console`).
3. 둘 다 아니면 `None` → Java 폴백/예외.

새 처리기를 추가하는 방법:
- 콘솔 명령으로 되면 `plugins/extra.rs`의 `extra_action`에 arm 추가 (모든 값 검증, 대상은 항상 호출 플레이어 UUID).
- 서버 상태를 바꿔야 하면 `PlayerCommand` 변형 → `route_plugin_command` → 리전 `Command` → 패킷 전송, 순서로 추가.
- 반드시 단위 테스트 추가 (AGENTS.md: 코덱 round-trip, 버그 수정은 회귀 테스트).

## 4. 반드시 알아야 할 사실 (이번 세션에서 확인)
- **서버에 있는 콘솔 명령만 쓸 수 있다.** 없는 것: `/damage`(그래서 직접 구현함), `/attribute`, `/transfer`, `/pardon`(pardon-ip만), `/op`·`/deop` 일부. 매핑 전에 `grep 'literal("명령")' crates/ferrite-server/src/game/*.rs`로 확인할 것. 존재: effect, title, tellraw, playsound, stopsound, particle, spawnpoint, setworldspawn, tp, kill, execute, data, team, scoreboard, bossbar, tag, worldborder, setblock, fill, clone, summon, give, clear, enchant, xp, ban, ban-ip, kick, whitelist, weather, time, difficulty, gamemode.
- 호스트 핸들러는 `settings.console`이 있을 때만 등록된다.
- NMS는 Mojang 매핑 이름(26.x 비난독화). 로컬 `vanilla/26.3/versions/26.3/server-26.3.jar`에서 **이름/시그니처만** `javap`로 읽는다. Mojang 코드/에셋은 절대 커밋 금지(`vanilla/`는 gitignore).
- CMI는 유료라 jar가 없다. 가능하면 사용자가 jar를 작업 폴더에 두면 `Scan.java`로 누락 목록을 뽑는다(커밋 금지). 확인 전에는 "CMI가 동작한다"고 말하지 않는다.
- 월드젠은 다른 사람 소유: 건드리지 않는다.

## 5. 로컬 환경 (Windows 기준 AGENTS.md)
```sh
source tools/env.sh                    # PowerShell: . .\tools\env.ps1
cargo test -p ferrite-plugin -p ferrite-server
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cd java/ferrite-api-impl && ./gradlew test      # JDK 25 필요
```
- 리눅스에서는 `RUSTUP_TOOLCHAIN=stable` 지정 (rust-toolchain.toml은 windows-msvc 고정).
- Gradle은 Maven Central 429를 피하려고 PaperMC 저장소를 먼저 두는 init script를 `~/.gradle/init.d/`에 사용했다.
- 워크스페이스 전체 `cargo test`는 최근에 돌리지 않았다. 로컬 픽스처 월드 테스트 4개는 환경 문제로 실패 가능.
- 검증 명령은 `&&`로 이어 붙이고, 커밋 전에 실패를 놓치지 않는다 (c836c06 사고 원인).

## 6. 작업 순서 (우선순위)
1. **측정:** `CatalogTest` 결과와 호스트 처리 목록을 비교해 "처리됨/전체"를 숫자로 보여주는 테스트(또는 스크립트) 추가. 이후 진행률은 이 숫자로 보고.
2. **패킷 배치:** 인벤토리 열기(`openInventory`, `openWorkbench` 등: 기존 `OpenScreen`·menu 코드 재사용), `sendBlockChange*`(`BlockUpdate`, BlockData→state id 변환 필요), `sendPluginMessage`(커스텀 페이로드), 리소스팩, `sendExperienceChange`, 애니메이션(`swing*`, 다른 플레이어 브로드캐스트 필요), 속도/넉백(`SetEntityMotion`).
3. **엔티티 상태:** fire/freeze ticks, glowing, invisible, silent, gravity, 포즈 등 엔티티 메타데이터 패킷. 읽기 질의(`getEntityId`, `getAddress`, `getPing`, `getLocale`, 시간/마지막 접속)는 호스트 상태를 Java 스냅샷으로 내려주는 방식 검토.
4. **서버/월드 쪽 미구현:** 프로필 밴 리스트(`getBanList(PROFILE)`), `getWhitelistedPlayers`, 다중 월드 호출(현재 기본 월드 한정), 시간 제한 밴(`/ban`이 만료를 표현 못함 → 자체 처리).
5. **NMS 확장:** `Scan.java`로 실제 플러그인(EssentialsX, 가능하면 CMI)을 돌려 누락 클래스를 우선순위로 채움. `FerriteWorld`/엔티티를 `CraftWorld`/`CraftEntity` 캐스트 가능하게, `ClientboundXxxPacket` 추가, `ServerPlayer` 필드 확장.
6. **Folia:** 리전 스케줄러 라우팅은 있음. 리전 스레드 규칙(엔티티/월드 접근 스레드 검사) 호환 테스트 추가.
7. **실제 플러그인 테스트:** Spigot/Paper/Folia 대표 플러그인을 `plugins/`에 넣고 부팅 → 로그의 `not implemented` 이름을 모아 다음 배치 입력으로 사용.

## 7. 작업 규칙
- 큰 작업 단위가 끝나고 검증되면 한 번에 커밋/푸시 (작은 단계마다 푸시 금지). 커밋 푸터에 Co-Authored-By / Claude-Session 줄.
- `cargo clippy -D warnings`, `cargo fmt` 유지. 테스트를 건너뛰거나 끄지 않는다.
- 사용자 선호: 중간 보고로 멈추지 않고 이어서 진행, 보고는 한국어, 구현됨/안됨을 정확히 구분.
