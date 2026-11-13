"""Compile an original adapter against the already-fetched official jar and decode damage types.

Usage: python tools/extract_damage_types.py vanilla/26.3 build/output/damage-types.json
No downloads, worlds, server/EULA startup or committed extraction output.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess


def extract(version_dir: Path, output: Path) -> None:
    java_home = os.environ.get("JAVA_HOME")
    java = str(Path(java_home) / "bin/java") if java_home else shutil.which("java")
    if not java:
        raise SystemExit("Java 25 JDK required")
    javac = str(Path(java).with_name("javac.exe" if os.name == "nt" else "javac"))
    jar = version_dir / "lib" / f"server-{version_dir.name}.jar"
    if not jar.is_file():
        raise SystemExit(f"Missing {jar}; run tools/fetch_vanilla.py first")
    classpath = os.pathsep.join(str(p.resolve()) for p in [jar, *sorted((version_dir / "lib/libraries").rglob("*.jar"))])
    source = Path(__file__).with_name("extractor") / "DamageTypeExtractor.java"
    classes = output.parent / "damage-extractor-classes"
    classes.mkdir(parents=True, exist_ok=True)
    subprocess.run([javac, "-cp", classpath, "-d", str(classes), str(source)], check=True)
    data = version_dir / "generated/data/minecraft/damage_type"
    subprocess.run([java, "-cp", os.pathsep.join([str(classes), classpath]), "DamageTypeExtractor", str(data), str(output)], cwd=classes, check=True)
    result = json.loads(output.read_text(encoding="utf-8"))
    expected = {"minecraft:" + str(p.relative_to(data).with_suffix("")).replace("\\", "/") for p in data.rglob("*.json")}
    if result["version"] != version_dir.name or set(result["types"]) != expected:
        raise SystemExit("Extracted damage-type version/keys differ from input data")
    print(f"Decoded {len(expected)} official damage types and {len(result['effects'])} sound effects")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version_dir", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    extract(args.version_dir.resolve(), args.output.resolve())
