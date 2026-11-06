#!/usr/bin/env bash
# Compile and run the block/item property extractor against a fetched vanilla version.
#
#   tools/extractor/build.sh [version]
#
# Requires vanilla/<version>/lib (extracted from server.jar) and a JDK 25 on JAVA_HOME or PATH.
# Normally run for you by `python tools/fetch_vanilla.py`.
set -euo pipefail

version="${1:-26.3}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
lib="$root/vanilla/$version/lib"
server="$lib/server-$version.jar"

if [[ ! -f "$server" ]]; then
  echo "missing $server; run 'python tools/fetch_vanilla.py $version' first" >&2
  exit 1
fi

bin="${JAVA_HOME:+$JAVA_HOME/bin/}"
javac="${bin}javac"
java="${bin}java"

case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) sep=';' ;;
  *) sep=':' ;;
esac

# Windows Java does not accept MSYS paths, so hand it native ones where available.
to_native() {
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$1"
  else
    printf '%s' "$1"
  fi
}

classpath="$(to_native "$server")"
while IFS= read -r jar; do
  classpath="$classpath$sep$(to_native "$jar")"
done < <(find "$lib/libraries" -name '*.jar' | sort)

classes="$lib/extractor-classes"
mkdir -p "$classes"
"$javac" -d "$(to_native "$classes")" -cp "$classpath" \
  "$(to_native "$root/tools/extractor/Extractor.java")" \
  "$(to_native "$root/tools/extractor/PistonExtractor.java")" \
  "$(to_native "$root/tools/extractor/AdvancementExtractor.java")" \
  "$(to_native "$root/tools/extractor/RecipeExtractor.java")"
"$java" -cp "$(to_native "$classes")$sep$classpath" Extractor \
  "$(to_native "$root/vanilla/$version/generated/ferrite")" \
  "$(to_native "$root/vanilla/$version/generated/reports/minecraft/components/item")"
"$java" -cp "$(to_native "$classes")$sep$classpath" PistonExtractor \
  "$(to_native "$root/vanilla/$version/generated/ferrite/piston_states.json")"
"$java" -cp "$(to_native "$classes")$sep$classpath" AdvancementExtractor \
  "$(to_native "$root/vanilla/$version/generated/data")" \
  "$(to_native "$root/vanilla/$version/generated/ferrite/advancements.json")"
"$java" -cp "$(to_native "$classes")$sep$classpath" RecipeExtractor \
  "$(to_native "$root/vanilla/$version/generated/ferrite/recipes.json")"
