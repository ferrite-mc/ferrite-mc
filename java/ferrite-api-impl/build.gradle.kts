plugins { java }
repositories { mavenCentral(); maven("https://repo.papermc.io/repository/maven-public/") }
dependencies { implementation("io.papermc.paper:paper-api:26.3.build.227-stable")
    // JDBC drivers Paper ships on the server classpath; plugins expect them without shading.
    implementation("org.xerial:sqlite-jdbc:3.50.3.0"); implementation("com.mysql:mysql-connector-j:9.4.0")
    // Resolves `libraries:` from plugin.yml, as Spigot's LibraryLoader does.
    implementation("org.apache.maven.resolver:maven-resolver-supplier:1.9.24"); implementation("org.slf4j:slf4j-jdk14:2.0.17")
    // Bukkit (SimpleCommandMap) and plugins use commons-lang3 from the server classpath.
    implementation("org.apache.commons:commons-lang3:3.12.0")
    testImplementation("org.junit.jupiter:junit-jupiter:5.13.4"); testRuntimeOnly("org.junit.platform:junit-platform-launcher") }
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
    inputs.file(projectDir.resolve("tools/generate_registry_keys.py"))
    inputs.file(vanilla.resolve("reports/registries.json"))
    inputs.file(vanilla.resolve("reports/blocks.json"))
    inputs.dir(vanilla.resolve("data/minecraft/enchantment"))
    inputs.dir(vanilla.resolve("data/minecraft/worldgen/biome"))
    inputs.dir(vanilla.resolve("data/minecraft/damage_type"))
    for (name in listOf("trim_material", "trim_pattern", "wolf_variant", "wolf_sound_variant", "jukebox_song",
            "banner_pattern", "decorated_pot_pattern", "painting_variant", "instrument", "cat_variant",
            "cat_sound_variant", "frog_variant", "chicken_variant", "chicken_sound_variant", "cow_variant",
            "cow_sound_variant", "pig_variant", "pig_sound_variant", "zombie_nautilus_variant",
            "sulfur_cube_archetype", "dialog")) inputs.dir(vanilla.resolve("data/minecraft/$name"))
    outputs.dir(registryOutput)
    commandLine("python", projectDir.resolve("tools/generate_registry_keys.py"), vanilla, registryOutput.get().file("ferrite/registry-keys.tsv").asFile)
}
sourceSets.main { resources.srcDir(registryOutput) }
tasks.processResources { dependsOn(generateRegistryKeys) }

// Game-rule defaults are not in reports/registries.json: query the locally fetched official jar.
val gameRuleOutput = layout.buildDirectory.dir("generated/gamerule-resources")
val generateGameRules = tasks.register<Exec>("generateGameRules") {
    val versionDir = rootProject.projectDir.resolve("../../vanilla/26.3")
    inputs.file(rootProject.projectDir.resolve("../../tools/extract_gamerules.py"))
    inputs.file(rootProject.projectDir.resolve("../../tools/extractor/GameRuleExtractor.java"))
    inputs.file(versionDir.resolve("lib/server-26.3.jar"))
    inputs.file(versionDir.resolve("generated/reports/registries.json"))
    outputs.dir(gameRuleOutput)
    commandLine("python", rootProject.projectDir.resolve("../../tools/extract_gamerules.py"), versionDir,
        gameRuleOutput.get().file("ferrite/game-rules.json").asFile, "--verify")
}
sourceSets.main { resources.srcDir(gameRuleOutput); resources.exclude("**/gamerule-extractor-classes/**", "**/game-rules-reference.json") }
tasks.processResources { dependsOn(generateGameRules) }
tasks.test {
    dependsOn(generateGameRules)
    systemProperty("test.gamerule.reference", gameRuleOutput.get().file("ferrite/game-rules-reference.json").asFile.absolutePath)
}
