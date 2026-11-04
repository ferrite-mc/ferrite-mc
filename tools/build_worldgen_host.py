#!/usr/bin/env python3
"""Compile the Ferrite world generation host against the locally downloaded server jar.

The Minecraft jar is only used as a compile-time class path. The output jar contains our own
classes only; no Mojang code is copied into it. Run `python tools/fetch_vanilla.py` first.

Usage: python tools/build_worldgen_host.py [--vanilla vanilla/26.3]
"""
import argparse
import os
import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DEFAULT_JAVA_HOME = "C:/Program Files/Eclipse Adoptium/jdk-25.0.5.8-hotspot"


def java_major(home: pathlib.Path) -> int:
    release = home / "release"
    try:
        for line in release.read_text(encoding="utf-8").splitlines():
            if line.startswith("JAVA_VERSION="):
                return int(line.split("=", 1)[1].strip('"').split(".")[0])
    except (OSError, ValueError):
        pass
    return 0


def java_home() -> pathlib.Path:
    """The first JDK 25+ among FERRITE_JAVA_HOME, JAVA_HOME and the default install."""
    for candidate in (os.environ.get("FERRITE_JAVA_HOME"), os.environ.get("JAVA_HOME"),
                      DEFAULT_JAVA_HOME):
        if candidate and java_major(pathlib.Path(candidate)) >= 25:
            return pathlib.Path(candidate)
    sys.exit("Java 25+ not found; set FERRITE_JAVA_HOME")


def java_tool(name: str) -> str:
    return str(java_home() / "bin" / (name + (".exe" if os.name == "nt" else "")))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--vanilla", default="vanilla/26.3")
    args = parser.parse_args()

    lib = ROOT / args.vanilla / "lib"
    server_jar = lib / "server-26.3.jar"
    if not server_jar.exists():
        print(f"missing {server_jar}; run tools/fetch_vanilla.py first", file=sys.stderr)
        return 1
    classpath = [str(server_jar)] + [str(p) for p in (lib / "libraries").rglob("*.jar")]

    source_root = ROOT / "java" / "ferrite-worldgen-host" / "src" / "main" / "java"
    sources = [str(p) for p in source_root.rglob("*.java")]
    out = ROOT / "target" / "worldgen-host"
    classes = out / "classes"
    if classes.exists():
        shutil.rmtree(classes)
    classes.mkdir(parents=True)

    subprocess.run(
        [java_tool("javac"), "-encoding", "UTF-8", "--release", "25",
         "-cp", os.pathsep.join(classpath), "-d", str(classes), *sources],
        check=True,
    )
    jar = out / "ferrite-worldgen-host.jar"
    subprocess.run([java_tool("jar"), "--create", "--file", str(jar), "-C", str(classes), "."],
                   check=True)
    print(f"built {jar}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
