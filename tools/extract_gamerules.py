"""Extract typed game-rule defaults from an already fetched official jar; never starts a server.

Usage: python tools/extract_gamerules.py vanilla/26.3 output.json [--verify]
--verify independently enumerates official public GameRules fields and compares both outputs.
All classes, extracted values and verification output stay beside the ignored output file.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess


def extract(version_dir: Path, output: Path, verify: bool) -> None:
    source = Path(__file__).with_name("extractor") / "GameRuleExtractor.java"
    java_home = os.environ.get("JAVA_HOME")
    java = str(Path(java_home) / "bin" / "java") if java_home else shutil.which("java")
    if not java:
        raise SystemExit("Java 25 JDK required (set JAVA_HOME)")
    javac = str(Path(java).with_name("javac.exe" if os.name == "nt" else "javac"))
    server = version_dir / "lib" / f"server-{version_dir.name}.jar"
    if not server.is_file():
        raise SystemExit(f"Missing {server}; run tools/fetch_vanilla.py first")
    libraries = sorted((version_dir / "lib" / "libraries").rglob("*.jar"))
    classpath = os.pathsep.join(str(p.resolve()) for p in [server, *libraries])
    classes = output.parent / "gamerule-extractor-classes"
    classes.mkdir(parents=True, exist_ok=True)
    subprocess.run([javac, "-cp", classpath, "-d", str(classes), str(source)], check=True)
    command = [java, "-cp", os.pathsep.join([str(classes.resolve()), classpath]), "GameRuleExtractor"]
    subprocess.run([*command, str(output.resolve())], cwd=classes, check=True)
    report = json.loads((version_dir / "generated/reports/registries.json").read_text(encoding="utf-8"))["minecraft:game_rule"]["entries"]
    actual = json.loads(output.read_text(encoding="utf-8"))
    if actual["version"] != version_dir.name:
        raise SystemExit("Official jar version differs from requested data version")
    if set(actual["rules"]) != set(report) or any(v["protocol_id"] != report[k]["protocol_id"] for k, v in actual["rules"].items()):
        raise SystemExit("Official game-rule registry differs from generated report")
    if verify:
        reference = output.with_name("game-rules-reference.json")
        subprocess.run([*command, str(reference.resolve()), "--fields"], cwd=classes, check=True)
        if actual != json.loads(reference.read_text(encoding="utf-8")):
            raise SystemExit("Registry and independent official GameRules fields disagree")
    print(f"Verified {len(report)} official typed game rules -> {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version_dir", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--verify", action="store_true")
    args = parser.parse_args()
    extract(args.version_dir.resolve(), args.output.resolve(), args.verify)
