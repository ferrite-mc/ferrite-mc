package dev.ferrite.host.integration;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.io.IOException;
import java.io.OutputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Map;
import java.util.zip.ZipEntry;
import java.util.zip.ZipOutputStream;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class HostIntegrationHarnessTest {
    @TempDir
    Path temporary;

    @Test
    void starts_loads_reports_and_closes_one_plugin_jar() throws Exception {
        Path plugins = temporary.resolve("plugins");
        Files.createDirectories(plugins);
        Path jar = plugins.resolve("Harness.jar");
        try (OutputStream output = Files.newOutputStream(jar);
                ZipOutputStream zip = new ZipOutputStream(output)) {
            zip.putNextEntry(new ZipEntry("plugin.yml"));
            zip.write(
                    ("name: Harness\n"
                                    + "version: 1.0\n"
                                    + "main: dev.ferrite.host.integration.HarnessPlugin\n"
                                    + "api-version: \"26.3\"\n")
                            .getBytes(java.nio.charset.StandardCharsets.UTF_8));
            zip.closeEntry();
        }

        HarnessPlugin.enabled = false;
        HarnessPlugin.disabled = false;
        try (HostIntegrationHarness harness = HostIntegrationHarness.start(plugins, temporary.resolve("data"))) {
            List<Map<String, Object>> reports = harness.reports();
            assertEquals(1, reports.size());
            assertEquals("Harness", reports.get(0).get("name"));
            assertEquals("dev.ferrite.host.integration.HarnessPlugin", reports.get(0).get("main"));
            assertEquals(true, reports.get(0).get("enabled"));
            assertEquals(null, reports.get(0).get("error"));
            assertTrue(HarnessPlugin.enabled);
        }
        assertTrue(HarnessPlugin.disabled);
    }
}
