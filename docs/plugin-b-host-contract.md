# Ferrite 플러그인 호스트 계약 (Plugin B, Lane 1)

- 계약 버전: **2** (2026-11-16 동결; v1 하위호환)
- 소유: Lane 1 (`lane/plugin-b-host`)
- Rust 구현: `crates/ferrite-plugin-host`
- Java 구현: `java/ferrite-host`
- 관련 지시서: [`plugin-b-lane-1-host.md`](handoff/plugin-b-lane-1-host.md),
  [`plugin-b-3lane-2026-11-16.md`](handoff/plugin-b-3lane-2026-11-16.md)

이 문서는 Rust 서버와 같은 프로세스의 임베디드 JVM 사이의 경계를 정의한다. Lane 2(Bukkit/Paper
facade)와 Lane 3(NMS)은 이 문서의 op/이벤트 이름과 payload만 사용한다. 계약을 바꾸면 이 문서의
버전을 올리고 세 레인 핸드오프에 영향 범위를 적는다.

## 0. 범위와 소유권

Lane 1이 소유한다:

- 프로세스당 하나뿐인 임베디드 JVM의 시작과 수명.
- 플러그인 jar 발견, `plugin.yml`/`paper-plugin.yml` 파싱과 로드 순서
  (`ferrite-plugin-meta`의 결정을 그대로 쓴다).
- 플러그인별 `URLClassLoader`, 데이터 폴더, `onLoad`/`onEnable`/`onDisable`.
- Rust↔Java RPC, 이벤트 전달, 스케줄러 실행, 로그 중계.
- `dev.ferrite.host.HostApi` SPI의 소유. Lane 2가 이 인터페이스의 구현을 제공한다.

Lane 1이 소유하지 않는다:

- `Server`/`World`/`Player`/`ItemStack`/Bukkit 이벤트/명령/스케줄러 facade의 의미론.
- NMS/CraftBukkit 클래스.
- 명령 tree, 채팅 이벤트, 전체 Paper lifecycle event.

## 1. 버전과 호환 규칙

- `CONTRACT_VERSION = 2` (`ferrite-plugin-host::CONTRACT_VERSION`, Java
  `Bootstrap.CONTRACT_VERSION`). v1 op/이벤트/필드는 그대로 유지한다.
- JVM을 시작할 때 Rust가 `contract_version`을 보내고 Java가 같은 값을 확인한다. 다르면 host
  시작은 실패하며, Rust는 `invalid_config` 오류를 낸다.
- op 이름, 이벤트 이름, 필드 이름은 **추가만** 가능하다. 기존 이름을 다른 의미로 재사용하지
  않는다. 제거/의미 변경은 계약 버전을 올린다.
- Java 호스트와 Rust 호스트는 같은 버전의 jar/바이너리로 배포한다. 구버전 op를 조용히
  재해석하지 않는다.

## 2. 전송과 인코딩

- 전송: JNI, 같은 프로세스. 소켓/파일 IPC를 쓰지 않는다.
- 문자열: UTF-16 JNI 문자열. JSON은 UTF-8 RFC 8259이며 Java 쪽은 의존성 없는 자체 파서를 쓴다.
- JSON 최상위 값은 항상 객체다.
- 숫자: 정수는 `i64`/`long`, 실수는 `f64`/`double`. 64비트 범위를 넘는 정수를 보내지 않는다.
- 경로: 운영체제 경로 문자열. Windows에서는 역슬래시를 그대로 쓰며 Java `Path.of`가 받는다.
- 로그/예외 문자열은 길이 제한 없이 전달하되, 호스트는 예외 스택을 그대로 보존한다.

### 2.1 Rust → Java 정적 진입점

`dev.ferrite.host.Bootstrap`의 `public static String` 메서드다. 모두 UTF-16 문자열/`long` 인자를
받고 JSON 응답을 돌려준다. Java는 예외를 밖으로 던지지 않고 `{ok:false,...}`로 변환한다
(`Bootstrap`이 `Throwable`을 잡는다). JNI 호출 자체가 실패하면 Rust가 `java_exception`으로
변환한다.

