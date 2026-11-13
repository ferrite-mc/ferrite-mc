"""Read static POI state sets/ticket capacities from an already-fetched official jar; no world/server.

Usage: python tools/extract_poi_types.py vanilla/26.3 build/poi-types.json
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
    server = version_dir / "lib" / f"server-{version_dir.name}.jar"
    if not server.is_file():
        raise SystemExit(f"Missing {server}; run tools/fetch_vanilla.py first")
    classpath = os.pathsep.join(str(p.resolve()) for p in [server, *sorted((version_dir / "lib/libraries").rglob("*.jar"))])
    source = Path(__file__).with_name("extractor") / "PoiTypeExtractor.java"
    classes = output.parent / "poi-extractor-classes"
    classes.mkdir(parents=True, exist_ok=True)
    subprocess.run([javac, "-cp", classpath, "-d", str(classes), str(source)], check=True)
    subprocess.run([java, "-cp", os.pathsep.join([str(classes), classpath]), "PoiTypeExtractor", str(output)], cwd=classes, check=True)
    result = json.loads(output.read_text(encoding="utf-8"))
    report = json.loads((version_dir / "generated/reports/registries.json").read_text(encoding="utf-8"))["minecraft:point_of_interest_type"]["entries"]
    if result["version"] != version_dir.name or set(result["types"]) != set(report):
        raise SystemExit("Official POI version/keys differ from generated report")
    print(f"Verified {len(report)} POI definitions against all {result['state_count']} official block states")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version_dir", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    extract(args.version_dir.resolve(), args.output.resolve())
