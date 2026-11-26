// Clean-room Ferrite NMS integration skeleton. No Mojang code or assets.
package dev.ferrite.nms.integration;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertNotNull;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertTrue;
import static org.junit.jupiter.api.Assertions.fail;

import java.io.File;
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
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.jar.JarEntry;
import java.util.jar.JarFile;
import java.util.stream.Stream;
import org.bukkit.plugin.java.JavaPlugin;
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
 * <p>Expected seam (from Lane 1's {@code HostIntegrationHarness}):
 *
 * <pre>{@code
 * package dev.ferrite.host.integration;
 * public final class HostIntegrationHarness implements AutoCloseable {
 *     public static HostIntegrationHarness start(Path pluginsDir, Path dataDir);
 *     public List<Map<String, Object>> reports(); // name, enabled, error per loaded jar
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
    private static final String NMS_PLUGIN_NAME = "FerriteNmsFixture";

    @Test
    void nmsFixtureJarBuildsAndLinksAgainstNmsSurface() throws Exception {
        Path nmsJar = findJar(javaRoot().resolve("fixtures/nms-plugin/build/libs"), "nms-plugin");
        assertNotNull(
                nmsJar,
                "The NMS fixture jar is missing; :ferrite-nms:test depends on :fixtures:nms-plugin:jar");
        String descriptor = assertFixtureJar(nmsJar, NMS_MAIN);
        assertTrue(descriptor.contains("name: " + NMS_PLUGIN_NAME), descriptor);
        assertTrue(descriptor.contains("load: POSTWORLD"), descriptor);
        assertTrue(descriptor.contains("folia-supported: true"), descriptor);

        try (URLClassLoader loader =
                new URLClassLoader(new URL[] {nmsJar.toUri().toURL()}, getClass().getClassLoader())) {
            Class<?> pluginClass = Class.forName(NMS_MAIN, true, loader);
            assertTrue(
                    JavaPlugin.class.isAssignableFrom(pluginClass),
                    pluginClass.getName() + " must extend org.bukkit.plugin.java.JavaPlugin");
            // Force linking of every declared method signature against ferrite-nms and paper-api.
            assertTrue(pluginClass.getDeclaredMethods().length > 0);
            assertEquals("not run", pluginClass.getMethod("lastReport").invoke(null));
            assertFalse((Boolean) pluginClass.getMethod("wasDisabled").invoke(null));
        }
    }

    @Test
    void typicalAndNmsFixturesLoadInTheSameHostJvm() throws Exception {
        Path javaRoot = javaRoot();
        Path hostJar = findJar(javaRoot.resolve("ferrite-host/build/libs"), "ferrite-host");
        Path apiImplJar = findJar(javaRoot.resolve("ferrite-api-impl/build/libs"), "ferrite-api-impl");
        Path nmsCoreJar = findJar(javaRoot.resolve("ferrite-nms/build/libs"), "ferrite-nms");
        Path typicalJar = findJar(javaRoot.resolve("fixtures/typical-plugin/build/libs"), "typical-plugin");
        Path nmsJar = findJar(javaRoot.resolve("fixtures/nms-plugin/build/libs"), "nms-plugin");

        assertNotNull(hostJar, "ferrite-host jar is missing");
        assertNotNull(apiImplJar, "ferrite-api-impl jar is missing");
        assertNotNull(nmsCoreJar, "ferrite-nms jar is missing");
        assertNotNull(
                nmsJar,
                "The NMS fixture jar is missing; :ferrite-nms:test depends on "
                        + ":fixtures:nms-plugin:jar and may not skip this prerequisite.");
        assertNotNull(typicalJar, "The typical fixture jar is missing");
        assertFixtureJar(nmsJar, NMS_MAIN);
        assertFixtureJar(typicalJar, null);

        try (URLClassLoader hostLoader = createHostClassLoader(hostJar, apiImplJar, nmsCoreJar)) {
            Class<?> harnessClass = Class.forName(HARNESS_CLASS, true, hostLoader);
            assertNotNull(
                    harnessClass,
                    "Lane 1's host jar has no " + HARNESS_CLASS + " test seam.");

            Path pluginsDir = Files.createTempDirectory("ferrite-nms-integration-plugins");
            Path dataDir = Files.createTempDirectory("ferrite-nms-integration-data");
            Files.copy(typicalJar, pluginsDir.resolve("typical-plugin.jar"));
            Files.copy(nmsJar, pluginsDir.resolve("nms-plugin.jar"));

            Method start = harnessClass.getMethod("start", Path.class, Path.class);
            Object harness = start.invoke(null, pluginsDir, dataDir);
            try {
                Object rawReports = harnessClass.getMethod("reports").invoke(harness);
                assertReportsOk(rawReports);
            } finally {
                if (harness instanceof AutoCloseable closeable) {
                    closeable.close();
                }
            }
        }
    }

    @SuppressWarnings("unchecked")
    private static void assertReportsOk(Object rawReports) {
        assertNotNull(rawReports, "The host harness returned no per-plugin reports");
        if (rawReports instanceof List<?> entries) {
            assertEquals(2, entries.size(), "Expected one report for each fixture: " + entries);
            Map<String, Map<String, Object>> byName = new LinkedHashMap<>();
            for (Object entry : entries) {
                assertTrue(entry instanceof Map<?, ?>, "Expected map report entry, got " + entry);
                Map<String, Object> report = (Map<String, Object>) entry;
                String name = String.valueOf(report.get("name"));
                byName.put(name, report);
                assertEquals(Boolean.TRUE, report.get("enabled"), "Fixture failed to enable: " + report);
                assertNull(report.get("error"), "Fixture reported an error: " + report);
            }
            assertTrue(byName.containsKey(NMS_PLUGIN_NAME), "Missing NMS fixture report: " + entries);
            return;
        }
        if (rawReports instanceof Map<?, ?> map) {
            assertEquals(2, map.size(), "Expected one report for each fixture: " + map);
            assertTrue(
                    map.values().stream().allMatch("ok"::equals),
                    "At least one fixture failed in the shared host JVM: " + map);
            return;
        }
        fail("Unsupported HostIntegrationHarness#reports() return type: " + rawReports.getClass().getName());
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

    private static String assertFixtureJar(Path jar, String expectedMain) throws IOException {
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
            return descriptor;
        }
    }

    /**
     * Builds an isolated host classloader ordered as: ferrite-host, ferrite-api-impl, ferrite-nms,
     * and then the Paper API/runtime libraries from the test classpath (excluding test-only dirs).
     */
    private static URLClassLoader createHostClassLoader(
            Path hostJar, Path apiImplJar, Path nmsCoreJar) throws IOException {
        Set<URL> urls = new LinkedHashSet<>();
        urls.add(hostJar.toUri().toURL());
        urls.add(apiImplJar.toUri().toURL());
        urls.add(nmsCoreJar.toUri().toURL());
        String classpath = System.getProperty("java.class.path", "");
        for (String entry : classpath.split(File.pathSeparator)) {
            if (entry.isBlank()) {
                continue;
            }
            Path path = Path.of(entry).toAbsolutePath().normalize();
            String normalized = path.toString().replace('\\', '/');
            if (normalized.contains("/build/classes/java/test")
                    || normalized.contains("/build/resources/test")) {
                continue;
            }
            urls.add(path.toUri().toURL());
        }
        return new URLClassLoader(urls.toArray(URL[]::new), ClassLoader.getPlatformClassLoader());
    }
}