| 메서드 | JNI 시그니처 | 요청 | 성공 응답 |
|---|---|---|---|
| `start` | `(Ljava/lang/String;)Ljava/lang/String;` | §3.1 설정 JSON | `{"ok":true,"contract_version":2,"api":"<impl class or null>"}` |
| `loadPlugin` | `(Ljava/lang/String;)Ljava/lang/String;` | §3.2 플러그인 JSON | `{"ok":true,"name":"Foo"}` |
| `fireEvent` | `(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;` | 이벤트 이름, §5 payload | `{"ok":true,"cancelled":false}` |
| `runTask` | `(J)Ljava/lang/String;` | Java task handle | `{"ok":true,"repeat":false}` / `{"ok":false,"missing":true}` |
| `runTaskAsync` | `(J)Ljava/lang/String;` | Java task handle | `{"ok":true,"submitted":true}` / `{"ok":false,"missing":true}` |
| `forgetTask` | `(J)Ljava/lang/String;` | Java task handle | `{"ok":true}` |
| `runCommand` | `(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;` | §7 origin JSON, 명령줄 | `{"ok":true,"handled":true,"messages":[]}` |
| `shutdown` | `(Ljava/lang/String;)Ljava/lang/String;` | `{"reason":"..."}` | `{"ok":true,"disabled":["Foo"],"failed":[]}` |
| `status` | `()Ljava/lang/String;` | 없음 | `{"ok":true,"started":true,"plugins":["Foo"]}` |

`runTask`/`runTaskAsync`/`forgetTask`는 Java `TaskRegistry`의 handle을 받는다. handle은 Java가
만들고 Rust에는 불투명한 값이다.

### 3. Java → Rust RPC

`dev.ferrite.host.HostBridge`의 native 메서드다. Rust가 JVM 시작 직후
`RegisterNatives`로 바인딩한다.

```java
public final class HostBridge {
    public static native String call(String op, String requestJson);
    public static native void log(int level, String plugin, String message);
}
```

`call`의 반환값은 항상 다음 envelope다.

```json
{"ok":true,"result":<any>}
```

```json
{"ok":false,"error":{"code":"invalid_request","message":"..."}}
```

`log` 레벨: `0=trace, 1=debug, 2=info, 3=warn, 4=error`. `plugin`은 플러그인 이름 또는 `"server"`.

#### 3.1 v1 op

| op | 요청 | 성공 result | 오류 코드 |
|---|---|---|---|
| `host.ping` | `{}` | `{"pong":true,"contract_version":2}` | `shutdown` |
| `host.info` | `{}` | `{"contract_version":2,"api_version":"26.3","server":{...},"plugins_dir":"...","data_dir":"...","capabilities":{"v1_ops":[...],"v2_ops":[...],"state_provider":true}}` | `shutdown` |
| `scheduler.schedule` | `{"plugin":"Foo","handle":1,"delay_ticks":0,"period_ticks":0,"async":false}` | `{"task_id":1}` | `invalid_request`, `plugin_not_found`, `shutdown` |
| `scheduler.cancel` | `{"plugin":"Foo","task_id":1}` | `{"cancelled":true}` | `invalid_request`, `plugin_not_found`, `not_owner`, `shutdown` |

- `handle`는 Java `TaskRegistry`가 만든 양의 정수다. Rust는 이 값을 그대로 보관했다가
  `Bootstrap.runTask*`에 넘긴다.
- `delay_ticks = 0`은 **다음 tick**에 실행한다(즉시 실행하지 않는다).
- `period_ticks = 0`은 one-shot, `> 0`은 repeating이다.
- `scheduler.cancel`은 task가 없으면 `result.cancelled=false`를 돌려준다. 다른 플러그인의
  task를 취소하려 하면 `not_owner` 오류다.

v1에 없는 op는 `unknown_op` 오류다. Rust는 모르는 op를 성공한 no-op으로 처리하지 않는다.

#### 3.2 v2 state op

v2는 v1 op/이벤트를 바꾸지 않고 다음 다섯 이름을 추가한다. 실제 Bukkit/NMS 의미론은 Lane 2/3
소유이고, Lane 1은 전송·검증·수명·서버 adapter seam만 소유한다. 서버 adapter는 월드 생성 뒤
설치되며, 그 전에는 state op가 `state_unavailable`로 실패한다.

