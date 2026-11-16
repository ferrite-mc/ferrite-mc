#!/usr/bin/env python3
"""Generate explicit UnsupportedOperationException bases for the Paper API.

Lane 2 implements a vertical slice of the Bukkit API on top of the real
`io.papermc.paper:paper-api:26.3.build.227-stable` interfaces. Anything outside the
slice must fail loudly instead of returning null/empty values that look successful.

The API interfaces are far too large to hand-write (Server has ~200 abstract methods,
Player ~225, ItemMeta ~116), so this script reads the real signatures with `javap` and
emits:

  * `Unsupported<Api>Base` classes under `dev.ferrite.api.*`. Hand-written facade classes
    extend them and override only the supported methods; inherited abstract methods throw
    `UnsupportedOperationException` with the API method name and the calling plugin.
  * The intentionally-unsupported part of `org.bukkit.inventory.ItemStack`.

Why ItemStack is generated as a concrete class: Paper's API `ItemStack` delegates every
accessor to a `craftDelegate` that only a CraftBukkit server installs, so it cannot be
constructed or read on Ferrite. The facade therefore ships its own class with the API's
binary name and implements the slice (material/amount/display name/lore/PDC) itself.

Usage, from `java/ferrite-api-impl`:

    ./gradlew -q printApiClasspath
    python3 tools/generate_unsupported.py

The script reads `build/paper-api-classpath.txt` by default; `--classpath` or the
`PAPER_API_CLASSPATH` environment variable override it. Generated files are committed so
the build does not depend on this script.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from collections import deque
from pathlib import Path

PRIMITIVES = {
    "V": "void",
    "Z": "boolean",
    "B": "byte",
    "C": "char",
    "S": "short",
    "I": "int",
    "J": "long",
    "F": "float",
    "D": "double",
}

# (API type, generated base class, message prefix)
BASES = [
    ("org.bukkit.Server", "dev.ferrite.api.server.UnsupportedServerBase", "Server"),
    ("org.bukkit.World", "dev.ferrite.api.world.UnsupportedWorldBase", "World"),
    ("org.bukkit.entity.Player", "dev.ferrite.api.entity.UnsupportedPlayerBase", "Player"),
    ("org.bukkit.block.Block", "dev.ferrite.api.block.UnsupportedBlockBase", "Block"),
    (
        "org.bukkit.inventory.PlayerInventory",
        "dev.ferrite.api.inventory.UnsupportedPlayerInventoryBase",
        "PlayerInventory",
    ),
    (
        "org.bukkit.inventory.meta.ItemMeta",
        "dev.ferrite.api.item.UnsupportedItemMetaBase",
        "ItemMeta",
    ),
    (
        "org.bukkit.persistence.PersistentDataContainer",
        "dev.ferrite.api.item.UnsupportedPersistentDataContainerBase",
        "PersistentDataContainer",
    ),
    (
        "org.bukkit.scheduler.BukkitScheduler",
        "dev.ferrite.api.scheduler.UnsupportedBukkitSchedulerBase",
        "BukkitScheduler",
    ),
    (
        "org.bukkit.plugin.PluginManager",
        "dev.ferrite.api.plugin.UnsupportedPluginManagerBase",
        "PluginManager",
    ),
    (
        "org.bukkit.command.ConsoleCommandSender",
        "dev.ferrite.api.command.UnsupportedConsoleCommandSenderBase",
        "ConsoleCommandSender",
    ),
]

ITEMSTACK_TYPE = "org.bukkit.inventory.ItemStack"
ITEMSTACK_PATH = "src/main/java/org/bukkit/inventory/ItemStack.java"
ITEMSTACK_TEMPLATE = "tools/templates/ItemStack.java.in"
ITEMSTACK_MARKER = "// @@GENERATED_UNSUPPORTED@@"

# Methods the ItemStack template implements by hand; everything else is generated as an
# explicit refusal. Keep this list in sync with the template.
SUPPORTED_ITEMSTACK = {
    "getType",
    "setType",
    "withType",
    "getAmount",
    "setAmount",
    "getMaxStackSize",
    "getItemMeta",
    "setItemMeta",
    "hasItemMeta",
    "editMeta",
    "getLore",
    "setLore",
    "lore",
    "displayName",
    "getTranslationKey",
    "getPersistentDataContainer",
    "editPersistentDataContainer",
    "toString",
    "equals",
    "hashCode",
    "isSimilar",
    "clone",
    "serialize",
    "asOne",
    "asQuantity",
    "add",
    "subtract",
}


class Method:
    __slots__ = ("name", "params", "ret", "is_static", "is_default", "depth", "decl")

    def __init__(self, name, params, ret, is_static, is_default, decl, depth=0):
        self.name = name
        self.params = params
        self.ret = ret
        self.is_static = is_static
        self.is_default = is_default
        self.decl = decl
        self.depth = depth

    def signature(self):
        return self.name + "(" + ",".join(self.params) + ")"


class Javap:
    def __init__(self, javap: str, classpath: str):
        self.javap = javap
        self.classpath = classpath
        self._cache: dict[str, tuple[list[Method], list[str]]] = {}

    def describe(self, cls: str) -> tuple[list[Method], list[str]]:
        if cls in self._cache:
            return self._cache[cls]
        proc = subprocess.run(
            [self.javap, "-s", "-p", "-classpath", self.classpath, cls],
            capture_output=True,
            text=True,
        )
        if proc.returncode != 0:
            raise RuntimeError(f"javap failed for {cls}: {proc.stderr.strip()}")
        methods, supers = _parse_description(cls, proc.stdout)
        self._cache[cls] = (methods, supers)
        return methods, supers


def _parse_description(cls: str, text: str) -> tuple[list[Method], list[str]]:
    lines = text.splitlines()
    head = None
    for line in lines:
        stripped = line.strip()
        if re.match(
            r"^(?:(?:public|protected|private|abstract|final|sealed|non-sealed)\s+)*(?:interface|class)\s+",
            stripped,
        ):
            head = stripped
            break

    supers = _parse_supers(head) if head else []

    methods: list[Method] = []
    for index, line in enumerate(lines):
        decl = line.strip()
        if not line.startswith("  ") or line.startswith("   "):
            continue
        if not decl.endswith(";") or "(" not in decl:
            continue
        if not decl.startswith(("public ", "protected ")):
            continue
        if index + 1 >= len(lines):
            continue
        next_line = lines[index + 1].strip()
        if not next_line.startswith("descriptor:"):
            continue
        descriptor = next_line[len("descriptor:") :].strip()
        name = _method_name(decl)
        if not name or name.startswith("<"):
            continue
        # javap prints constructors with the fully qualified class name as the member name.
        if name == cls or name.rsplit(".", 1)[-1] == _simple_name(cls):
            continue
        params_desc, ret_desc = _split_descriptor(descriptor)
        methods.append(
            Method(
                name=name,
                params=[_desc_to_type(p) for p in params_desc],
                ret=_desc_to_type(ret_desc),
                is_static=bool(re.search(r"\bstatic\b", decl)),
                is_default=bool(re.search(r"\bdefault\b", decl)),
                decl=decl,
            )
        )
    return methods, supers


def _simple_name(cls: str) -> str:
    return cls.rsplit(".", 1)[-1].replace("$", ".")


def _method_name(decl: str) -> str | None:
    before = decl.split("(", 1)[0].strip()
    token = before.rsplit(" ", 1)[-1]
    # Drop a generic method prefix such as `public <T> T get(...)`'s return type.
    if "<" in token:
        return None
    return token


def _parse_supers(head: str) -> list[str]:
    head = head.rstrip("{").strip()
    match = re.match(
        r"^(?:(?:public|protected|private|abstract|final|sealed|non-sealed)\s+)*"
        r"(?:interface|class)\s+([^\s<{]+)",
        head,
    )
    if not match:
        return []
    tail = head[match.end() :].strip()
    if tail.startswith("<"):
        depth = 0
        for index, char in enumerate(tail):
            if char == "<":
                depth += 1
            elif char == ">":
                depth -= 1
                if depth == 0:
                    tail = tail[index + 1 :]
                    break
    parts: list[str] = []
    for segment in re.split(r"\b(?:extends|implements)\b", tail):
        segment = segment.strip().rstrip("{").strip()
        if segment:
            parts.extend(_split_top_level(segment))
    return [_strip_generics(part).strip() for part in parts if _strip_generics(part).strip()]


def _split_top_level(text: str) -> list[str]:
    parts: list[str] = []
    depth = 0
    current: list[str] = []
    for char in text:
        if char == "<":
            depth += 1
        elif char == ">":
            depth -= 1
        if char == "," and depth == 0:
            parts.append("".join(current))
            current = []
        else:
            current.append(char)
    if current:
        parts.append("".join(current))
    return parts


def _strip_generics(text: str) -> str:
    out: list[str] = []
    depth = 0
    for char in text:
        if char == "<":
            depth += 1
        elif char == ">":
            depth -= 1
        elif depth == 0:
            out.append(char)
    return "".join(out)


def _split_descriptor(descriptor: str) -> tuple[list[str], str]:
    if not descriptor.startswith("("):
        raise ValueError(f"not a method descriptor: {descriptor}")
    params: list[str] = []
    index = 1
    while descriptor[index] != ")":
        length, index = _field_descriptor_length(descriptor, index)
        params.append(descriptor[index - length : index])
    index += 1
    length, end = _field_descriptor_length(descriptor, index)
    return params, descriptor[index:end]


def _field_descriptor_length(descriptor: str, start: int) -> tuple[int, int]:
    index = start
    while descriptor[index] == "[":
        index += 1
    if descriptor[index] == "L":
        end = descriptor.index(";", index) + 1
    else:
        end = index + 1
    return end - start, end


def _desc_to_type(descriptor: str) -> str:
    dims = 0
    index = 0
    while descriptor[index] == "[":
        dims += 1
        index += 1
    if descriptor[index] == "L":
        name = descriptor[index + 1 : -1].replace("/", ".").replace("$", ".")
    else:
        name = PRIMITIVES[descriptor[index]]
    return name + "[]" * dims


def _collect(javap: Javap, root: str) -> list[Method]:
    groups: dict[tuple[str, tuple[str, ...]], list[tuple[int, Method]]] = {}
    queue: deque[tuple[str, int]] = deque([(root, 0)])
    seen: set[str] = set()

    while queue:
        cls, depth = queue.popleft()
        if cls in seen:
            continue
        seen.add(cls)
        if cls.startswith("java.") and cls not in ("java.lang.Comparable", "java.lang.Iterable"):
            continue
        try:
            methods, supers = javap.describe(cls)
        except RuntimeError as error:
            print(f"warning: {error}", file=sys.stderr)
            continue
        for method in methods:
            if method.is_static:
                continue
            key = (method.name, tuple(method.params))
            groups.setdefault(key, []).append((depth, method))
        for super_name in supers:
            queue.append((super_name, depth + 1))

    result: list[Method] = []
    for group in groups.values():
        group.sort(key=lambda item: item[0])
        # The closest declaration is what Java resolves; if it is a default method,
        # the API's default implementation must keep working.
        if group[0][1].is_default:
            continue
        chosen = group[0][1]
        chosen.depth = group[0][0]
        for depth, candidate in group[1:]:
            if candidate.ret != chosen.ret and _is_subtype(javap, candidate.ret, chosen.ret):
                chosen = candidate
                chosen.depth = depth
        result.append(chosen)
    return result


def _is_subtype(javap: Javap, candidate: str, parent: str, seen: set[str] | None = None) -> bool:
    """Whether the API type `candidate` is assignable to `parent` (erased names)."""
    if candidate == parent:
        return True
    if candidate.endswith("[]") or parent.endswith("[]"):
        return False
    if candidate in PRIMITIVES.values() or parent in PRIMITIVES.values():
        return False
    if seen is None:
        seen = set()
    if candidate in seen:
        return False
    seen.add(candidate)
    try:
        _, supers = javap.describe(candidate)
    except RuntimeError:
        return False
    return any(_is_subtype(javap, super_name, parent, seen) for super_name in supers)


def _signature(method: Method) -> str:
    """The full generic signature (type params, return type, parameters, throws)."""
    decl = method.decl.rstrip(";").strip().replace("$", ".")
    decl = re.sub(
        r"^(?:(?:public|protected|private|abstract|final|static|default|synchronized|native|strictfp)\s+)+",
        "",
        decl,
    )
    open_index = decl.index("(")
    close_index = decl.rindex(")")
    inner = decl[open_index + 1 : close_index].strip()
    params = _split_top_level(inner) if inner else []
    named = ", ".join(f"{param.strip()} p{index}" for index, param in enumerate(params))
    return decl[: open_index + 1] + named + decl[close_index:]


def _emit_method(method: Method, prefix: str, indent: str, with_override: bool = True) -> str:
    lines = []
    if with_override:
        lines.append(f"{indent}@Override")
    lines.extend([
        f"{indent}public {_signature(method)} {{",
        f'{indent}    throw unsupported("{method.name}");',
        f"{indent}}}",
    ])
    return "\n".join(lines) + "\n"


def _write_bases(javap: Javap, repo_root: Path) -> None:
    for api_type, generated_name, prefix in BASES:
        methods = _collect(javap, api_type)
        package = generated_name.rsplit(".", 1)[0]
        simple = generated_name.rsplit(".", 1)[1]
        body = "\n".join(_emit_method(method, prefix, "    ") for method in methods)
        source = f"""// GENERATED by tools/generate_unsupported.py from paper-api 26.3.build.227-stable.
