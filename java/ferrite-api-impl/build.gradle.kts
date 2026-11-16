import java.io.File

plugins {
    `java-library`
}

// The root build must see the fixture's `jar` task while configuring `test`.
evaluationDependsOn(":fixtures:typical-plugin")

group = "dev.ferrite"
version = "0.1.0-SNAPSHOT"

// Repositories are owned by the root java/settings.gradle.kts; this build uses
// its dependencyResolutionManagement repositories under FAIL_ON_PROJECT_REPOS.

dependencies {
    // The host classpath provides the Paper API; the facade must never bundle it.
    // It is only used to compile against the real 26.3 signatures.
    compileOnly("io.papermc.paper:paper-api:26.3.build.227-stable")
    // The generated vanilla reports are JSON; paper-api already brings gson at runtime.
    compileOnly("com.google.code.gson:gson:2.14.0")

    // Tests need the API at runtime (event classes, HandlerList, PluginCommand, YAML).
    testImplementation("io.papermc.paper:paper-api:26.3.build.227-stable")
    testImplementation("com.google.code.gson:gson:2.14.0")
    testImplementation(platform("org.junit:junit-bom:5.11.4"))
    testImplementation("org.junit.jupiter:junit-jupiter")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
}

java {
    toolchain {
        languageVersion = JavaLanguageVersion.of(25)
    }
}

tasks.withType<JavaCompile>().configureEach {
    options.encoding = "UTF-8"
    options.compilerArgs.addAll(listOf("-Xlint:all,-serial,-processing", "-parameters"))
}

val fixtureJar = project(":fixtures:typical-plugin").tasks.named<Jar>("jar").flatMap { it.archiveFile }

tasks.test {
    useJUnitPlatform()
    dependsOn(project(":fixtures:typical-plugin").tasks.named("jar"))

    // The fixture jar must not leak onto the test classpath: the tests load it with
    // their own URLClassLoader, the way the host will load plugin jars.
    doFirst {
        systemProperty("ferrite.fixture.typical.jar", fixtureJar.get().asFile.absolutePath)
    }

    testLogging {
        events("failed", "skipped")
        exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
    }
}

// The API-base generator reads the real 26.3 signatures with javap. Gradle resolves the
// full Paper API classpath (including adventure/bungee-chat) into this file so the
// generator never has to guess a dependency path.
val apiClasspathFile = layout.buildDirectory.file("paper-api-classpath.txt")
val printApiClasspath by tasks.registering {
    val classpath = configurations.testRuntimeClasspath
    outputs.file(apiClasspathFile)
    doLast {
        apiClasspathFile.get().asFile.writeText(
            classpath.get().joinToString(File.pathSeparator) { it.absolutePath },
        )
    }
}

// Lane 1 loads this artifact on the host classpath. No relocation is needed: the only
// runtime dependency is the Paper API, which the host already provides.
val shadedJar by tasks.registering(Jar::class) {
    archiveClassifier.set("shaded")
    from(sourceSets.main.get().output)
}