| op | 요청 | 성공 result | 오류 코드 |
|---|---|---|---|
| `world.block_get` | `{"world":"minecraft:overworld","x":0,"y":64,"z":0}` | `{"world":"...","x":0,"y":64,"z":0,"state_id":1,"material":"minecraft:stone","properties":{},"loaded":true}` | `invalid_request`, `state_unavailable`, `unknown_world`, `unknown_block_state`, `chunk_not_loaded`, `shutdown` |
| `world.block_set_async` | `{"world":"...","x":0,"y":64,"z":0,"state_id":1,"expected_state_id":0}` (`expected_state_id` 선택) | `{"queued":true,"request_id":1}` | `invalid_request`, `state_unavailable`, `unknown_world`, `unknown_block_state`, `chunk_not_loaded`, `stale_request`, `queue_full`, `shutdown` |
| `world.chunk_loaded` | `{"world":"...","x":0,"z":0}` | `{"world":"...","x":0,"z":0,"loaded":true}` | `invalid_request`, `state_unavailable`, `unknown_world`, `shutdown` |
| `player.state_get` | `{"uuid":"..."}` | 사용 가능한 경우 player snapshot 객체 | 위 공통 + `player_not_found`; 이 슬라이스의 기본 provider는 `unsupported` |
| `entity.state_get` | `{"uuid":"..."}` | 사용 가능한 경우 entity snapshot 객체 | 위 공통 + `entity_not_found`; 이 슬라이스의 기본 provider는 `unsupported` |

- 요청은 JSON 객체이며 알 수 없는 필드는 `invalid_request`다. `state_id`는 26.3 registry의
  전역 block-state id(`u16` 범위)이고, 음수/범위 밖/없는 id는 `unknown_block_state`다.
- `world.block_get`은 **동기 read**다. Java 호출 스레드가 provider의 실제 dimension source에서
  스냅샷을 읽는다. 청크가 없으면 공기로 대신하지 않고 `chunk_not_loaded`다.
- `world.block_set_async`는 **동기 검증 + 비동기 적용**이다. provider는 world/state/청크와
  선택적 compare-and-set을 검증한 뒤 소유 큐에 넣고 즉시 `queued:true`를 돌려준다. 적용은
  `PluginHost::tick`(게임 루프)이 수행한다. 따라서 `queued:true`는 성공적인 **접수**이며 블록이
  이미 바뀌었다는 뜻이 아니다.
- compare-and-set이 접수 시점에 어긋나면 `stale_request`다. 큐에 들어간 뒤 다른 변경과
  경쟁하면 적용 시 `compare_set_block`이 실패하고 경고 로그만 남는다(이 슬라이스에는 완료
  상태 조회 op가 없다).
- `world.chunk_loaded`는 resident chunk만 조회하며 새 청크를 생성하지 않는다.
- provider가 없거나 아직 설치되지 않았으면 `state_unavailable`; 종료 중에는 `shutdown`.
  `player.state_get`/`entity.state_get`는 후보이며 현재 기본 provider는 `unsupported`다.
- Java→Rust native 호출 자체에는 취소 가능한 타임아웃이 없다. `timeout`은 Rust가 Java를
  호출하는 v1 경로의 제한이며, state read는 provider가 즉시 답해야 한다. 느린 provider는
  서버 tick을 막을 수 있으므로 Lane 2/3은 리전 tick을 기다리는 read를 만들면 안 된다.

`host.info`의 `capabilities.v2_ops`는 **실제 설치된 provider가 광고한 op만** 나열한다.
provider가 없으면 빈 배열이고 `state_provider=false`다. 기본 서버 provider는
`world.block_get`, `world.block_set_async`, `world.chunk_loaded`를 광고한다.

### 4. 설정 JSON (`Bootstrap.start`)

```json
{
  "contract_version": 2,
  "api_version": "26.3",
  "server": {
    "name": "Ferrite",
    "version": "0.2.1",
    "motd": "A Ferrite server",
    "max_players": 20,
    "online_mode": true,
    "view_distance": 10
  },
  "plugins_dir": "C:\\server\\plugins",
  "data_dir": "C:\\server\\plugins",
  "api_impl_class": "dev.ferrite.api.FerriteHostApi"
}
```

- `api_impl_class`가 `null`/빈 문자열이면 `dev.ferrite.host.NoopHostApi`를 쓴다. Lane 2는 자신의
  구현 클래스 이름을 설정에 넣거나, `ferrite-host`가 기본값을 갖도록 합의한다.
