"""Extract registry identifiers from local vanilla reports; generated output stays under build/."""
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
out = pathlib.Path(sys.argv[2])
report = root / "reports" / "registries.json"
if not report.exists():
    raise SystemExit(f"Missing {report}; run tools/fetch_vanilla.py first")
registries = json.loads(report.read_text(encoding="utf-8"))
# Registries whose members are the data-pack files under data/minecraft/<name>/.
DATA_DRIVEN = ("trim_material", "trim_pattern", "wolf_variant", "wolf_sound_variant", "jukebox_song",
               "banner_pattern", "decorated_pot_pattern", "painting_variant", "instrument", "cat_variant",
               "cat_sound_variant", "frog_variant", "chicken_variant", "chicken_sound_variant", "cow_variant",
               "cow_sound_variant", "pig_variant", "pig_sound_variant", "zombie_nautilus_variant",
               "sulfur_cube_archetype", "dialog")
keys = {}
ids = {}
for name in ("item", "block", "menu", "mob_effect", "sound_event", "attribute", "entity_type", "fluid", "game_event", "worldgen/structure_type",
             "villager_profession", "point_of_interest_type", "villager_type", "data_component_type", "particle_type", "potion"):
    entries = registries["minecraft:" + name]["entries"]
    keys[name] = set(entries)
    ids[name] = {key: entry["protocol_id"] for key, entry in entries.items()}
for name, directory in (("enchantment", "enchantment"), ("worldgen/biome", "worldgen/biome"), ("worldgen/structure", "worldgen/structure"), ("damage_type", "damage_type"),
                        *((name, name) for name in DATA_DRIVEN)):
    base = root / "data" / "minecraft" / directory
    keys[name] = {"minecraft:" + str(path.relative_to(base).with_suffix("")).replace("\\", "/") for path in base.rglob("*.json")}
    if not keys[name]:
        raise SystemExit(f"Missing generated entries in {base}")
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text("".join(f"{kind}\t{key}\n" for kind, values in keys.items() for key in sorted(values)), encoding="utf-8")
out.with_name("registry-ids.tsv").write_text(
    "".join(f"{kind}\t{key}\t{value}\n" for kind, values in ids.items() for key, value in sorted(values.items(), key=lambda item: item[1])),
    encoding="utf-8")
