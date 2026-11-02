# Ferrite 설정 (`ferrite.toml`)

Ferrite는 실행 디렉터리의 `ferrite.toml`을 읽습니다. 파일이 없으면 모든 값이 기본값으로
동작하고, 파일에 없는 키도 각각 기본값을 사용합니다. 키 이름은 kebab-case입니다.
템플릿은 저장소 루트의 [`ferrite.toml.example`](../ferrite.toml.example)에 있습니다.

| 키 | 타입 | 기본값 | 설명 |
|---|---|---|---|
| `bind-address` | 문자열 (`String`) | `"0.0.0.0:25565"` | 서버가 접속을 받을 주소. `IP:포트` 형식이며, 모든 인터페이스에서 받으려면 `0.0.0.0:<포트>`를 사용합니다. |
| `online-mode` | 불리언 (`bool`) | `true` | `true`면 Mojang 세션 서버로 인증하고(암호화 + `hasJoined` 검증) 정품 계정만 접속할 수 있습니다. `false`면 오프라인 모드로 아무 이름이나 접속할 수 있습니다. |
| `enforce-secure-profile` | 불리언 (`bool`) | `true` | 정품 서버에서 서명된 채팅(프로필 키)을 요구합니다. 오프라인 모드에서는 무시됩니다. |
| `motd` | 문자열 (`String`) | `"A Ferrite server"` | 서버 목록(멀티플레이 화면)에 표시되는 서버 설명 문구입니다. |
| `chat-spam-threshold-seconds` | 정수 (`i32`) | `10` | 미통합(pending) 설정. 비OP 플레이어의 반복 채팅 허용 시간(초)입니다. 기본값 10초에서는 연속 9개 메시지까지 허용되고 10번째에 `Kicked for spamming`으로 접속이 끊깁니다. OP는 면제되며, `0` 이하이면 제한이 비활성화됩니다. |
| `command-spam-threshold-seconds` | 정수 (`i32`) | `10` | 미통합(pending) 설정. 비OP 플레이어의 반복 명령 허용 시간(초)으로, 명령 실행 후 검사됩니다. 기본값 10초를 넘기면 `Kicked for spamming`으로 끊깁니다. OP는 면제되며, `0` 이하이면 제한이 비활성화됩니다. |
| `server-icon` | 문자열 경로 (`PathBuf`) | `"server-icon.png"` | 서버 목록 아이콘(64x64 PNG). 파일이 없거나 잘못되면 경고만 남기고 아이콘 없이 시작합니다. |
| `max-players` | 정수 (`u32`) | `20` | 최대 동시 접속자 수. 상태 응답에 표시되고 접속 제한에 사용됩니다. |
| `view-distance` | 정수 (`u8`) | `10` | 플레이어에게 전송하는 청크 반경(단위: 청크). 실제 적용 시 2~32 범위로 제한됩니다. |
| `supported-versions` | 문자열 (`all` 또는 `native`) | `all` | `all`은 26.3(프로토콜 777)과 번역 가능한 이전 버전(26.2/776)의 접속을 허용하고, `native`는 26.3 클라이언트만 허용합니다. 허용되지 않는 버전은 로그인 단계에서 "Outdated client"로 끊기며, 서버 목록 상태 응답은 항상 코어 버전을 알립니다. |
| `compression-threshold` | 정수 (`usize`) | `256` | 이 크기(바이트) 이상인 패킷을 압축합니다. 값이 작을수록 더 많은 패킷을 압축합니다. |
| `forwarding` | 문자열 (`none`, `velocity`, `bungee`) | `none` | 프록시 플레이어 전달 방식. `velocity`/`bungee`를 쓰면 `online-mode`는 무시되고 인증은 프록시가 담당합니다. |
| `forwarding-secret` | 문자열 (`String`) | `""` | Velocity 모던 포워딩의 비밀 값. `forwarding = "velocity"`이면 필수이며 프록시 설정과 같아야 합니다. |
| `world` | 문자열 경로 (`PathBuf`, 선택) | `None` | Anvil 월드 디렉터리. 설정하지 않으면 메모리 월드를 사용합니다. |
| `generator` | 문자열 (`vanilla` 또는 `superflat`) | `vanilla` | 새 청크 생성기. `superflat`은 테스트용 평지 월드입니다. |
| `gamemode` | 문자열 (`survival` 또는 `creative`) | `creative` | 새 플레이어의 기본 게임 모드입니다. |
| `ops` | 배열 | `[]` | 운영자 UUID 목록. 각 항목은 UUID 문자열 또는 `{ uuid, level }`(레벨 0~4, 기본 4)입니다. |
| `plugins-dir` | 문자열 경로 (`PathBuf`) | `"plugins"` | 플러그인 jar를 읽을 디렉터리입니다. |
| `plugin-bridge` | 문자열 경로 (`PathBuf`, 선택) | `None` (→ `java/ferrite-api-impl/build/libs/ferrite-api-impl-all.jar`) | 내장 JVM 브리지 jar 경로입니다. |
| `whitelist` | 불리언 (`bool`) | `false` | `whitelist.json`에 있는 플레이어만 접속하도록 합니다. |
| `plugins.on-event-error` | 문자열 (`allow` 또는 `deny`) | `allow` | 플러그인 이벤트 처리 중 오류 정책. `allow`는 동작을 계속하고, `deny`는 동작을 막습니다. |

참고: `compression-threshold`와 `max-players`는 프로토콜이 `i32`로 표현할 수 있는 범위를
넘으면 서버가 시작 단계에서 오류로 종료됩니다.

`chat-spam-threshold-seconds`와 `command-spam-threshold-seconds`는 아직 `main`에 통합되지 않은
`yxshadxw/freebuff2-command-blocks` 브랜치에서 추가된 설정입니다. 해당 브랜치가 통합되기 전까지
`main` 빌드는 이 키를 인식하지 않으므로(무시) 설정해도 동작이 바뀌지 않습니다.

## 실행 방법

```sh
source tools/env.sh                 # Git Bash (PowerShell: . .\tools\env.ps1)
cp ferrite.toml.example ferrite.toml
cargo run --release -p ferrite-server
```

- `ferrite.toml`은 **실행 시의 작업 디렉터리**에서 읽습니다. 보통 저장소 루트에서
  `cargo run`을 실행하므로 저장소 루트에 두면 됩니다.
- 로컬 테스트(정품 인증 없이 접속)라면 `ferrite.toml`에 `online-mode = false`를 설정하고
  클라이언트에서 `localhost`로 접속하세요. 이 설정은 인증을 하지 않으므로 공개 서버에서는
  사용하지 마세요.
- 로그 수준은 환경 변수 `RUST_LOG`로 조정할 수 있습니다(기본값 `info`, 예: `RUST_LOG=debug`).