- `api_impl_class`는 host classloader에서 로드 가능해야 한다. 플러그인 jar에 넣지 않는다.
- `plugins_dir`/`data_dir`은 Rust가 만든 절대 경로다. Java는 데이터 폴더를 한 번 더 확인한다.

서버의 `[plugins]` 설정은 이 JSON으로 다음과 같이 매핑된다. classpath 순서는 **`host-jar`가
항상 첫 번째**이고, 그 뒤에 `runtime-classpath`에 나열한 순서대로 붙는다. 플러그인 jar는
classpath에 넣지 않는다. `api-impl-class`는 이 classpath 안에서 로드 가능해야 하며, 빈
문자열/생략이면 `NoopHostApi`다.

```toml
[plugins]
enabled = true
host-jar = "java/ferrite-host/build/libs/ferrite-host.jar"
runtime-classpath = ["java/ferrite-api-impl/build/libs/ferrite-api-impl.jar"]
api-impl-class = "dev.ferrite.api.FerriteHostApi"
```

### 5. 플러그인 메타데이터 JSON (`Bootstrap.loadPlugin`)

Rust가 `plugin.yml`/`paper-plugin.yml`을 파싱한 결과를 구조화해 보낸다. Java는 YAML을 다시
읽지 않는다.

```json
{
  "name": "Foo",
  "version": "1.2.3",
  "main": "com.example.foo.FooPlugin",
  "api_version": "1.21",
  "load": "POSTWORLD",
  "dependencies": ["Bar"],
  "soft_dependencies": [],
  "load_before": [],
  "provides": [],
  "folia_supported": false,
  "format": "plugin.yml",
  "jar": "C:\\server\\plugins\\Foo.jar",
  "data_folder": "C:\\server\\plugins\\Foo",
  "commands": {
    "foo": {"description": "…", "usage": "/foo", "aliases": ["f"], "permission": "foo.use"}
  },
  "permissions": {
    "foo.use": {"description": "…", "default": "op", "children": {}}
  }
}
```

- `load`는 `STARTUP` 또는 `POSTWORLD`다. `postworld` 플러그인은 월드가 준비된 뒤에 로드된다.
- `dependencies`는 hard dependency다. Rust가 로드 순서와 누락을 먼저 처리한다. 실행 중 실패한
  플러그인에 의존하는 플러그인은 Java에 도달하기 전에 Rust가 `dependency_failed`로 실패시킨다.
- `commands`/`permissions`는 원문이 아니라 구조화된 값이다. Lane 2가 Bukkit 타입으로 변환한다.

### 6. 플러그인 생명주기

1. Rust가 jar를 스캔하고 `ferrite-plugin-meta`로 `startup`/`postworld` 순서를 정한다.
2. `startup`은 월드 초기화 전에, `postworld`는 월드 준비 뒤에 **한 번씩** 로드한다.
3. 플러그인마다:
   - 데이터 폴더 `data_dir/<name>/`를 만든다. 이름에 경로 구분자/`..`가 있으면 그 플러그인만
     실패한다.
   - 부모가 host classloader인 `URLClassLoader`를 만든다(자식 우선이 아니라 Java 기본
     parent-first다). 플러그인 jar는 시스템 classpath에 넣지 않는다.
   - `main` 클래스를 로드하고 `dev.ferrite.host.JavaPlugin`의 하위 클래스인지 확인한다.
   - 인스턴스 생성 → `onLoad()` → `onEnable()`.
   - `onEnable`이 성공해야 `enabled=true`다. 실패하면 `onDisable`을 시도하고 classloader를
     닫으며 그 플러그인만 `failed`로 보고한다.
4. 종료 시 로드 역순으로 `onDisable()` → classloader close. 한 플러그인의 실패가 다른
   플러그인의 disable이나 서버 종료를 막지 않는다.
5. JVM은 클래스를 완전히 언로드하지 못한다. classloader를 닫아도 클래스 메타데이터가 남을 수
   있다. 재로드는 v1에서 지원하지 않는다.

`onLoad`/`onEnable`/`onDisable`은 Rust의 전용 host 스레드에서 호출된다. 서버 tick 스레드가
아니다. Lane 2는 `onEnable`에서 서버 상태를 읽을 때 이 점을 전제로 해야 한다. `onEnable`
완료 전에는 어떤 이벤트도 그 플러그인에 전달되지 않는다.

