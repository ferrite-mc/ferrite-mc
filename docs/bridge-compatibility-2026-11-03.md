# Plugin bridge probe — 2026-11-03

Four plugin jars were tested separately with the embedded JVM and the local bridge built
from this branch. Each probe copied one fixture into a temporary plugin directory, started
the host, checked the plugin's enabled state, dispatched one console command, and shut it
down. The probe prints these results; a successful test process by itself does not establish
that the plugin enabled or its command succeeded.

| Plugin | Version | Enabled | Command accepted |
| --- | --- | --- | --- |
| Vault | 1.7.3-b131 | yes | `vault-info` |
| LuckPerms | 5.5.90 | yes | `luckperms` |
| EssentialsX | 2.22.0 | yes | `help` |
| PlaceholderAPI | 2.12.3 | yes | `placeholderapi version` |

The tested jars' SHA-256 values are:

- Vault: `a6b5ed97f43a5cf5bbaf00a7c8cd23c5afc9bd003f849875af8b36e6cf77d01d`
- LuckPerms: `65a99437c47d84434d1843c8dbfcbd36d09f389356473278a2c3870e63a5afdb`
- EssentialsX: `bda4685105977fca2e209820a9f0ad24275bd103390a03236f38e59bfdac58e6`
- PlaceholderAPI: `fde03259f5af6938f3c33eeb4d814000a1adabf1d2304ce14970be81f609a437`

The EssentialsX probe previously logged a null permissions-handler exception on shutdown.
After emitting plugin lifecycle events and clearing disabled plugins' event handlers, the
probe enabled EssentialsX, reported `help: Ok(true)`, and did not log that exception.
EssentialsX still warns that `net.minecraft.server.MinecraftServer` is absent. The four
isolated probes do not establish compatibility of every API call, player-facing feature,
plugin combination, persistence operation, or NMS-dependent feature.

Run an individual probe from the repository root after building the bridge with
`java/ferrite-api-impl/gradlew`'s `shadedJar` task:

```powershell
$env:FERRITE_COMPAT_PROBE = 'C:/path/to/Plugin.jar'
cargo test -p ferrite-plugin --test compat -- --exact compat_probe --nocapture
```

Inspect `FERRITE_RESULT`, `FERRITE_COMMAND`, and the full shutdown output; the current
probe logs errors rather than treating all plugin failures as test failures.
