//! Runtime locator and first-launch bootstrapper for Mojang's `vanilla/26.3/generated` data.
//!
//! Portable releases do not redistribute Mojang code or extracted data. When no pre-extracted
//! `vanilla/<GAME_VERSION>/generated` tree is present on disk, [`ensure_vanilla_generated`]
//! downloads the official Minecraft server jar (verified by SHA-1), runs Mojang's data generator
//! and Ferrite's local Java extractors using an installed Java 25+ JDK, and populates
//! `./vanilla/<GAME_VERSION>/generated` so first-time users can launch directly.

use std::collections::HashSet;
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, bail};
use ferrite_protocol::GAME_VERSION;
use flate2::read::DeflateDecoder;
use serde::Deserialize;
use sha1::{Digest, Sha1};

const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

const EXTRACTOR_JAVA: &str = include_str!("../../../tools/extractor/Extractor.java");
const PISTON_EXTRACTOR_JAVA: &str = include_str!("../../../tools/extractor/PistonExtractor.java");
const ADVANCEMENT_EXTRACTOR_JAVA: &str =
    include_str!("../../../tools/extractor/AdvancementExtractor.java");
const RECIPE_EXTRACTOR_JAVA: &str = include_str!("../../../tools/extractor/RecipeExtractor.java");

#[derive(Deserialize)]
struct Manifest {
    versions: Vec<ManifestVersion>,
}

#[derive(Deserialize)]
struct ManifestVersion {
    id: String,
    url: String,
}

#[derive(Deserialize)]
struct VersionMeta {
    downloads: VersionDownloads,
    #[serde(rename = "javaVersion")]
    java_version: Option<JavaRequirement>,
}

#[derive(Deserialize)]
struct VersionDownloads {
    server: DownloadArtifact,
}

#[derive(Deserialize)]
struct DownloadArtifact {
    sha1: String,
    url: String,
}

#[derive(Deserialize)]
struct JavaRequirement {
    #[serde(rename = "majorVersion")]
    major_version: u32,
}

fn has_valid_vanilla_data(dir: &Path) -> bool {
    dir.join("reports/registries.json").is_file()
        && (dir.join(".complete").is_file() || dir.join("data/minecraft").is_dir())
}

fn check_cancelled(cancelled: Option<&AtomicBool>) -> anyhow::Result<()> {
    if cancelled.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
        bail!("서버 시작이 취소되었습니다.");
    }
    Ok(())
}

