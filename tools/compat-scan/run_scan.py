#!/usr/bin/env python3
"""Download third-party open-source plugin jars into gitignored `compat/` and
run static compatibility scans against Ferrite's built Java runtime classpath.
"""

import hashlib
import json
import os
import pathlib
import subprocess
import sys
import urllib.error
import urllib.request
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent.parent
COMPAT = ROOT / "compat"
SCANNER = ROOT / "tools" / "compat-scan" / "CompatScanner.java"

ARTIFACTS = [
    {
        "name": "worldedit-core",
        "maven_central_url": "https://repo1.maven.org/maven2/com/sk89q/worldedit/worldedit-core/maven-metadata.xml",
        "coordinates": "com.sk89q.worldedit:worldedit-core:7.4.6",
        "url": "https://maven.enginehub.org/repo/com/sk89q/worldedit/worldedit-core/7.4.6/worldedit-core-7.4.6.jar",
        "filename": "worldedit-core-7.4.6.jar",
        "source": "EngineHub Maven (not on Maven Central)",
    },
    {
        "name": "worldedit-bukkit-maven",
        "maven_central_url": "https://repo1.maven.org/maven2/com/sk89q/worldedit/worldedit-bukkit/maven-metadata.xml",
        "coordinates": "com.sk89q.worldedit:worldedit-bukkit:7.4.6",
        "url": "https://maven.enginehub.org/repo/com/sk89q/worldedit/worldedit-bukkit/7.4.6/worldedit-bukkit-7.4.6.jar",
        "filename": "worldedit-bukkit-7.4.6-maven.jar",
        "source": "EngineHub Maven (not on Maven Central)",
    },
    {
        "name": "worldedit-bukkit-dist",
        "maven_central_url": None,
        "coordinates": "org.enginehub.worldedit:worldedit-bukkit:7.4.6 (Modrinth dist)",
        "url": "https://cdn.modrinth.com/data/1u6JkXh5/versions/YFnqiUlL/worldedit-bukkit-7.4.6.jar",
        "filename": "worldedit-bukkit-7.4.6-dist.jar",
        "source": "Modrinth (shaded plugin jar for server boot)",
    },
    {
        "name": "ProtocolLib",
        "maven_central_url": "https://repo1.maven.org/maven2/net/dmulloy2/ProtocolLib/maven-metadata.xml",
        "coordinates": "net.dmulloy2:ProtocolLib:5.4.0",
        "url": "https://repo1.maven.org/maven2/net/dmulloy2/ProtocolLib/5.4.0/ProtocolLib-5.4.0.jar",
        "filename": "ProtocolLib-5.4.0.jar",
        "source": "Maven Central",
    },
    {
        "name": "VaultAPI",
        "maven_central_url": "https://repo1.maven.org/maven2/com/github/MilkBowl/VaultAPI/maven-metadata.xml",
        "coordinates": "com.github.MilkBowl:VaultAPI:1.7.1",
        "url": "https://jitpack.io/com/github/MilkBowl/VaultAPI/1.7.1/VaultAPI-1.7.1.jar",
        "filename": "VaultAPI-1.7.1.jar",
        "source": "JitPack (not on Maven Central)",
    },
    {
        "name": "LuckPerms-api",
        "maven_central_url": "https://repo1.maven.org/maven2/net/luckperms/api/maven-metadata.xml",
        "coordinates": "net.luckperms:api:5.5",
        "url": "https://repo1.maven.org/maven2/net/luckperms/api/5.5/api-5.5.jar",
        "filename": "LuckPerms-api-5.5.jar",
        "source": "Maven Central",
    },
    {
        "name": "LuckPerms-Bukkit",
        "maven_central_url": None,
        "coordinates": "net.luckperms:luckperms-bukkit:5.5.71 (Modrinth)",
        "url": "https://cdn.modrinth.com/data/Vebnzrzj/versions/b0mk8uS6/LuckPerms-Bukkit-5.5.71.jar",
        "filename": "LuckPerms-Bukkit-5.5.71.jar",
        "source": "Modrinth (full Bukkit plugin jar)",
    },
    {
        "name": "PlaceholderAPI",
        "maven_central_url": "https://repo1.maven.org/maven2/me/clip/placeholderapi/maven-metadata.xml",
        "coordinates": "me.clip:placeholderapi:2.12.3",
        "url": "https://repo.extendedclip.com/releases/me/clip/placeholderapi/2.12.3/placeholderapi-2.12.3.jar",
        "filename": "PlaceholderAPI-2.12.3.jar",
        "source": "ExtendedClip Maven (not on Maven Central)",
    },
]


