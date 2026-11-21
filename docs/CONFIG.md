# Ferrite 설정 (`ferrite.toml`)

Ferrite는 실행 디렉터리의 `ferrite.toml`을 읽습니다. 파일이 없으면 기본 설정 파일이 생성되며,
파일에 없는 키도 각각 기본값을 사용합니다. 키 이름은 kebab-case입니다.
템플릿은 저장소 루트의 [`ferrite.toml.example`](../ferrite.toml.example)에 있습니다.

| 키 | 타입 | 기본값 | 설명 |
|---|---|---|---|
| `bind-address` | 문자열 (`String`) | `"0.0.0.0:25565"` | 서버가 접속을 받을 주소. `IP:포트` 형식이며, 모든 인터페이스에서 받으려면 `0.0.0.0:<포트>`를 사용합니다. |
| `online-mode` | 불리언 (`bool`) | `true` | `true`면 Mojang 세션 서버로 인증하고(암호화 + `hasJoined` 검증) 정품 계정만 접속할 수 있습니다. `false`면 오프라인 모드로 아무 이름이나 접속할 수 있습니다. |
| `enforce-secure-profile` | 불리언 (`bool`) | `true` | 정품 모드에서 서명된 채팅(프로필 키)을 요구합니다. |
| `motd` | 문자열 (`String`) | `"A Ferrite server"` | 서버 목록(멀티플레이 화면)에 표시되는 서버 설명 문구입니다. |
| `server-icon` | 경로 (`PathBuf`) | `"server-icon.png"` | 64×64 PNG 서버 아이콘 경로입니다. |
| `max-players` | 정수 (`u32`) | `20` | 최대 동시 접속자 수. 상태 응답에 표시되고 접속 제한에 사용됩니다. |
| `view-distance` | 정수 (`u8`) | `10` | 플레이어에게 전송하는 청크 반경(단위: 청크). 실제 적용 시 2~32 범위로 제한됩니다. |
| `compression-threshold` | 정수 (`i32`) | `256` | 이 크기(바이트) 이상인 패킷을 압축합니다. `-1`이면 유선 압축을 끄고, `0`이면 모든 패킷을 압축합니다. |
| `world` | 경로 (`PathBuf`) | `"world"` | 월드 저장 경로(`storage = "anvil"`이면 디렉터리, `"fworld"`이면 `<world>.fworld` 블롭 파일). |
| `generator` | 문자열 (`rust` 또는 `superflat`) | `"rust"` | 청크 생성기. `"rust"`는 `ferrite-worldgen` 기반 순수 Rust 병렬 월드젠(Overworld/Nether/End)을 사용하고, `"superflat"`은 평지 월드를 생성합니다. |
| `storage` | 문자열 (`memory`, `fworld`, `anvil`) | `"memory"` | 월드 저장 백엔드. `"memory"`는 인메모리 전용(디스크 미저장), `"fworld"`는 `xxh32` 무결성 검증 단일 압축 블롭(`.fworld`) 전체를 RAM에 적재, `"anvil"`은 표준 리전 파일(`mca`)을 사용합니다. |
| `compression` | 문자열 (`auto`, `lz4`, `zlib`, `gzip`, `none`) | `"auto"` | 청크 압축 방식. `"auto"`는 Anvil에서 `zlib`, `.fworld`에서 `lz4`를 선택합니다. |
| `threads` | 정수 (`usize`) | `0` (`auto`) | Tokio 런타임 및 `ferrite-region` 스케줄러 워커 스레드 수. `0` 또는 생략 시 시스템 병렬도(CPU 코어 수)를 사용합니다. |
| `lighting-pool` | 불리언 (`bool`) | `false` | `ferrite-light`의 `LightPool`을 이용한 배치 병렬 라이팅 및 생성 청크 초기 라이트 계산을 활성화합니다. |
| `forwarding` | 문자열 (`none`, `velocity`, `bungee`) | `"none"` | 프록시 플레이어 포워딩 방식. |
| `whitelist` | 불리언 (`bool`) | `false` | `whitelist.json` 사용 여부. |

### `[worldgen]`

| 키 | 타입 | 기본값 | 설명 |
|---|---|---|---|
| `seed` | 문자열 | `"0"` | 순수 Rust 월드젠(`ferrite-worldgen`) 시드 문자열. |

### `[network]` (트래픽 압축 및 TCP 배칭)