// Do not hand-edit: every method below is intentionally unsupported and fails loudly.
package {package};

/**
 * Explicit refusal base for {{@link {api_type}}}.
 *
 * <p>Lane 2 supports a vertical slice only. Facade classes extend this base and override
 * the supported methods; everything else throws {{@link UnsupportedOperationException}}
 * naming the API method and the plugin that called it.
 */
@SuppressWarnings({{"unchecked", "rawtypes"}})
public abstract class {simple} implements {api_type} {{
    /** Build the facade's explicit unsupported-API error. */
    protected static RuntimeException unsupported(String method) {{
        return dev.ferrite.api.internal.Unsupported.exception("{prefix}#" + method);
    }}

{body}}}
"""
        target = repo_root / "src/main/java" / package.replace(".", "/") / f"{simple}.java"
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(source, encoding="utf-8")
        print(f"wrote {target.relative_to(repo_root)} ({len(methods)} methods)")


def _write_itemstack(javap: Javap, repo_root: Path) -> None:
    methods = _collect(javap, ITEMSTACK_TYPE)
    stubs = [
        _emit_method(method, "ItemStack", "    ", with_override=False)
        for method in methods
        if method.name not in SUPPORTED_ITEMSTACK
    ]
    template = (repo_root / ITEMSTACK_TEMPLATE).read_text(encoding="utf-8")
    if ITEMSTACK_MARKER not in template:
        raise SystemExit(f"missing marker {ITEMSTACK_MARKER} in {ITEMSTACK_TEMPLATE}")
    source = template.replace(ITEMSTACK_MARKER, "\n".join(stubs).rstrip())
    target = repo_root / ITEMSTACK_PATH
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(source, encoding="utf-8")
    print(f"wrote {target.relative_to(repo_root)} ({len(stubs)} unsupported methods)")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--classpath", help="paper-api + transitive dependencies classpath")
    parser.add_argument("--javap", default=None, help="javap executable")
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parent.parent
    classpath = (
        args.classpath
        or os.environ.get("PAPER_API_CLASSPATH")
        or (repo_root / "build/paper-api-classpath.txt").read_text(encoding="utf-8").strip()
    )
    if not classpath:
        raise SystemExit("empty Paper API classpath")

    javap = args.javap or os.environ.get("JAVAP") or _find_javap()
    generator = Javap(javap, classpath)
    _write_bases(generator, repo_root)
    _write_itemstack(generator, repo_root)
    return 0


def _find_javap() -> str:
    import shutil

    found = shutil.which("javap")
    if found:
        return found
    java_home = os.environ.get("JAVA_HOME")
    if java_home:
        candidate = Path(java_home) / "bin" / "javap"
        if candidate.exists():
            return str(candidate)
    raise SystemExit("javap not found; set JAVA_HOME or --javap")


if __name__ == "__main__":
    raise SystemExit(main())