/// Ensures `vanilla/<GAME_VERSION>/generated` exists and registers it for the whole process.
pub async fn ensure_vanilla_generated(cancelled: Option<&AtomicBool>) -> anyhow::Result<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let resolved = ferrite_protocol::vanilla_generated_dir(Some(manifest_dir));
    if has_valid_vanilla_data(&resolved) {
        ferrite_protocol::set_vanilla_generated_dir(resolved.clone());
        return Ok(resolved);
    }

    if let Some(env_dir) = std::env::var_os("FERRITE_VANILLA_ROOT").map(PathBuf::from)
        && has_valid_vanilla_data(&env_dir)
    {
        ferrite_protocol::set_vanilla_generated_dir(env_dir.clone());
        return Ok(env_dir);
    }

    let base_dir = PathBuf::from("vanilla").join(GAME_VERSION);
    let gen_dir = base_dir.join("generated");
    if has_valid_vanilla_data(&gen_dir) {
        ferrite_protocol::set_vanilla_generated_dir(gen_dir.clone());
        return Ok(gen_dir);
    }

    check_cancelled(cancelled)?;
    tracing::info!(
        target = %gen_dir.display(),
        version = GAME_VERSION,
        "바닐라 26.3 데이터가 없어 공식 Minecraft 서버 JAR에서 자동 추출을 시작합니다 (최초 1회)..."
    );

    let client = reqwest::Client::builder()
        .user_agent(concat!("ferrite-mc/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("HTTP 클라이언트 초기화 실패")?;

    let manifest: Manifest = client
        .get(VERSION_MANIFEST_URL)
        .send()
        .await
        .context("Mojang 버전 매니페스트 조회 실패 (인터넷 연결 확인 필요)")?
        .error_for_status()
        .context("Mojang 버전 매니페스트 HTTP 오류")?
        .json()
        .await
        .context("Mojang 버전 매니페스트 JSON 파싱 실패")?;

    let entry = manifest
        .versions
        .into_iter()
        .find(|v| v.id == GAME_VERSION)
        .with_context(|| {
            format!("Mojang 매니페스트에서 Minecraft {GAME_VERSION} 버전을 찾을 수 없습니다")
        })?;

    let meta: VersionMeta = client
        .get(&entry.url)
        .send()
        .await
        .context("Minecraft 버전 메타데이터 다운로드 실패")?
        .error_for_status()
        .context("Minecraft 버전 메타데이터 HTTP 오류")?
        .json()
        .await
        .context("Minecraft 버전 메타데이터 파싱 실패")?;

    let min_java = meta.java_version.map_or(25, |j| j.major_version.max(25));
    let (java_bin, javac_bin) = find_java(min_java).with_context(|| {
        format!(
            "바닐라 {GAME_VERSION} 데이터(vanilla/{GAME_VERSION}/generated)가 없고 Java {min_java}+ 실행 파일을 찾지 못했습니다. \
             Java {min_java} JDK(예: https://adoptium.net/)를 설치한 뒤 다시 시작하거나, \
             기존 vanilla/{GAME_VERSION}/generated 폴더를 실행 폴더에 복사하세요"
        )
    })?;

    check_cancelled(cancelled)?;
    fs::create_dir_all(&base_dir)
        .with_context(|| format!("디렉터리 생성 실패: {}", base_dir.display()))?;
    let jar_path = base_dir.join("server.jar");
    let expected_sha1 = meta.downloads.server.sha1.to_ascii_lowercase();
    let needs_download =
        !jar_path.is_file() || file_sha1_hex(&jar_path).is_ok_and(|actual| actual != expected_sha1);
    if needs_download {
        tracing::info!(
            url = %meta.downloads.server.url,
            dest = %jar_path.display(),
            "공식 Minecraft {GAME_VERSION} server.jar 다운로드 중..."
        );
        let bytes = client
            .get(&meta.downloads.server.url)
            .send()
            .await
            .context("Minecraft server.jar 다운로드 실패")?
            .error_for_status()
            .context("Minecraft server.jar 다운로드 HTTP 오류")?
            .bytes()
            .await
            .context("Minecraft server.jar 본문 수신 실패")?;
        let actual_sha1 = sha1_hex(&bytes);
        if actual_sha1 != expected_sha1 {
            bail!(
                "다운로드된 server.jar SHA-1 불일치: expected={expected_sha1}, actual={actual_sha1}"
            );
        }
        fs::write(&jar_path, &bytes)
            .with_context(|| format!("server.jar 저장 실패: {}", jar_path.display()))?;
    } else {
        tracing::info!(jar = %jar_path.display(), "기존 공식 server.jar(SHA-1 일치)를 재사용합니다");
    }

    check_cancelled(cancelled)?;
    let base_abs = fs::canonicalize(&base_dir).unwrap_or_else(|_| base_dir.clone());
    let gen_abs = base_abs.join("generated");
    let jar_abs = base_abs.join("server.jar");

    tokio::task::spawn_blocking(move || -> anyhow::Result<PathBuf> {
        if gen_abs.exists() {
            fs::remove_dir_all(&gen_abs)
                .with_context(|| format!("기존 불완전 generated 정리 실패: {}", gen_abs.display()))?;
        }
        tracing::info!("Minecraft 데이터 생성기(--reports --server) 실행 중...");
        let mut cmd = hidden_command(&java_bin);
        cmd.arg("-DbundlerMainClass=net.minecraft.data.Main")
            .arg("-jar")
            .arg(&jar_abs)
            .arg("--reports")
            .arg("--server")
            .arg("--output")
            .arg(&gen_abs)
            .current_dir(&base_abs);
        let status = cmd.status().context("Java 데이터 생성기 실행 실패")?;
        if !status.success() {
            bail!("Minecraft 데이터 생성기가 오류 코드로 종료되었습니다: {status}");
        }

        let lib_dir = base_abs.join("lib");
        let classpath = extract_bundled_jars(&jar_abs, &lib_dir, GAME_VERSION)?;
        let bundled_server = lib_dir.join(format!("server-{GAME_VERSION}.jar"));
        extract_resources(&bundled_server, &gen_abs)?;
        let count = extract_structures(&bundled_server, &gen_abs)?;
        tracing::info!(structures = count, "바닐라 구조물/리소스 추출 완료");

        if let Some(javac) = javac_bin {
            run_java_extractors(&java_bin, &javac, &classpath, &lib_dir, &gen_abs)?;
        } else {
            tracing::warn!(
                "javac를 찾지 못해 선택적 부가 메타데이터(advancements/recipes.json) 추출을 건너뛰었습니다. \
                 전체 기능을 사용하려면 Java JDK를 설치하세요."
            );
        }

        fs::write(gen_abs.join(".complete"), format!("{GAME_VERSION}\n"))
            .context(".complete 마커 기록 실패")?;
        Ok(gen_abs)
    })
    .await
    .context("바닐라 추출 작업 패닉")??;

    let final_gen = fs::canonicalize(&gen_dir).unwrap_or(gen_dir);
    ferrite_protocol::set_vanilla_generated_dir(final_gen.clone());
    tracing::info!(path = %final_gen.display(), "바닐라 26.3 데이터 추출이 완료되었습니다");
    Ok(final_gen)
}

fn hidden_command(program: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn sha1_hex(bytes: &[u8]) -> String {
    let digest = Sha1::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn file_sha1_hex(path: &Path) -> anyhow::Result<String> {
    let bytes = fs::read(path)?;
    Ok(sha1_hex(&bytes))
}

fn java_exe_name() -> &'static str {
    if cfg!(windows) { "java.exe" } else { "java" }
}

fn javac_exe_name() -> &'static str {
    if cfg!(windows) { "javac.exe" } else { "javac" }
}

/// Parses `java -version` output (written to stderr) into the major version number.
fn parse_java_major(stderr: &str) -> Option<u32> {
    let marker = stderr.find("version \"")?;
    let rest = &stderr[marker + 9..];
    let end = rest.find(['.', '"', '-', '_', '+'])?;
    rest[..end].parse::<u32>().ok()
}

fn probe_java_major(java: &Path) -> Option<u32> {
    let out = hidden_command(java).arg("-version").output().ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    parse_java_major(&text).or_else(|| parse_java_major(&String::from_utf8_lossy(&out.stdout)))
}

fn sibling_javac(java: &Path) -> Option<PathBuf> {
    let candidate = java.with_file_name(javac_exe_name());
    if candidate.is_file() {
        return Some(candidate);
    }
    None
}

fn find_java(min_major: u32) -> anyhow::Result<(PathBuf, Option<PathBuf>)> {
    let mut candidates = Vec::new();
    for var in ["FERRITE_JAVA25", "JAVA_HOME"] {
        if let Some(home) = std::env::var_os(var) {
            let bin = PathBuf::from(home).join("bin").join(java_exe_name());
            if bin.is_file() {
                candidates.push(bin);
            }
        }
    }
    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let bin = dir.join(java_exe_name());
            if bin.is_file() {
                candidates.push(bin);
            }
        }
    }
    #[cfg(windows)]
    {
        for root in [
            r"C:\Program Files\Eclipse Adoptium",
            r"C:\Program Files\Java",
            r"C:\Program Files\Microsoft",
            r"C:\Program Files\Zulu",
            r"C:\Program Files\Amazon Corretto",
            r"C:\Program Files\BellSoft",
        ] {
            if let Ok(entries) = fs::read_dir(root) {
                let mut subdirs: Vec<PathBuf> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect();
                subdirs.sort();
                subdirs.reverse();
                for dir in subdirs {
                    let bin = dir.join("bin").join(java_exe_name());
                    if bin.is_file() {
                        candidates.push(bin);
                    }
                }
            }
        }
    }
    #[cfg(unix)]
    {
        if let Ok(entries) = fs::read_dir("/usr/lib/jvm") {
            let mut subdirs: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            subdirs.sort();
            subdirs.reverse();
            for dir in subdirs {
                let bin = dir.join("bin").join(java_exe_name());
                if bin.is_file() {
                    candidates.push(bin);
                }
            }
        }
    }

    let mut seen = HashSet::new();
    let mut fallback_jre = None;
    for candidate in candidates {
        let canonical = fs::canonicalize(&candidate).unwrap_or(candidate);
        if !seen.insert(canonical.clone()) {
            continue;
        }
        if let Some(major) = probe_java_major(&canonical)
            && major >= min_major
        {
            if let Some(javac) = sibling_javac(&canonical) {
                return Ok((canonical, Some(javac)));
            }
            if fallback_jre.is_none() {
                fallback_jre = Some(canonical);
            }
        }
    }
    if let Some(java) = fallback_jre {
        return Ok((java, None));
    }
    bail!("Java {min_major}+가 설치되어 있지 않습니다")
}

