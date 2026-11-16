plugins {
    java
}

// Repositories belong to Lane 1's java/settings.gradle.kts (FAIL_ON_PROJECT_REPOS), which must
// also gain the Paper Maven repository. See java/ferrite-nms/build.gradle.kts.
dependencies {
    // The host supplies these classes at runtime; the fixture jar only carries its own plugin.
    compileOnly(project(":ferrite-nms"))
    compileOnly("io.papermc.paper:paper-api:26.3.build.227-stable")
}

java {
    toolchain {
        languageVersion = JavaLanguageVersion.of(25)
    }
}

tasks.withType<JavaCompile>().configureEach {
    options.encoding = "UTF-8"
    options.release.set(25)
}

tasks.jar {
    archiveBaseName = "nms-plugin"
    archiveVersion = ""
    archiveClassifier = ""
}
