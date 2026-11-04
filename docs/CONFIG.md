# Ferrite 설정 (`ferrite.toml`)

Ferrite는 실행 디렉터리의 `ferrite.toml`을 읽습니다. 파일이 없으면 모든 값이 기본값으로
동작하고, 파일에 없는 키도 각각 기본값을 사용합니다. 키 이름은 kebab-case입니다.
템플릿은 저장소 루트의 [`ferrite.toml.example`](../ferrite.toml.example)에 있습니다.

| 키 | 타입 | 기본값 | 설명 |
|---|---|---|---|
| `bind-address` | 문자열 (`String`) | `"0.0.0.0:25565"` | 서버가 접속을 받을 주소. `IP:포트` 형식이며, 모든 인터페이스에서 받으려면 `0.0.0.0:<포트>`를 사용합니다. |
| `online-mode` | 불리언 (`bool`) | `true` | `true`면 Mojang 세션 서버로 인증하고(암호화 + `hasJoined` 검증) 정품 계정만 접속할 수 있습니다. `false`면 오프라인 모드로 아무 이름이나 접속할 수 있습니다. |
| `motd` | 문자열 (`String`) | `"A Ferrite server"` | 서버 목록(멀티플레이 화면)에 표시되는 서버 설명 문구입니다. |
| `max-players` | 정수 (`u32`) | `20` | 최대 동시 접속자 수. 상태 응답에 표시되고 접속 제한에 사용됩니다. |
| `view-distance` | 정수 (`u8`) | `10` | 플레이어에게 전송하는 청크 반경(단위: 청크). 실제 적용 시 2~32 범위로 제한됩니다. |
| `supported-versions` | 문자열 (`all` 또는 `native`) | `all` | `all`은 26.3(프로토콜 777)과 번역 가능한 이전 버전(26.2/776)의 접속을 허용하고, `native`는 26.3 클라이언트만 허용합니다. 허용되지 않는 버전은 로그인 단계에서 "Outdated client"로 끊기며, 서버 목록 상태 응답은 항상 코어 버전을 알립니다. |
| `compression-threshold` | 정수 (`usize`) | `256` | 이 크기(바이트) 이상인 패킷을 압축합니다. 값이 작을수록 더 많은 패킷을 압축합니다. |
| `generator` | 문자열 (`superflat` 또는 `vanilla`) | `superflat` | 월드에 없는 새 청크를 만드는 방법. `superflat`은 평지, `vanilla`는 사용자가 받은 공식 서버 jar의 생성기를 별도 JVM 프로세스(월드 생성 호스트)에서 실행합니다. 아래 `[worldgen]` 참고. |

### `[worldgen]` (`generator = "vanilla"`일 때)

| 키 | 타입 | 기본값 | 설명 |
|---|---|---|---|
| `heap-mb` | 정수 (`u32`) | `1024` | 호스트 JVM의 힙(MiB). 512에서도 동작을 확인했습니다. |
| `threads` | 정수 (`u32`) | `4` | 호스트가 동시에 처리하는 청크 요청 수. |
| `jvm-args` | 문자열 배열 | `[]` | 호스트 JVM에 추가로 넘기는 옵션(GC 등). |
| `java-home` | 문자열 | (자동 탐색) | 쓸 Java 25 이상 설치 경로. |
| `seed` | 문자열 | `"0"` | 월드 시드. 숫자나 문자열(`server.properties`와 같은 규칙). |

호스트는 `python tools/fetch_vanilla.py`로 받은 `vanilla/26.3/lib/server-26.3.jar`와
`python tools/build_worldgen_host.py`로 빌드한 `target/worldgen-host/ferrite-worldgen-host.jar`를 씁니다.
호스트의 작업 폴더는 `worldgen/`(git 제외)이고 로그는 `worldgen/host.log`입니다. 설계는
[`docs/worldgen-host.md`](worldgen-host.md)를 보세요.

### EULA (`eula.txt`)

서버는 시작할 때 실행 디렉터리의 `eula.txt`를 확인합니다. 없으면 `eula=false`로 만들고 종료합니다.
Minecraft EULA(https://aka.ms/MinecraftEULA)를 읽고 **직접** `eula=true`로 바꾸세요. Ferrite는 대신
동의하지 않으며, 월드 생성 호스트가 실행하는 공식 서버 jar도 같은 파일을 읽습니다.

참고: `compression-threshold`와 `max-players`는 프로토콜이 `i32`로 표현할 수 있는 범위를
넘으면 서버가 시작 단계에서 오류로 종료됩니다.

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