struct ZipEntry {
    name: String,
    compression_method: u16,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
}

fn read_u16_le(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn read_u32_le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

struct SimpleZip {
    data: Vec<u8>,
    entries: Vec<ZipEntry>,
}

impl SimpleZip {
    fn open(path: &Path) -> anyhow::Result<Self> {
        let data = fs::read(path).with_context(|| format!("ZIP 열기 실패: {}", path.display()))?;
        let entries = parse_zip_directory(&data)?;
        Ok(Self { data, entries })
    }

    fn read_entry(&self, target: &str) -> anyhow::Result<Vec<u8>> {
        let entry = self
            .entries
            .iter()
            .find(|e| e.name == target)
            .with_context(|| format!("ZIP 내 엔트리 없음: {target}"))?;
        self.decode_entry(entry)
    }

    fn decode_entry(&self, entry: &ZipEntry) -> anyhow::Result<Vec<u8>> {
        let off =
            usize::try_from(entry.local_header_offset).context("ZIP 로컬 헤더 오프셋 범위 초과")?;
        let header = self
            .data
            .get(off..off + 30)
            .context("ZIP 로컬 헤더가 잘렸습니다")?;
        if &header[0..4] != b"PK\x03\x04" {
            bail!("잘못된 ZIP 로컬 헤더 시그니처 ({})", entry.name);
        }
        let name_len = usize::from(read_u16_le(&header[26..28]));
        let extra_len = usize::from(read_u16_le(&header[28..30]));
        let payload_start = off + 30 + name_len + extra_len;
        let comp_len = usize::try_from(entry.compressed_size).context("ZIP 압축 크기 범위 초과")?;
        let payload = self
            .data
            .get(payload_start..payload_start + comp_len)
            .with_context(|| format!("ZIP 엔트리 데이터가 잘렸습니다: {}", entry.name))?;

        match entry.compression_method {
            0 => Ok(payload.to_vec()),
            8 => {
                let mut decoder = DeflateDecoder::new(Cursor::new(payload));
                let cap = usize::try_from(entry.uncompressed_size).unwrap_or(0);
                let mut out = Vec::with_capacity(cap);
                decoder
                    .read_to_end(&mut out)
                    .with_context(|| format!("Deflate 해제 실패: {}", entry.name))?;
                Ok(out)
            }
            method => bail!("지원되지 않는 ZIP 압축 방식 {method} ({})", entry.name),
        }
    }
}

fn parse_zip_directory(data: &[u8]) -> anyhow::Result<Vec<ZipEntry>> {
    if data.len() < 22 {
        bail!("ZIP 파일이 너무 작습니다");
    }
    let min_eocd = data.len().saturating_sub(22 + 65535);
    let mut eocd_pos = None;
    let mut i = data.len() - 22;
    loop {
        if &data[i..i + 4] == b"PK\x05\x06" {
            let comment_len = usize::from(read_u16_le(&data[i + 20..i + 22]));
            if i + 22 + comment_len == data.len() {
                eocd_pos = Some(i);
                break;
            }
        }
        if i == min_eocd {
            break;
        }
        i -= 1;
    }
    let eocd = eocd_pos.context("ZIP End-of-Central-Directory를 찾을 수 없습니다")?;
    let entry_count = usize::from(read_u16_le(&data[eocd + 10..eocd + 12]));
    let cd_size = usize::try_from(read_u32_le(&data[eocd + 12..eocd + 16]))?;
    let cd_offset = usize::try_from(read_u32_le(&data[eocd + 16..eocd + 20]))?;
    if entry_count == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
        bail!("ZIP64 아카이브는 지원되지 않습니다");
    }
    let mut pos = cd_offset;
    let cd_end = cd_offset
        .checked_add(cd_size)
        .context("ZIP CD 오버플로우")?;
    if cd_end > data.len() {
        bail!("ZIP Central Directory가 파일 끝을 넘었습니다");
    }
    let mut out = Vec::with_capacity(entry_count);
    while pos < cd_end {
        let rec = data
            .get(pos..pos + 46)
            .context("ZIP CD 레코드가 잘렸습니다")?;
        if &rec[0..4] != b"PK\x01\x02" {
            bail!("잘못된 ZIP CD 시그니처");
        }
        let compression_method = read_u16_le(&rec[10..12]);
        let compressed_size = u64::from(read_u32_le(&rec[20..24]));
        let uncompressed_size = u64::from(read_u32_le(&rec[24..28]));
        let name_len = usize::from(read_u16_le(&rec[28..30]));
        let extra_len = usize::from(read_u16_le(&rec[30..32]));
        let comment_len = usize::from(read_u16_le(&rec[32..34]));
        let local_header_offset = u64::from(read_u32_le(&rec[42..46]));
        let name_bytes = data
            .get(pos + 46..pos + 46 + name_len)
            .context("ZIP 파일명이 잘렸습니다")?;
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        out.push(ZipEntry {
            name,
            compression_method,
            compressed_size,
            uncompressed_size,
            local_header_offset,
        });
        pos += 46 + name_len + extra_len + comment_len;
    }
    Ok(out)
}