def check_maven_central(url: str | None) -> str:
    if not url:
        return "n/a"
    try:
        req = urllib.request.Request(url, method="HEAD", headers={"User-Agent": "ferrite-compat-scan/1.0"})
        with urllib.request.urlopen(req, timeout=10) as r:
            return f"HTTP {r.status}"
    except urllib.error.HTTPError as e:
        return f"HTTP {e.code} {e.reason}"
    except Exception as e:
        return f"ERR {e}"


def download_if_needed(url: str, dest: pathlib.Path) -> tuple[bool, str]:
    if dest.is_file() and dest.stat().st_size > 0:
        return True, "cached"
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "ferrite-compat-scan/1.0"})
        with urllib.request.urlopen(req, timeout=60) as r:
            data = r.read()
        dest.write_bytes(data)
        return True, f"HTTP {r.status}"
    except urllib.error.HTTPError as e:
        return False, f"HTTP {e.code} {e.reason}"
    except Exception as e:
        return False, f"ERR {e}"


def sha256_file(path: pathlib.Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def extract_plugin_descriptor(jar_path: pathlib.Path) -> dict[str, str]:
    info: dict[str, str] = {}
    with zipfile.ZipFile(jar_path, "r") as zf:
        for candidate in ("plugin.yml", "paper-plugin.yml"):
            if candidate in zf.namelist():
                text = zf.read(candidate).decode("utf-8", errors="replace")
                info["descriptor"] = candidate
                for line in text.splitlines():
                    for key in ("name", "version", "api-version", "main"):
                        if line.startswith(f"{key}:"):
                            val = line.split(":", 1)[1].strip().strip("'\"")
                            info.setdefault(key, val)
                break
    return info


def find_single_jar(directory: pathlib.Path, prefix: str) -> pathlib.Path:
    for p in sorted(directory.glob(f"{prefix}*.jar")):
        if "-sources" not in p.name and "-javadoc" not in p.name:
            return p
    raise FileNotFoundError(f"missing {prefix}*.jar in {directory}")


def build_runtime_classpath() -> list[str]:
    java_dir = ROOT / "java"
    host_jar = find_single_jar(java_dir / "ferrite-host" / "build" / "libs", "ferrite-host")
    api_impl_jar = find_single_jar(java_dir / "ferrite-api-impl" / "build" / "libs", "ferrite-api-impl")
    nms_jar = find_single_jar(java_dir / "ferrite-nms" / "build" / "libs", "ferrite-nms")
    cp_file = java_dir / "ferrite-api-impl" / "build" / "paper-api-classpath.txt"
    cp = [str(host_jar), str(api_impl_jar), str(nms_jar)]
    sep = ";" if os.name == "nt" else ":"
    for entry in cp_file.read_text(encoding="utf-8").split(sep):
        entry = entry.strip()
        if entry and pathlib.Path(entry).is_file():
            cp.append(entry)
    return cp


def main() -> int:
    COMPAT.mkdir(parents=True, exist_ok=True)
    cp = build_runtime_classpath()
    results = []

    for art in ARTIFACTS:
        mc_status = check_maven_central(art["maven_central_url"])
        dest = COMPAT / art["filename"]
        ok, dl_status = download_if_needed(art["url"], dest)
        record = {
            "name": art["name"],
            "coordinates": art["coordinates"],
            "source": art["source"],
            "maven_central_probe": mc_status,
            "url": art["url"],
            "download_status": dl_status,
        }
        if not ok:
            results.append(record)
            continue

        record["sha256"] = sha256_file(dest)
        record["size_bytes"] = dest.stat().st_size
        record["plugin_meta"] = extract_plugin_descriptor(dest)

        scan_proc = subprocess.run(
            ["java", str(SCANNER), str(dest), *cp],
            capture_output=True,
            text=True,
            check=True,
        )
        record["scan_output"] = scan_proc.stdout
        print(scan_proc.stdout)

        jdeps_proc = subprocess.run(
            ["jdeps", "--ignore-missing-deps", "--summary", str(dest)],
            capture_output=True,
            text=True,
        )
        record["jdeps_summary"] = jdeps_proc.stdout.strip() or jdeps_proc.stderr.strip()
        results.append(record)

    out_path = COMPAT / "scan-results.json"
    out_path.write_text(json.dumps(results, indent=2), encoding="utf-8")
    print(f"Wrote {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