| 키 | 타입 | 기본값 | 설명 |
|---|---|---|---|
| `compression-level` | 정수 (`u32`) | `6` | zlib 압축 레벨 (`1..=12`, 높을수록 대역폭 절감·낮을수록 CPU 절감) |
| `skip-incompressible` | 불리언 (`bool`) | `true` | 압축 후 프레임이 커지면 자동으로 무압축 마커(`data_length=0`)로 전송 |
| `writer-offload-bytes` | 정수 (`usize`) | `4096` | 배치 크기가 이 값 이상일 때 압축·암호화를 블로킹 풀로 오프로드 |
| `batch-linger-ms` | 정수 (`u64`) | `2` | 소형 패킷을 한 번의 TCP 쓰기로 모으는 최대 대기 시간(ms, `0..=100`) |
| `send-queue-bytes` | 정수 (`usize`) | `2097152` | 연결당 최대 미압축 송신 큐 크기(바이트) |
| `max-connections` | 정수 (`usize`) | `4096` | 최대 동시 TCP 연결 수 (`0`이면 무제한) |
| `connection-burst` | 정수 (`u32`) | `20` | IP당 TCP 연결 버스트 허용량 (`0`이면 IP 연결 제한 비활성화, 루프백 제외) |
| `connection-per-second` | 정수 (`u32`) | `5` | IP당 TCP 연결 토큰 초당 리필 수 |

### CLI 인자 (`ferrite-server` / `--headless`)

실행 시 CLI 인자로 `ferrite.toml` 설정을 덮어쓸 수 있습니다(Windows Tauri UI에서는 사이드바의 **월드 저장 / 압축 방식 / 워커 스레드 / 압축 임계값 / 압축 레벨 / 비압축 폴백 / 오프로드(B) / 배칭 지연(ms) / 최대 TCP 연결 / 병렬 라이팅** 입력으로 선택 가능).

- `--headless`: Windows에서 GUI 없이 콘솔 모드로 실행
- `--storage <memory|fworld|anvil>` (`--memory`, `--in-memory`, `--fworld`, `--anvil`): 월드 저장 백엔드 지정
- `--compression <auto|lz4|zlib|gzip|none>`: 월드 청크 압축 알고리즘 지정
- `--compression-threshold <N>`: 패킷 압축 임계값 (`-1` = 끔, `0` = 전체 압축)
- `--compression-level <1..=12>`: 패킷 zlib 압축 레벨 (`1` = 빠름, `12` = 최대 압축)
- `--skip-incompressible <true|false>`: 비압축성 패킷 무압축 폴백 여부
- `--writer-offload-bytes <N>`: 송신 오프로드 배치 크기 임계값(바이트)
- `--batch-linger-ms <N>`: 아웃바운드 패킷 배칭 대기 시간(ms)
- `--max-connections <N>`: 최대 동시 TCP 연결 수 (`0` = 무제한)
- `--threads <N>`: Tokio 및 리전 스케줄러 워커 스레드 수 지정 (`0` = 자동)
- `--lighting-pool`: `ferrite-light` 병렬 라이팅 풀 활성화
- `--world <PATH>`: 월드 디렉터리 또는 `.fworld` 블롭 경로 지정
- `--generator <rust|superflat>`: 지형 생성기 선택
- `-h`, `--help`: CLI 도움말 출력

### 시작 시 생성되는 기본 디렉터리

`eula.txt`에 동의한 뒤 서버가 시작되면 `ferrite_server::startup_dirs`가 필요한 디렉터리를 생성합니다.
지원하지 않는 `plugins/` 디렉터리는 생성하지 않으며 기존 사용자 파일은 덮어쓰지 않습니다.

- `storage = "memory"`(기본값) 또는 `storage = "fworld"`: `config/`, `playerdata/`만 생성(`world/` Anvil 트리 미생성).
- `storage = "anvil"`: `config/`, `<world>/{data,datapacks,dimensions/minecraft/overworld/region,entities,playerdata}` 생성.

### EULA (`eula.txt`)

서버는 시작할 때 실행 디렉터리의 `eula.txt`를 확인합니다. 없으면 `eula=false`로 만들고 종료합니다.
Minecraft EULA(https://aka.ms/MinecraftEULA)를 읽고 **직접** `eula=true`로 바꾸세요.

## 실행 방법

```sh
source tools/env.sh                 # Git Bash (PowerShell: . .\tools\env.ps1)
cp ferrite.toml.example ferrite.toml
cargo run --release -p ferrite-server -- --headless --storage memory --threads 8
```