fn safe_rel_path(rel: &str) -> anyhow::Result<PathBuf> {
    let mut clean = PathBuf::new();
    for comp in Path::new(rel).components() {
        match comp {
            Component::Normal(part) => clean.push(part),
            _ => bail!("허용되지 않는 상대 경로: {rel}"),
        }
    }
    if clean.as_os_str().is_empty() {
        bail!("빈 경로");
    }
    Ok(clean)
}

fn extract_bundled_jars(
    outer_jar: &Path,
    lib_dir: &Path,
    version: &str,
) -> anyhow::Result<Vec<PathBuf>> {
    fs::create_dir_all(lib_dir)?;
    let outer = SimpleZip::open(outer_jar)?;
    let versions_raw = outer.read_entry("META-INF/versions.list")?;
    let versions_text = String::from_utf8_lossy(&versions_raw);
    let versions: Vec<&str> = versions_text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if versions.len() != 1 {
        bail!("expected one bundled server jar, found {}", versions.len());
    }
    let version_entry = versions[0]
        .split('\t')
        .nth(2)
        .context("META-INF/versions.list 형식 오류")?;
    let server_path = lib_dir.join(format!("server-{version}.jar"));
    if !server_path.is_file() {
        let bytes = outer.read_entry(&format!("META-INF/versions/{version_entry}"))?;
        fs::write(&server_path, bytes)?;
    }

    let mut classpath = vec![server_path];
    let libraries_dir = lib_dir.join("libraries");
    let libs_raw = outer.read_entry("META-INF/libraries.list")?;
    for line in String::from_utf8_lossy(&libs_raw).lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(rel) = trimmed.split('\t').nth(2) else {
            continue;
        };
        let dest = libraries_dir.join(safe_rel_path(rel)?);
        if !dest.is_file() {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            let bytes = outer.read_entry(&format!("META-INF/libraries/{rel}"))?;
            fs::write(&dest, bytes)?;
        }
        classpath.push(dest);
    }
    Ok(classpath)
}