### 7. 이벤트

이벤트 이름과 payload는 v1에서 다음 네 개만 지원한다.

| 이름 | payload | 취소 | 전달 |
|---|---|---|---|
| `player_join` | `{"player_uuid":"…","player_name":"…"}` | 불가 | 동기 |
| `player_quit` | `{"player_uuid":"…","player_name":"…"}` | 불가 | 동기 |
| `block_break` | `{"player_uuid":"…","player_name":"…","world":"minecraft:overworld","x":0,"y":64,"z":0,"material":"minecraft:stone"}` | 가능 | 지연 |
| `player_interact` | `{"player_uuid":"…","player_name":"…","world":"minecraft:overworld","x":0,"y":64,"z":0,"face":1,"material":"minecraft:stone","action":"right_click_block"}` | 가능 | 지연 |

- `action`은 `left_click_block` 또는 `right_click_block`이다.
- **동기**: `fire_event`를 호출한 스레드에서 즉시 Java 리스너를 실행한다. 서버 coordinator
  스레드가 `player_join`/`player_quit`에 쓴다. 리스너는 오래 블록하면 안 된다.
- **지연**: 블록 파괴/상호작용을 소유한 리전 스레드는 Java를 기다리지 않는다. Rust 서버
  어댑터가 이벤트를 큐에 넣고 다음 tick에 `fire_event`로 전달한다. 따라서 그 시점에는 블록
  파괴/상호작용이 이미 적용됐고, 리스너의 취소는 **동작을 되돌리지 못한다**. 취소 결과가 오면
  호스트는 경고 로그를 남긴다. 이 제약은 v1의 명시적 한계이며, 이후 계약에서 리전 스레드가
  아닌 지점에 취소 가능한 동기 경로를 추가할 수 있다.
- 알 수 없는 이벤트 이름은 `unknown_event` 오류다. 성공한 no-op으로 숨기지 않는다.
- 리스너 예외는 `java_exception` 오류로 Rust에 돌아오고, 서버는
  `PluginEventPolicy`(기본 `Allow`)에 따라 동작을 계속하거나 취소한다. 지연 이벤트의 예외는
  로그만 남기고 서버 tick을 멈추지 않는다.
- `HostApi`가 설치되지 않은 경우(Noop) 모든 이벤트는 `cancelled=false`다.

### 8. 스케줄러

- 시간 단위는 서버 tick(50 ms)이다. `PluginHost::tick()`이 서버 tick마다 호출된다.
- 동기(`async=false`): tick을 호출한 서버 스레드에서 실행된다. 실행 중 다른 리전을 동기
  대기하면 안 된다(Lane 2/3 책임).
- 비동기(`async=true`): `ferrite-plugin-async-*` 데몬 스레드 풀에서 실행된다. 반복 주기보다
  실행이 길면 겹칠 수 있다. v1은 이를 막지 않으며 로그로만 관찰한다.
- 취소 토큰은 `scheduler.schedule`이 돌려주는 `task_id`(`u64`)다.
- one-shot은 실행 뒤, repeating은 취소 전까지 유지된다. task가 예외를 던지면 그 task는
  취소된다.
- 플러그인의 `onEnable`이 끝나기 전에는 그 플러그인의 task가 실행되지 않는다. `onDisable`
  중이거나 끝난 뒤의 schedule 요청은 `IllegalStateException`이다.
- 플러그인 disable/서버 shutdown 시 그 플러그인의 모든 task를 취소한다. `onDisable` 중에
  새로 만든 task도 disable이 끝날 때 취소된다.
- v1은 task 본문을 Rust가 소유하지 않는다. Java `TaskRegistry`가 `Runnable`을 보관하고
  Rust는 handle만 호출한다.

### 9. 오류

Rust `HostError`와 RPC 오류 코드:

| 코드 | 의미 |
|---|---|
| `disabled` | `plugins.enabled=false` |
| `already_started` | 이 프로세스에 플러그인 호스트가 이미 있다 |
| `jvm_already_running` | JVM은 있지만 classpath에 이 호스트 jar가 없다 |
| `host_jar_missing` | host jar/classes 경로가 없다 |
| `metadata` | 플러그인 디렉터리/jar 메타데이터 오류 |
| `invalid_config` | 설정/계약 버전 오류 |
| `invalid_request` | RPC 요청 스키마 오류 |
| `unknown_op` | 모르는 RPC op |
| `unknown_event` | 모르는 이벤트 이름 |
| `not_running` | 호스트가 시작되지 않았거나 종료됐다 |
| `shutdown` | 종료 중 RPC |
| `plugin_not_found` | 플러그인 이름을 모른다 |
| `not_owner` | 다른 플러그인의 task를 취소하려 함 |
| `plugin_failed` | 플러그인 로드/활성화 실패 |
| `dependency_failed` | hard dependency가 실행 중 실패함 |
| `java_exception` | Java 예외(스택 포함) |
| `timeout` | Java 호출이 제한 시간을 넘김 |
| `invalid_response` | Java가 계약과 다른 JSON을 돌려줌 |
| `unsupported` | provider가 아직 구현하지 않은 v2 op |
| `state_unavailable` | 서버 state provider가 아직 설치되지 않음 |
| `unknown_world` | v2 world 키를 모름 |
| `unknown_block_state` | 26.3 registry에 없는 block-state id |
| `chunk_not_loaded` | 요청한 청크가 resident가 아님 |
| `stale_request` | v2 compare-and-set 기대값이 현재 값과 다름 |
| `queue_full` | v2 write 큐가 가득 참(조용히 버리지 않음) |

`timeout`의 한계: JNI 호출은 취소할 수 없다. Rust는 호출 스레드를 기다리지 않고 오류를
돌려주지만, Java 쪽 작업은 계속 실행될 수 있다. 타임아웃 뒤의 상태는 보장하지 않으며 로그에
남긴다.

### 10. 타임아웃과 스레드

- `start`: 기본 30초. `loadPlugin`, `fireEvent`, `runCommand`, `shutdown`: 기본 10초.
  모두 `PluginHostConfig`에서 조정할 수 있다.
- Rust는 JNI 호출을 전용 `ferrite-plugin-host` 스레드에서 실행하고 호출자에게 결과를
  돌려준다. 따라서 Java 호출이 멈춰도 서버 tick이 무기한 멈추지 않는다(타임아웃 후에도 Java
  작업은 남을 수 있음).
- Java→Rust native 콜백은 호출한 Java 스레드에서 그대로 실행된다. Rust 핸들러는 Java 객체를
  다시 조작하지 않으며(재진입 없음), 락을 오래 잡지 않는다.
- Java `TaskRegistry`의 동기 task는 `PluginHost::tick()`을 호출한 스레드에서 실행된다.
- 모든 JNI 스레드는 daemon으로 attach된다.

### 11. 로그

- Java `HostLogger`는 `HostBridge.log`로 Rust `tracing`에 보낸다. target은 `ferrite_plugin`,
  필드는 `plugin`이다.
- 플러그인 이름이 붙은 로그와 `onEnable`/`onDisable` 예외 스택을 남긴다.
- Rust `PluginHost`는 플러그인별 로드/실패/종료 로그를 남기며, 한 플러그인의 실패가 다른
  플러그인의 로드를 막지 않는다(명시적 hard dependency 제외).

### 12. Lane 2/3 SPI

`java/ferrite-host`가 다음을 제공한다. Lane 2가 구현하고, Lane 3은 Lane 2를 통해 쓴다.

```java
public interface HostApi {
    default void onHostStarted(HostContext context) throws Exception {}
    default void onPluginEnabled(HostPlugin plugin) throws Exception {}
    default void onPluginDisabled(HostPlugin plugin) {}
    default EventResult fireEvent(String name, Map<String, Object> payload) throws Exception {
        return EventResult.pass();
    }
    default CommandResult runCommand(CommandOrigin origin, String line) throws Exception {
        return CommandResult.notHandled();
    }
    default void onHostStopping() {}

    // v2 candidates: default unsupported in Lane 1; the native transport helpers on
    // Bootstrap are the supported path until a lane explicitly overrides these.
    default Map<String, Object> blockGet(Map<String, Object> request) { throw ...; }
    default Map<String, Object> blockSetAsync(Map<String, Object> request) { throw ...; }
    default boolean chunkLoaded(String world, int x, int z) { throw ...; }
    default Map<String, Object> playerStateGet(java.util.UUID uuid) { throw ...; }
    default Map<String, Object> entityStateGet(java.util.UUID uuid) { throw ...; }
}
```

