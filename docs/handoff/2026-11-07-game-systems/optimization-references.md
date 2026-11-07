# Ferrite 최적화·서버 호환성 참고자료

조사 기준일: 2026-11-08

이 문서는 구현할 기능의 우선순위를 정하고 설계를 비교하기 위한 참고 목록이다. 외부 구현을
그대로 복사하거나 라이선스를 확인하지 않은 코드를 가져오지 않는다. Ferrite에 적용할 때는
Minecraft Java 26.3 동작과 플러그인 API 계약을 보존하고, 성능 측정과 회귀 검증을 추가한다.

## 참고자료

| 자료 | 종류 | Ferrite에서 확인할 내용 |
|---|---|---|
| [서버 최적화 모드 10선 — WiseHosting](https://wisehosting.com/blog/10-best-minecraft-server-optimization-mods) | Minecraft 서버 최적화 모드 소개 | 모드가 해결하는 병목과 적용 범위를 후보 목록으로 삼는다. 글의 버전·측정 조건을 확인하고 Ferrite에 그대로 적용 가능한 것으로 간주하지 않는다. |
| [Server Optimization 모드팩 — Modrinth](https://modrinth.com/modpack/server-optimizationn) | 최적화 모드팩 | 모드 조합, 중복 기능, 필수/선택 설정을 확인한다. 모드팩은 성능 근거나 바닐라 호환성 인증이 아니다. |
| [Minecraft Optimization Guide — YouHaveTrouble](https://github.com/YouHaveTrouble/minecraft-optimization) | 서버 운영·설정 가이드 | JVM/서버 설정, 운영 지침, 실제 병목 측정과 잘못된 튜닝의 위험을 확인한다. Ferrite는 Rust 서버이므로 Java 전용 플래그를 직접 적용하지 않는다. |
| [Pumpkin](https://github.com/Pumpkin-MC/Pumpkin) | Rust로 작성한 Minecraft 서버 | Rust 서버의 구조, 병렬 처리, 게임 동작 구현 및 성능 검증 접근을 비교한다. Ferrite와 버전·기능·측정 환경이 다르므로 수치 비교는 동일 조건에서만 한다. |
| [PatchBukkit](https://github.com/Pumpkin-MC/PatchBukkit) | Pumpkin의 Bukkit/Paper 플러그인 호환 계층 | JVM 임베딩, Rust↔Java 브리지, API 표면과 호환 한계를 조사해 Ferrite 플러그인 브리지/NMS 로드맵에 반영한다. NMS·CraftBukkit 내부 의존성이 동작한다고 미리 가정하지 않는다. |
| [PatchBukkit 아키텍처 문서](https://github.com/Pumpkin-MC/PatchBukkit/blob/master/ARCHITECTURE.md) | 플러그인 브리지 구조 | Rust 네이티브 부분, Java API 구현, JNI 및 FFI 통신 경계를 비교한다. Ferrite의 현재 `HostBridge`와 실제 구현 가능한 API 범위를 기준으로 평가한다. |
| [Cubiomes](https://github.com/Cubitect/cubiomes) | C 기반 바이옴·일부 피처 생성 참고 라이브러리 | 독립 알고리즘 설계 시 버전 범위, 공개 동작, 비교용 테스트 입력을 조사한다. 블록 단위 지형 생성 전체를 제공하는 것은 아니므로 전체 월드젠 대체재로 보지 않는다. 코드 재사용 전 라이선스와 출처를 별도 검토한다. |
| [C2ME](https://github.com/RelativityMC/C2ME) | Minecraft 청크 로딩·월드젠 최적화 모드 | 병렬 월드젠, 청크 I/O·캐시·스케줄링 최적화 아이디어와 적용 가능한 버전 범위를 조사한다. Ferrite에는 설계 참고만 하며 코드를 복사하지 않는다. 라이선스와 각 기능의 동작 영향은 적용 전에 확인한다. |
| [Folia 기술 개요](https://docs.papermc.io/folia/reference/overview/) | region 단위 병렬 tick 설계 참고 | 서로 인접하지 않는 ticking region, region 소유 데이터의 단일 쓰기 소유권, 지역/엔티티/global task queue, 교차 region 작업 경계를 참고한다. Folia의 Java 구현을 복사하지 않고 Ferrite의 regionizer와 API 계약에 맞춰 설계한다. |
| [Advanced Slime World Manager (ASWM)](https://github.com/Paul19988/Advanced-Slime-World-Manager) | Slime Region Format 월드 매니저 플러그인 | 포맷, 월드 로드/저장 수명주기, 압축·직렬화, 호환성 경계를 조사한다. |
| [Advanced Slime Paper](https://github.com/InfernalSuite/AdvancedSlimePaper) | ASWM 관련 서버 구현/후속 프로젝트 참고 | ASWM과의 역할 관계, 서버 측 통합이 필요한 영역, 포맷 호환 테스트 방법을 확인한다. |

## 플러그인 JVM 병렬 처리 — 현재 구조와 최적화 방향

### 현재 확인된 구조

- `java/ferrite-api-impl/src/main/java/dev/ferrite/server/Bootstrap.java`의
  `LEGACY_LOCK`은 플러그인 로딩, 여러 이벤트 호출, 명령 실행, scheduler tick, shutdown을
  감싼다. 이 경로들은 현재 JVM 안에서 직렬 실행된다.
- 같은 클래스의 entity/region scheduler 실행 진입점은 이 전역 잠금을 통하지 않는다.
  따라서 스케줄러가 병렬 실행될 수 있다는 사실만으로 일반 Bukkit 이벤트나 API 호출까지
  스레드 안전하다고 볼 수 없다.
- JVM/JNI 자체는 여러 attached thread의 진입을 지원할 수 있지만, Ferrite의 Bukkit 서버,
  플러그인 상태, HostBridge 호출 대상은 공유 객체와 서버 상태를 사용한다. 임의의 이벤트를
  동시에 실행하면 레거시 플러그인 호환성과 게임 상태 일관성이 깨질 수 있다.

### 단계적 최적화안

1. **먼저 병목을 계측한다.** JVM 진입 대기, `LEGACY_LOCK` 대기/보유 시간, 이벤트 종류별
   callback 시간, Rust↔Java 왕복 횟수를 별도로 기록한다. 평균뿐 아니라 p95/p99와 tick 지연을
   비교하고, 플러그인 없음·무동작 listener·실제 listener 워크로드를 나눠 측정한다.
2. **레거시 경로는 직렬성을 유지한다.** 기존 Bukkit/Paper 플러그인 이벤트는 기본적으로
   전역 순서와 메인 스레드 기대를 보존한다. 먼저 모든 호출을 병렬화하거나 잠금을 제거하지 않는다.
3. **명시적 region-aware 경로만 분리한다.** 플러그인이 opt-in한 지역 작업을 대상으로 소유
   region 확인, 해당 region 상태만 읽고 쓰는 API, 안전한 메시지 전달 규약을 마련한 뒤 지역별
   큐/잠금으로 병렬화한다. 다른 region이나 전역 서버 상태 접근은 명시적인 스케줄링 경계를
   거치게 한다.
4. **회귀 기준을 둔다.** 같은 입력에서 직렬/병렬 결과와 이벤트 순서를 비교하고, 두 region이
   동시에 갱신되는 상황, 재진입 callback, 취소 이벤트, 플러그인 예외, shutdown 경합을 시험한다.
   최적화 전후 TPS만으로 호환성이나 데이터 안전을 주장하지 않는다.

현 단계의 결론은 **작업 단위 병렬화는 가능하지만 일반 플러그인 callback의 무차별 병렬화는
아직 안전하지 않다**는 것이다. 우선 계측 후 region-aware API를 명시적으로 분리하는 것이
검토할 다음 단계다.

## Ferrite 적용 절차

1. 기능·최적화 후보마다 현재 Ferrite 구현 위치와 누락 동작을 먼저 확인한다.
2. 기준 Minecraft 버전, 설정, 하드웨어, 워크로드를 기록하고 최적화 전후를 같은 조건에서 잰다.
3. 동작 변경이 없는 최적화도 기존 회귀 테스트를 통과시키고, 게임 동작을 건드리면 바닐라 대조
   테스트를 추가한다. 성능 수치만으로 동작 동등성을 주장하지 않는다.
4. Paper/Bukkit 호환 기능은 이벤트·스케줄러·플러그인 관찰 가능성을 보존한다. 이벤트를 생략하는
   최적화는 등록된 리스너가 있을 때의 동작까지 시험한다.
5. 외부 코드·패치·라이선스는 별도로 검토한다. Mojang 코드나 에셋은 저장소에 넣지 않는다.

## 작업 순서

플러그인/NMS 브리지, 피스톤, 엔드, 동물·볼트, 실제 클라이언트 검증은 서로 다른 파일 소유권과
수용 기준을 두고 병렬 진행한다. ASWM 월드 관리와 **Rust 월드젠은 현재 작업에서 제외**한다.
Rust 월드젠은 나머지 게임 작업 뒤 마지막 단계에서 알고리즘 경계별 병렬 워커로 독립 구현하며,
Moonrise식 최적화 기법은 그 단계에서 관련 알고리즘과 함께 적용·측정한다. ASWM/Slime 형식의
압축 월드와 메모리 상주 청크 접근은 구현 후보로 기록하되, 대규모 월드의 메모리 상한과 저장
복구 비용을 평가한 뒤 적용 범위를 정한다.
