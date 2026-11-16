pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
        maven {
            name = "PaperMC"
            url = uri("https://repo.papermc.io/repository/maven-public/")
        }
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        mavenCentral()
        maven {
            name = "PaperMC"
            url = uri("https://repo.papermc.io/repository/maven-public/")
        }
    }
}

rootProject.name = "ferrite-java"

// Lane 1
include("ferrite-host")

// Lane 2 and Lane 3 fill these modules; Lane 1 only owns the include list and empty scaffold.
include("ferrite-api-impl")
include("ferrite-nms")

// Lane 2/3 fixture modules are optional. A Lane 1-only checkout has no `fixtures/` directory,
// and including a module without a build file would fail configuration, so include each one
// only when its directory exists.
val fixturesDir = rootDir.resolve("fixtures")
if (fixturesDir.resolve("typical-plugin").isDirectory) {
    include(":fixtures:typical-plugin")
}
if (fixturesDir.resolve("nms-plugin").isDirectory) {
    include(":fixtures:nms-plugin")
}
