// Clean-room Ferrite NMS integration skeleton. No Mojang code or assets.
package dev.ferrite.nms.integration;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.io.InputStream;
import java.lang.reflect.Method;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Map;
import java.util.jar.JarEntry;
import java.util.jar.JarFile;
import java.util.stream.Stream;
import org.junit.jupiter.api.Assumptions;
import org.junit.jupiter.api.Test;

/**
 * Skeleton for the real embedded-host integration: the typical fixture (Lane 2) and the NMS
 * fixture (Lane 3) must load into the same host JVM and both report {@code ok}.
 *
 * <p>The test is self-arming. It never fakes a pass: until Lane 1 publishes its Gradle root and
 * host jar and Lane 2 publishes the typical fixture, it aborts with the missing prerequisites.
 * Those prerequisites are:
 *
 * <ol>
 *   <li>{@code java/settings.gradle.kts} includes {@code :ferrite-nms},
 *       {@code :fixtures:nms-plugin} and {@code :fixtures:typical-plugin}, and adds
 *       {@code https://repo.papermc.io/repository/maven-public/} because Paper API is not on
 *       Maven Central.
 *   <li>{@code :ferrite-host} produces {@code build/libs/ferrite-host*.jar} and installs
 *       Lane 2's {@code NmsHandleProvider} with {@code NmsBridge.install(provider)} before the
 *       POSTWORLD fixtures enable. Without that provider the NMS fixture must fail with
 *       {@code provider_not_installed}; this test then fails rather than skips.
 *   <li>Lane 2 produces {@code fixtures/typical-plugin/build/libs/typical-plugin*.jar} and
 *       implements the provider in {@code ferrite-api-impl}.
 *   <li>Lane 1 exposes the test seam below. Its exact shape may change; only the two reflective
 *       names in this file need updating when it does.
 * </ol>
 *
 * <p>Expected seam (proposed in the Lane 3 handoff):
 *
 * <pre>{@code
 * package dev.ferrite.host.integration;
 * public final class HostIntegrationHarness implements AutoCloseable {
 *     public static HostIntegrationHarness start(Path pluginsDir, Path dataDir);
 *     public Map<String, String> reports(); // plugin name -> "ok" or an error report
 *     public void close();
 * }
 * }</pre>
 *
 * <p>This test does not start a second JVM or a second host. The harness owns the one embedded
 * JVM, loads both jars into it, and reports per-plugin results. The assertion is deliberately
 * about both fixtures sharing that one host, not about two independent classloaders.
 */
class RealHostIntegrationTest {
    private static final String HARNESS_CLASS = "dev.ferrite.host.integration.HostIntegrationHarness";
    private static final String NMS_MAIN = "dev.ferrite.fixtures.nms.NmsFixturePlugin";

    @Test
    void typicalAndNmsFixturesLoadInTheSameHostJvm() throws Exception {
        Path javaRoot = javaRoot();
        Path hostJar = findJar(javaRoot.resolve("ferrite-host/build/libs"), "ferrite-host");
        Path typicalJar = findJar(javaRoot.resolve("fixtures/typical-plugin/build/libs"), "typical-plugin");
        Path nmsJar = findJar(javaRoot.resolve("fixtures/nms-plugin/build/libs"), "nms-plugin");

        List<String> missing = new ArrayList<>();
        if (hostJar == null) {
            missing.add("java/ferrite-host/build/libs/ferrite-host*.jar (Lane 1)");
        }
        if (typicalJar == null) {
            missing.add("java/fixtures/typical-plugin/build/libs/typical-plugin*.jar (Lane 2)");
        }
        Assumptions.assumeTrue(
                missing.isEmpty(),
                () -> "Real-host integration is not armed yet; missing " + missing
                        + ". Lane 1 must include :ferrite-nms, :fixtures:nms-plugin and "
                        + ":fixtures:typical-plugin in java/settings.gradle.kts and add the Paper "
                        + "repository; Lane 2 must build the typical fixture. Then rerun "
                        + "`cd java && ./gradlew :ferrite-nms:test :fixtures:nms-plugin:jar`.");

        assertNotNull(
                nmsJar,
                "The Lane 3 NMS fixture jar is missing; :ferrite-nms:test depends on "
                        + ":fixtures:nms-plugin:jar and may not skip this prerequisite.");
        assertFixtureJar(nmsJar, NMS_MAIN);
        assertFixtureJar(typicalJar, null);

        Class<?> harnessClass = loadHarnessClass(hostJar);
        assertNotNull(
                harnessClass,
                "Lane 1's host jar has no " + HARNESS_CLASS + " test seam. This is a Lane 1 "
                        + "artifact gap, not a legitimate skip: start(Path,Path) and reports() "
                        + "are required to prove both fixtures share one host JVM and that the "
                        + "NMS fixture reaches Lane 2's NmsHandleProvider.");

        Path pluginsDir = Files.createTempDirectory("ferrite-nms-integration-plugins");
        Path dataDir = Files.createTempDirectory("ferrite-nms-integration-data");
        Files.copy(typicalJar, pluginsDir.resolve("typical-plugin.jar"));
        Files.copy(nmsJar, pluginsDir.resolve("nms-plugin.jar"));

        Method start = harnessClass.getMethod("start", Path.class, Path.class);
        Object harness = start.invoke(null, pluginsDir, dataDir);
        try {
            @SuppressWarnings("unchecked")
            Map<String, String> reports =
                    (Map<String, String>) harnessClass.getMethod("reports").invoke(harness);
            assertNotNull(reports, "The host harness returned no per-plugin reports");
            assertEquals(2, reports.size(), "Expected one report for each fixture: " + reports);
            assertTrue(
                    reports.values().stream().allMatch("ok"::equals),
                    "At least one fixture failed in the shared host JVM: " + reports);
        } finally {
            if (harness instanceof AutoCloseable closeable) {
                closeable.close();
            }
        }
    }

