plugins {
    `java-library`
}

// Repositories are owned by Lane 1's java/settings.gradle.kts. Its current
// dependencyResolutionManagement block has FAIL_ON_PROJECT_REPOS and only mavenCentral, so Lane 1
// must add https://repo.papermc.io/repository/maven-public/ there before this module can resolve
// the Paper API. Do not add a project-level repositories block here.
dependencies {
    // The NMS surface is compiled against the Paper API types plugins already use. It is not
    // shaded into the jar: Lane 1/Lane 2 provide the API implementation on the host classpath.
    compileOnly("io.papermc.paper:paper-api:26.3.build.227-stable")

    testImplementation("io.papermc.paper:paper-api:26.3.build.227-stable")
    testImplementation("org.junit.jupiter:junit-jupiter:5.13.4")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
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

tasks.test {
    // The real-host integration test must run with all host, facade, and fixture jars built.
    dependsOn(":ferrite-host:jar", ":ferrite-api-impl:jar", ":fixtures:typical-plugin:jar", ":fixtures:nms-plugin:jar")
    useJUnitPlatform()
    testLogging {
        events("passed", "skipped", "failed")
        exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
    }
}

tasks.jar {
    archiveBaseName = "ferrite-nms"
}
