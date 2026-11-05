"""Download the official vanilla server jar and run its data generator.

Usage:  python tools/fetch_vanilla.py [version]      (default: version in ferrite-protocol)

Output (gitignored):
  vanilla/<version>/server.jar
  vanilla/<version>/generated/reports/{packets,registries,blocks,items,...}.json
  vanilla/<version>/generated/data/minecraft/...   (worldgen, loot tables, recipes, tags)
  vanilla/<version>/generated/ferrite/block_states.json, items inside it + entity_types.json
  vanilla/<version>/lib/                          (bundled server jar + libraries, extractor input)

Nothing produced here may be committed; the repo only ships code that reads it.
"""

import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import urllib.request
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFEST = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
EXTRACTOR = ROOT / "tools" / "extractor" / "Extractor.java"


def default_version() -> str:
    src = (ROOT / "crates/ferrite-protocol/src/lib.rs").read_text(encoding="utf-8")
    return re.search(r'GAME_VERSION: &str = "([^"]+)"', src).group(1)


def fetch_json(url: str) -> dict:
    with urllib.request.urlopen(url) as r:
        return json.load(r)


def download(url: str, dest: pathlib.Path, sha1: str) -> None:
    if dest.exists() and hashlib.sha1(dest.read_bytes()).hexdigest() == sha1:
        return
    print(f"downloading {url}")
    with urllib.request.urlopen(url) as r:
        data = r.read()
    if hashlib.sha1(data).hexdigest() != sha1:
        sys.exit(f"sha1 mismatch for {url}")
    dest.write_bytes(data)


def find_java(major: int) -> str:
    """Prefer JAVA_HOME, then PATH; warn if the major version is too old."""
    java = shutil.which("java")
    if os.environ.get("JAVA_HOME"):
        java = str(pathlib.Path(os.environ["JAVA_HOME"]) / "bin" / "java")
    if not java:
        sys.exit("java not found")
    out = subprocess.run([java, "-version"], capture_output=True, text=True).stderr
    found = int(re.search(r'version "(\d+)', out).group(1))
    if found < major:
        sys.exit(f"{version_label} needs Java {major}+, found {found} at {java}")
    return java


def extract_bundled(jar: pathlib.Path, lib: pathlib.Path, version: str) -> list[pathlib.Path]:
    """Extract the bundled server jar and its libraries from the bootstrapper jar.

    `lib/` keeps the extractor's classpath together and outside the game directory, so
    rerunning the data generator cannot clobber it.
    """
    lib.mkdir(parents=True, exist_ok=True)
    classpath: list[pathlib.Path] = []
    with zipfile.ZipFile(jar) as outer:
        versions = [
            line.split("\t")
            for line in outer.read("META-INF/versions.list").decode().splitlines()
            if line.strip()
        ]
        if len(versions) != 1:
            sys.exit(f"expected one bundled server jar, found {len(versions)}")
        server = lib / f"server-{version}.jar"
        if not server.exists():
            server.write_bytes(outer.read("META-INF/versions/" + versions[0][2]))
        classpath.append(server)

        libraries = lib / "libraries"
        for line in outer.read("META-INF/libraries.list").decode().splitlines():
            if not line.strip():
                continue
            path = line.split("\t")[2]
            library = libraries / path
            if not library.exists():
                library.parent.mkdir(parents=True, exist_ok=True)
                library.write_bytes(outer.read("META-INF/libraries/" + path))
            classpath.append(library)
    return classpath


def run_extractor(java: str, version: str, out: pathlib.Path) -> None:
    """Compile and run tools/extractor against the bundled server jar in vanilla/<ver>/lib."""
    javac = pathlib.Path(java).with_name("javac.exe" if os.name == "nt" else "javac")
    if not javac.exists():
        sys.exit(f"ferrite extractor requires a full JDK: {javac} not found")
    lib = out / "lib"
    classes = lib / "extractor-classes"
    classes.mkdir(parents=True, exist_ok=True)
    classpath = os.pathsep.join(
        str(path) for path in extract_bundled(out / "server.jar", lib, version)
    )
    print("compiling tools/extractor/Extractor.java")
    subprocess.run(
        [str(javac), "-d", str(classes), "-cp", classpath, str(EXTRACTOR)],
        check=True,
    )
    print("extracting block, item and entity properties")
    subprocess.run(
        [java, "-cp", os.pathsep.join([str(classes), classpath]), "Extractor",
         str(out / "generated" / "ferrite"),
         str(out / "generated" / "reports" / "minecraft" / "components" / "item")],
        check=True,
    )


def extract_resources(out: pathlib.Path, version: str) -> None:
    """Copy the English translations and version.json out of the user's own server jar.

    The server reads them at startup for command feedback and `/version`, so no Mojang text is
    kept in this repository.
    """
    reports = out / "generated" / "reports"
    reports.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out / "lib" / f"server-{version}.jar") as server:
        (reports / "lang_en_us.json").write_bytes(server.read("assets/minecraft/lang/en_us.json"))
        (reports / "version.json").write_bytes(server.read("version.json"))


def extract_structures(out: pathlib.Path, version: str) -> int:
    """Copy the structure templates (`/place template`) out of the user's own server jar."""
    target = out / "generated" / "data" / "minecraft" / "structure"
    prefix = "data/minecraft/structure/"
    count = 0
    with zipfile.ZipFile(out / "lib" / f"server-{version}.jar") as server:
        for name in server.namelist():
            if name.startswith(prefix) and name.endswith(".nbt"):
                path = target / name[len(prefix):]
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(server.read(name))
                count += 1
    return count


def main() -> None:
    global version_label
    version = sys.argv[1] if len(sys.argv) > 1 else default_version()
    version_label = f"Minecraft {version}"
    entry = next((v for v in fetch_json(MANIFEST)["versions"] if v["id"] == version), None)
    if entry is None:
        sys.exit(f"unknown version {version}")
    meta = fetch_json(entry["url"])
    out = ROOT / "vanilla" / version
    out.mkdir(parents=True, exist_ok=True)
    jar = out / "server.jar"
    server = meta["downloads"]["server"]
    download(server["url"], jar, server["sha1"])

    java = find_java(meta.get("javaVersion", {}).get("majorVersion", 21))
    gen = out / "generated"
    if gen.exists():
        shutil.rmtree(gen)
    subprocess.run(
        [java, "-DbundlerMainClass=net.minecraft.data.Main", "-jar", str(jar),
         "--reports", "--server", "--output", str(gen)],
        cwd=out, check=True,
    )
    run_extractor(java, version, out)
    extract_resources(out, version)
    print(f"extracted {extract_structures(out, version)} structure templates")
    print(f"ok: {gen}")


if __name__ == "__main__":
    main()
