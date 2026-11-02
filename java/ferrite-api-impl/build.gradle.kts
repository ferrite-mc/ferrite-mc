plugins { java }
repositories { mavenCentral(); maven("https://repo.papermc.io/repository/maven-public/") }
dependencies { implementation("io.papermc.paper:paper-api:26.3.build.227-stable"); testImplementation("org.junit.jupiter:junit-jupiter:5.13.4"); testRuntimeOnly("org.junit.platform:junit-platform-launcher") }
java { toolchain { languageVersion = JavaLanguageVersion.of(25) } }
tasks.test { useJUnitPlatform() }
tasks.test { systemProperty("test.vanilla.data", rootProject.projectDir.resolve("../../vanilla/26.3/generated").absolutePath) }
tasks.test { dependsOn(":test-plugin:jar"); systemProperty("test.plugin.jar", project(":test-plugin").layout.buildDirectory.file("libs/test-plugin.jar").get().asFile.absolutePath) }
tasks.test { inputs.file(project(":test-plugin").layout.buildDirectory.file("libs/test-plugin.jar")) }
tasks.register<Jar>("shadedJar") {
    archiveClassifier = "all"
    duplicatesStrategy = DuplicatesStrategy.EXCLUDE
    from(sourceSets.main.get().output)
    from({ configurations.runtimeClasspath.get().map { if (it.isDirectory) it else zipTree(it) } }) { exclude("META-INF/*.SF", "META-INF/*.RSA", "META-INF/*.DSA") }
}
tasks.build { dependsOn("shadedJar") }
tasks.test { dependsOn(":test-dependency:jar"); inputs.file(project(":test-dependency").layout.buildDirectory.file("libs/test-dependency.jar")); systemProperty("test.dependency.jar", project(":test-dependency").layout.buildDirectory.file("libs/test-dependency.jar").get().asFile.absolutePath) }
tasks.test { dependsOn(":test-plugin:legacyJar"); inputs.file(project(":test-plugin").layout.buildDirectory.file("libs/test-plugin-legacy.jar")); systemProperty("test.legacy.jar", project(":test-plugin").layout.buildDirectory.file("libs/test-plugin-legacy.jar").get().asFile.absolutePath) }

val registryOutput = layout.buildDirectory.dir("generated/registry-resources")
val generateRegistryKeys by tasks.registering(Exec::class) {
    val vanilla = rootProject.projectDir.resolve("../../vanilla/26.3/generated")
    inputs.file(vanilla.resolve("reports/registries.json"))
    inputs.dir(vanilla.resolve("data/minecraft/enchantment"))
    inputs.dir(vanilla.resolve("data/minecraft/worldgen/biome"))
    inputs.dir(vanilla.resolve("data/minecraft/damage_type"))
    outputs.dir(registryOutput)
    commandLine("python", projectDir.resolve("tools/generate_registry_keys.py"), vanilla, registryOutput.get().file("ferrite/registry-keys.tsv").asFile)
}
sourceSets.main { resources.srcDir(registryOutput) }
tasks.processResources { dependsOn(generateRegistryKeys) }
