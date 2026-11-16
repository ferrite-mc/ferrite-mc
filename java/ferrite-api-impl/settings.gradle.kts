// Standalone build for Lane 2 until Lane 1 freezes `java/settings.gradle.kts`.
//
// Lane 2 may only touch `java/ferrite-api-impl/**` and `java/fixtures/typical-plugin/**`,
// so this file makes the module buildable on its own. When the top-level Java build
// exists, delete this file and let `java/settings.gradle.kts` own the include:
//
//   include(":ferrite-api-impl")
//   include(":fixtures:typical-plugin")
//
// The module paths below deliberately match the future top-level paths so the build
// scripts do not change when the include moves.
rootProject.name = "ferrite-lane-2"

include(":fixtures")
project(":fixtures").projectDir = file("../fixtures")
include(":fixtures:typical-plugin")
project(":fixtures:typical-plugin").projectDir = file("../fixtures/typical-plugin")