- `Bootstrap`의 `worldBlockGet`, `worldBlockSetAsync`, `worldChunkLoaded`,
  `playerStateGet`, `entityStateGet`는 위 native op로 보내는 **transport helper**다. 이 메서드들은
  v2 op 이름을 바꾸지 않고 그대로 사용한다. `HostApi`의 v2 default 메서드는 native 경로가 아니라
  향후 in-process adapter용이며, 기본 구현은 `UnsupportedOperationException`이다.
- `HostContext`는 API 버전/서버 정보/디렉터리/로거/스케줄러를 제공한다.
- `HostPlugin`은 `pluginName()`, `pluginVersion()`, `pluginSpec()`, `pluginDataFolder()`,
  `pluginLogger()`, `hostContext()`, `isPluginEnabled()`, `setPluginEnabled(boolean)`,
  `onLoad()`, `onEnable()`, `onDisable()`을 갖는다. Bukkit과 이름이 충돌하는 `getName`,
  `getLogger`, `getDataFolder`는 **일부러 쓰지 않는다**. Lane 2의 Bukkit `JavaPlugin`이 이
  클래스를 상속하고 Bukkit 시그니처를 자유롭게 구현할 수 있게 하기 위해서다.
- `JavaPlugin`(host)은 위 메서드의 기본 구현과 데이터 폴더/로거 바인딩을 제공한다.
  플러그인 `main` 클래스는 반드시 이 클래스의 하위 클래스여야 한다.
- `EventResult`는 `cancelled`만 전달한다. 이벤트 상태 변경은 Lane 2가 자신의 이벤트 객체로
  처리한다.
- `CommandResult`는 `handled`와 `CommandMessage` 목록을 전달한다. 명령 tree 등록은 Lane 2가
  한다. Lane 1의 서버 배선은 `plugin.yml`에 선언된 명령 이름만 게임 명령 tree에 등록하고,
  실행은 `HostApi.runCommand`로 넘긴다.

### 13. Gradle 모듈

`java/settings.gradle.kts`가 다음을 include한다.

- `ferrite-host` — Lane 1. 이 계약의 Java 구현.
- `ferrite-api-impl` — Lane 2. 빈 스캐폴드만 Lane 1이 만든다.
- `ferrite-nms` — Lane 3. 빈 스캐폴드만 Lane 1이 만든다.

`ferrite-host`는 외부 런타임 의존성이 없다(테스트의 JUnit 제외). host classpath에는
`ferrite-host`와 Lane 2의 런타임 jar만 넣고 플러그인 jar는 넣지 않는다.

`java/settings.gradle.kts`는 PaperMC Maven 저장소를 포함하고, Lane 2/3의 fixture 모듈은
디렉터리가 실제로 있을 때만 include한다. 따라서 fixture가 아직 없는 Lane 1 단독 체크아웃도
설정과 빌드가 깨지지 않는다.

### 14. v1의 알려진 한계

- 블록 파괴/상호작용 취소는 지연 전달이라 동작을 되돌리지 못한다(§7).
- 플러그인 간 classpath 공유(Paper `join-classpath`)를 지원하지 않는다. 각 플러그인은 자기
  classloader만 본다.
- 플러그인 재로드/언로드를 지원하지 않는다. JVM은 클래스를 완전히 언로드하지 않는다.
- Bukkit/Paper API, 명령 실행 의미론, NMS는 Lane 2/3의 몫이다. 이 계약은 전송과 생명주기만
  보장한다.
- 실제 Minecraft 클라이언트 렌더링/화면 검증은 하지 않는다.

## 변경 기록

| 버전 | 날짜 | 변경 |
|---|---|---|
| 1 | 2026-11-16 | 최초 동결. JNI 전송, 생명주기, 4개 이벤트, 스케줄러, 오류 코드, Lane 2 SPI. |
| 2 | 2026-11-16 | v2 state op 5개 동결. 첫 수직 슬라이스는 `world.block_get`, `world.block_set_async`, `world.chunk_loaded`; `player.state_get`/`entity.state_get`는 후보. `host.info` capability 추가. |