    /** Gradle runs tests from the module directory; a system property overrides it for IDEs. */
    private static Path javaRoot() {
        String override = System.getProperty("ferrite.java.root");
        if (override != null && !override.isBlank()) {
            return Path.of(override).toAbsolutePath().normalize();
        }
        Path workingDirectory = Path.of(System.getProperty("user.dir")).toAbsolutePath().normalize();
        if (Files.isDirectory(workingDirectory.resolve("ferrite-nms"))) {
            return workingDirectory;
        }
        Path parent = workingDirectory.getParent();
        if (parent != null && Files.isDirectory(parent.resolve("ferrite-nms"))) {
            return parent;
        }
        return workingDirectory;
    }

    /** Newest non-source jar in a build directory, or null when the module has not built yet. */
    private static Path findJar(Path directory, String baseName) throws IOException {
        if (!Files.isDirectory(directory)) {
            return null;
        }
        try (Stream<Path> entries = Files.list(directory)) {
            return entries.filter(Files::isRegularFile)
                    .filter(path -> path.getFileName().toString().endsWith(".jar"))
                    .filter(path -> !path.getFileName().toString().contains("-sources"))
                    .filter(path -> !path.getFileName().toString().contains("-javadoc"))
                    .filter(path -> path.getFileName().toString().startsWith(baseName))
                    .max(Comparator.comparingLong(path -> path.toFile().lastModified()))
                    .orElse(null);
        }
    }

    private static void assertFixtureJar(Path jar, String expectedMain) throws IOException {
        try (JarFile file = new JarFile(jar.toFile())) {
            JarEntry descriptorEntry = file.getJarEntry("plugin.yml");
            assertNotNull(descriptorEntry, jar + " has no plugin.yml");
            String descriptor;
            try (InputStream input = file.getInputStream(descriptorEntry)) {
                descriptor = new String(input.readAllBytes(), StandardCharsets.UTF_8);
            }
            assertTrue(descriptor.contains("main:"), jar + " plugin.yml has no main class");
            if (expectedMain != null) {
                assertTrue(
                        descriptor.contains(expectedMain),
                        jar + " plugin.yml does not name " + expectedMain + ": " + descriptor);
            }
        }
    }

    /**
     * Loads the host's test seam from its jar when Lane 1 has not put it on the test classpath.
     * A found class keeps its loader alive for the duration of the test; a missing class closes it.
     */
    private static Class<?> loadHarnessClass(Path hostJar) throws IOException {
        ClassLoader parent = Thread.currentThread().getContextClassLoader();
        if (parent == null) {
            parent = RealHostIntegrationTest.class.getClassLoader();
        }
        URLClassLoader loader = new URLClassLoader(new URL[] {hostJar.toUri().toURL()}, parent);
        try {
            return Class.forName(HARNESS_CLASS, false, loader);
        } catch (ClassNotFoundException missing) {
            loader.close();
            return null;
        }
    }
}
