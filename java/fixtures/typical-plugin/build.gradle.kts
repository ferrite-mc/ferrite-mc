plugins {
    java
}

group = "dev.ferrite.fixtures"
version = "1.0.0"

// Repositories are owned by the root java/settings.gradle.kts.

dependencies {
    // The host classpath provides the API; the fixture jar must stay API-only so it
    // exercises the facade the same way a real plugin would.
    compileOnly("io.papermc.paper:paper-api:26.3.build.227-stable")
}

java {
    toolchain {
        languageVersion = JavaLanguageVersion.of(25)
    }
}

tasks.jar {
    archiveFileName.set("typical-plugin-1.0.0.jar")
    manifest {
        attributes(
            "Implementation-Title" to "Ferrite typical fixture plugin",
            "Implementation-Version" to project.version,
        )
    }
}

// Descriptor variant: the same classes packaged with `paper-plugin.yml` instead of
// `plugin.yml`. The host's plugin-meta scanner prefers paper-plugin.yml, so this artifact
// documents and tests the Paper-plugin manifest shape without changing the main fixture.
val paperJar by tasks.registering(Jar::class) {
    archiveFileName.set("typical-plugin-1.0.0-paper.jar")
    from(sourceSets.main.get().output) {
        exclude("plugin.yml")
    }
    from("src/paper/resources")
    manifest {
        attributes(
            "Implementation-Title" to "Ferrite typical fixture plugin (paper descriptor)",
            "Implementation-Version" to project.version,
        )
    }
}

tasks.withType<JavaCompile>().configureEach {
    options.encoding = "UTF-8"
    options.compilerArgs.addAll(listOf("-Xlint:all,-serial,-processing", "-parameters"))
}