fn extract_resources(server_jar: &Path, gen_dir: &Path) -> anyhow::Result<()> {
    let zip = SimpleZip::open(server_jar)?;
    let reports = gen_dir.join("reports");
    fs::create_dir_all(&reports)?;
    fs::write(
        reports.join("lang_en_us.json"),
        zip.read_entry("assets/minecraft/lang/en_us.json")?,
    )?;
    fs::write(
        reports.join("version.json"),
        zip.read_entry("version.json")?,
    )?;
    Ok(())
}

fn extract_structures(server_jar: &Path, gen_dir: &Path) -> anyhow::Result<usize> {
    let zip = SimpleZip::open(server_jar)?;
    let target = gen_dir.join("data/minecraft/structure");
    let prefix = "data/minecraft/structure/";
    let mut count = 0;
    for entry in &zip.entries {
        if let Some(rel) = entry.name.strip_prefix(prefix)
            && entry.name.ends_with(".nbt")
        {
            let dest = target.join(safe_rel_path(rel)?);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(dest, zip.decode_entry(entry)?)?;
            count += 1;
        }
    }
    Ok(count)
}

fn run_java_extractors(
    java: &Path,
    javac: &Path,
    classpath: &[PathBuf],
    lib_dir: &Path,
    gen_dir: &Path,
) -> anyhow::Result<()> {
    let cp = std::env::join_paths(classpath).context("Java classpath 조합 실패")?;
    let src_dir = lib_dir.join("extractor-src");
    let classes_dir = lib_dir.join("extractor-classes");
    fs::create_dir_all(&src_dir)?;
    fs::create_dir_all(&classes_dir)?;

    let files = [
        ("Extractor.java", EXTRACTOR_JAVA),
        ("PistonExtractor.java", PISTON_EXTRACTOR_JAVA),
        ("AdvancementExtractor.java", ADVANCEMENT_EXTRACTOR_JAVA),
        ("RecipeExtractor.java", RECIPE_EXTRACTOR_JAVA),
    ];
    let mut java_paths = Vec::with_capacity(files.len());
    for (name, content) in files {
        let path = src_dir.join(name);
        fs::write(&path, content)?;
        java_paths.push(path);
    }

    tracing::info!("Ferrite 바닐라 메타데이터 extractor 컴파일 중...");
    let status = hidden_command(javac)
        .arg("-d")
        .arg(&classes_dir)
        .arg("-cp")
        .arg(&cp)
        .args(&java_paths)
        .status()
        .context("javac 실행 실패")?;
    if !status.success() {
        bail!("javac 컴파일 실패: {status}");
    }

    let mut run_cp_paths = Vec::with_capacity(classpath.len() + 1);
    run_cp_paths.push(classes_dir);
    run_cp_paths.extend_from_slice(classpath);
    let run_cp = std::env::join_paths(&run_cp_paths).context("실행 classpath 조합 실패")?;

    let ferrite_dir = gen_dir.join("ferrite");
    fs::create_dir_all(&ferrite_dir)?;

    let run = |main_class: &str, args: &[PathBuf]| -> anyhow::Result<()> {
        let status = hidden_command(java)
            .arg("-cp")
            .arg(&run_cp)
            .arg(main_class)
            .args(args)
            .status()
            .with_context(|| format!("{main_class} 실행 실패"))?;
        if !status.success() {
            bail!("{main_class} 종료 오류: {status}");
        }
        Ok(())
    };

    run(
        "Extractor",
        &[
            ferrite_dir.clone(),
            gen_dir.join("reports/minecraft/components/item"),
        ],
    )?;
    run("PistonExtractor", &[ferrite_dir.join("piston_states.json")])?;
    run(
        "AdvancementExtractor",
        &[gen_dir.join("data"), ferrite_dir.join("advancements.json")],
    )?;
    run("RecipeExtractor", &[ferrite_dir.join("recipes.json")])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_temurin_and_openjdk_version_strings() {
        let temurin = "openjdk version \"25.0.5\" 2026-10-20\nOpenJDK Runtime Environment";
        let prerelease = "openjdk version \"26-ea\" 2026-03-17";
        let old = "openjdk version \"21.0.4\" 2024-07-16";
        assert_eq!(parse_java_major(temurin), Some(25));
        assert_eq!(parse_java_major(prerelease), Some(26));
        assert_eq!(parse_java_major(old), Some(21));
    }

    #[test]
    fn simple_zip_reads_stored_entries() {
        // Minimal valid single-entry stored ZIP built in memory ("hello.txt" -> b"world").
        let name = b"hello.txt";
        let payload = b"world";
        let mut buf = Vec::new();
        // Local file header (offset 0)
        buf.extend_from_slice(b"PK\x03\x04");
        buf.extend_from_slice(&20u16.to_le_bytes()); // version needed
        buf.extend_from_slice(&0u16.to_le_bytes()); // flags
        buf.extend_from_slice(&0u16.to_le_bytes()); // method = stored
        buf.extend_from_slice(&0u16.to_le_bytes()); // mod time
        buf.extend_from_slice(&0u16.to_le_bytes()); // mod date
        buf.extend_from_slice(&0u32.to_le_bytes()); // crc32 (unchecked)
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // extra len
        buf.extend_from_slice(name);
        buf.extend_from_slice(payload);

        let cd_offset = buf.len() as u32;
        buf.extend_from_slice(b"PK\x01\x02");
        buf.extend_from_slice(&20u16.to_le_bytes());
        buf.extend_from_slice(&20u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // local offset = 0
        buf.extend_from_slice(name);

        let cd_size = (buf.len() as u32) - cd_offset;
        buf.extend_from_slice(b"PK\x05\x06");
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&cd_size.to_le_bytes());
        buf.extend_from_slice(&cd_offset.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());

        let entries = parse_zip_directory(&buf).unwrap();
        let zip = SimpleZip { data: buf, entries };
        assert_eq!(zip.read_entry("hello.txt").unwrap(), b"world");
        assert!(safe_rel_path("../evil").is_err());
        assert!(safe_rel_path("a/b/c.nbt").is_ok());
    }

    #[test]
    fn simple_zip_reads_real_deflated_vanilla_jar_when_present() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vanilla/26.3");
        let jar = base.join("server.jar");
        let inner = base.join("lib/server-26.3.jar");
        if !jar.is_file() || !inner.is_file() {
            return;
        }
        let outer = SimpleZip::open(&jar).expect("open outer server.jar");
        let list = outer
            .read_entry("META-INF/versions.list")
            .expect("versions.list");
        assert!(!list.is_empty());

        let bundled = SimpleZip::open(&inner).expect("open bundled server jar");
        let version_json = bundled.read_entry("version.json").expect("version.json");
        assert!(String::from_utf8_lossy(&version_json).contains("26.3"));
        let lang = bundled
            .read_entry("assets/minecraft/lang/en_us.json")
            .expect("lang_en_us");
        assert!(lang.len() > 1000);
    }
}
