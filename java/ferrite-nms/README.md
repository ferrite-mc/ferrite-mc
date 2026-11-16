# ferrite-nms — selected 26.3 NMS surface

Clean-room NMS/CraftBukkit compatibility wrappers for Ferrite's embedded plugin JVM. No Mojang
code or assets are copied; class names exist only so plugins compiled against CraftBukkit can
link, and every wrapper reads or writes Rust-owned state through a host port.

Target: Minecraft Java **26.3**, protocol **777**, data version **5023**, Paper API
**26.3.build.227-stable**, Java 25.

## Status: contract v1 inspected, state binding pending

Lane 1 froze `docs/plugin-b-host-contract.md` **v1** on 2026-11-16. It defines JNI, lifecycle,
events, the scheduler and the `HostApi` SPI (§12), but **no world, block, player or registry RPC
ops**. §12 says Lane 3 uses Lane 2 through `HostApi`. Accordingly,
`dev.ferrite.nms.host.NmsHost` is a **Lane 2/3 port**, not a claimed Lane 1 wire op: Lane 2's
facade implementation (or a future contract version) must bind it. No wrapper invents a Rust op
name or a fallback that pretends to succeed.

Until `NmsHostBinding.install(host)` is called, every wrapper that needs world/player/registry
state throws `IllegalStateException` naming the missing bridge. The only local fallbacks are
value objects: `net.minecraft.world.item.ItemStack` copies a Paper stack defensively.

Gradle ownership: Lane 1's `java/settings.gradle.kts` uses `FAIL_ON_PROJECT_REPOS` and currently
lists only Maven Central, so it must add `https://repo.papermc.io/repository/maven-public/`; it
includes `ferrite-nms` but not `fixtures:nms-plugin`, so the fixture include is still a Lane 1
request. Module build files therefore declare no repositories.

Lane 2 integration status: `FerriteWorld` and `FerritePlayer` are `final` and extend Lane 2's
`Unsupported*Base` classes, not `CraftWorld`/`CraftPlayer`. A plugin's
`(CraftWorld) world` / `(CraftPlayer) player` casts therefore fail until Lane 2 changes those base
classes or the lanes agree on an interface. This is a cross-lane blocker, not something this
module can silently paper over.

## Surface

| Class | Supported in the first slice | Explicitly refused |
|---|---|---|
| `org.bukkit.craftbukkit.inventory.CraftItemStack` | `asNMSCopy`, `asBukkitCopy`, `asCraftCopy` | `asCraftMirror` (needs a live server-side stack) |
| `org.bukkit.craftbukkit.CraftWorld#getHandle` | world key/UID/environment/seed/time/spawn/minY/maxY, block reads, chunk-loaded reads | synchronous block writes; use `setBlockAsync` |
| `org.bukkit.craftbukkit.util.CraftMagicNumbers` | `getMaterial`, `getKey`, `getBlock`, `getItem`, `getBlockState`, `getBlockStateId`, `getDataVersion`, `isLegacy` | `fromLegacy` (needs Lane 2 `UnsafeValues`) |
| `org.bukkit.craftbukkit.entity.CraftPlayer#getHandle` | UUID/name/world/position/rotation/game mode/health/food snapshots, async game-mode/health writes | direct live `ServerPlayer` mutation |
| `org.bukkit.craftbukkit.entity.CraftEntity#getHandle` | UUID/type/world/position/rotation/velocity/passengers/vehicle snapshots, async teleport | live entity field access |
| `net.minecraft.server.level.ServerLevel` | reads + `setBlockAsync` | `setBlock` (sync) |
| `net.minecraft.server.level.ServerPlayer` | snapshots + async writes | packet listener / connection objects |
| `net.minecraft.world.item.ItemStack` | material/count and a lossless Paper metadata copy | `CraftItemStack.asCraftMirror` |
| NMS menus, packet listeners, raw `Unsafe` memory | none | class/method absent or explicit `UnsupportedOperationException` |

`ServerLevel`, `ServerPlayer` and `ServerChunkCache` are **snapshot handles**. They never keep a
Rust pointer, `JNIEnv`, or a live tick-state object. Writes return a `CompletableFuture` completed
by the host on the owning region thread; Java callers never block a region tick.

## Version gate

`FerriteNmsVersion` pins `26.3` / `777` / `5023`. `NmsHostBinding.install` rejects a host whose
`HostVersion` differs, with `NmsVersionMismatchException`. `CraftMagicNumbers.getDataVersion()`
reports the attached host's data version after the same check. Multi-version support is a
non-goal.

## Build and test

The module is meant to be included by Lane 1's `java/settings.gradle.kts`:

```sh
cd java && ./gradlew :ferrite-nms:test :fixtures:nms-plugin:jar
```

That command is not runnable in this worktree yet: Lane 1's root settings/wrapper are in its own
worktree, its repository set lacks the Paper repository, and its include list has no
`fixtures:nms-plugin`. Local verification used a throwaway harness outside the repository that
mirrors Lane 1's repository mode and adds the Paper repository. See the Lane 3 handoff for the
exact commands and results.

Tests use `FakeNmsHost` to exercise snapshots, wrong world IDs, version mismatches, async write
routing and explicit refusals. Paper's `ItemStack` needs Lane 2's `RegistryAccess`/`ItemFactory`
even to construct, so metadata round-trip tests are blocked until the facade module exists; no
test asserts a fake success for that path.
