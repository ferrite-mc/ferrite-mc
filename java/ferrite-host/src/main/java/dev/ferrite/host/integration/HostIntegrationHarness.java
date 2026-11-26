package dev.ferrite.host.integration;

import dev.ferrite.host.Bootstrap;
import java.io.IOException;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

/**
 * Stable in-JVM harness for Lane 3 integration tests.
 *
 * <p>This runs the Java host without Rust: {@link #start(Path, Path)} starts the host, scans the
 * supplied plugin jars and loads them through {@link Bootstrap#loadPlugin(String)}. It is not a
 * replacement for the Rust {@code PluginHost}; it exists so an NMS/typical fixture can exercise
 * the host API in a plain JUnit process.
 *
 * <p>Reflection contract:
 * <pre>
 * dev.ferrite.host.integration.HostIntegrationHarness.start(java.nio.file.Path, java.nio.file.Path)
 * dev.ferrite.host.integration.HostIntegrationHarness.reports() -&gt; java.util.List
 * close()
 * </pre>
 */
public final class HostIntegrationHarness implements AutoCloseable {
    private final List<Map<String, Object>> reports = new ArrayList<>();
    private boolean closed;

    private HostIntegrationHarness() {
    }

    /**
     * Starts the Java host and loads every readable {@code .jar} in {@code pluginsDir}.
     *
     * <p>Each jar must contain a top-level {@code plugin.yml} or {@code paper-plugin.yml} with at
     * least {@code name}, {@code version} and {@code main}. A plugin failure is recorded in
     * {@link #reports()} and does not stop the remaining jars.
     */
    public static HostIntegrationHarness start(Path pluginsDir, Path dataDir) throws Exception {
        Path plugins = pluginsDir.toAbsolutePath().normalize();
        Path data = dataDir.toAbsolutePath().normalize();
        Files.createDirectories(plugins);
        Files.createDirectories(data);
        if (!Files.isDirectory(plugins)) {
            throw new IllegalArgumentException("pluginsDir is not a directory: " + plugins);
        }

        String started = Bootstrap.start(hostConfig(plugins, data));
        if (!ok(started)) {
            throw new IllegalStateException("embedded Java host did not start: " + started);
        }
        HostIntegrationHarness harness = new HostIntegrationHarness();
        try {
            for (Path jar : jars(plugins)) {
                harness.load(jar, data);
            }
        } catch (Throwable error) {
            harness.close();
            throw error;
        }
        return harness;
    }

    /** Immutable per-jar load reports, in jar path order. */
    public List<Map<String, Object>> reports() {
        return List.copyOf(reports);
    }

    private void load(Path jar, Path dataDir) {
        Map<String, Object> report = new LinkedHashMap<>();
        report.put("jar", jar.toString());
        try {
            Descriptor descriptor = descriptor(jar);
            report.put("name", descriptor.name);
            report.put("version", descriptor.version);
            report.put("main", descriptor.main);
            if (descriptor.name == null || descriptor.version == null || descriptor.main == null) {
                report.put("enabled", false);
                report.put("error", "plugin descriptor is missing name, version or main");
                reports.add(Collections.unmodifiableMap(new LinkedHashMap<>(report)));
                return;
            }
            String metadata = pluginMetadata(descriptor, jar, dataDir);
            String response = Bootstrap.loadPlugin(metadata);
            if (!ok(response)) {
                report.put("enabled", false);
                report.put("error", response == null ? "no load response" : response);
            } else {
                report.put("enabled", true);
                report.put("error", null);
            }
        } catch (Throwable error) {
            report.put("enabled", false);
            report.put("error", error.getClass().getName() + ": " + error.getMessage());
        }
        reports.add(Collections.unmodifiableMap(new LinkedHashMap<>(report)));
    }

    @Override
    public void close() {
        if (closed) {
            return;
        }
        closed = true;
        try {
            Bootstrap.shutdown("{\"reason\":\"HostIntegrationHarness close\"}");
        } catch (Throwable ignored) {
            // Close must not mask a test failure; the returned reports still show load results.
        }
    }

    private static List<Path> jars(Path plugins) throws IOException {
        try (var stream = Files.list(plugins)) {
            return stream
                    .filter(path -> path.getFileName().toString().endsWith(".jar"))
                    .sorted(Comparator.comparing(Path::toString))
                    .toList();
        }
    }

    private static Descriptor descriptor(Path jar) throws IOException {
        try (ZipFile zip = new ZipFile(jar.toFile())) {
            for (String name : List.of("plugin.yml", "paper-plugin.yml")) {
                ZipEntry entry = zip.getEntry(name);
                if (entry == null) {
                    continue;
                }
                try (InputStream input = zip.getInputStream(entry)) {
                    Map<String, String> top = simpleYaml(
                            new String(input.readAllBytes(), StandardCharsets.UTF_8));
                    return new Descriptor(
                            top.get("name"),
                            top.get("version"),
                            top.get("main"),
                            top.getOrDefault("load", "POSTWORLD"),
                            top.getOrDefault("api-version", "26.3"),
                            name);
                }
            }
        }
        return new Descriptor(null, null, null, "POSTWORLD", "26.3", "plugin.yml");
    }

    private static Map<String, String> simpleYaml(String text) {
        Map<String, String> values = new LinkedHashMap<>();
        for (String line : text.split("\\R")) {
            if (line.isEmpty()
                    || line.startsWith("#")
                    || Character.isWhitespace(line.charAt(0))) {
                continue;
            }
            int colon = line.indexOf(':');
            if (colon <= 0) {
                continue;
            }
            String key = line.substring(0, colon).trim();
            String value = line.substring(colon + 1).trim();
            if (value.isEmpty()) {
                continue;
            }
            if (value.length() >= 2
                    && ((value.startsWith("\"") && value.endsWith("\""))
                            || (value.startsWith("'") && value.endsWith("'")))) {
                value = value.substring(1, value.length() - 1);
            }
            values.putIfAbsent(key, value);
        }
        return values;
    }

    private static String hostConfig(Path plugins, Path data) {
        String apiImpl = detectApiImplClass();
        return "{\"contract_version\":3,\"api_version\":\"26.3\",\"server\":{"
                + "\"name\":\"Ferrite\",\"version\":\"0.2.1\",\"motd\":\"harness\","
                + "\"max_players\":20,\"online_mode\":true,\"view_distance\":10},"
                + "\"plugins_dir\":" + quote(plugins.toString()) + ","
                + "\"data_dir\":" + quote(data.toString()) + ","
                + "\"api_impl_class\":" + (apiImpl == null ? "null" : quote(apiImpl)) + "}";
    }

    private static String detectApiImplClass() {
        String candidate = "dev.ferrite.api.FerriteHostApi";
        try {
            Class.forName(candidate, false, HostIntegrationHarness.class.getClassLoader());
            return candidate;
        } catch (ClassNotFoundException missing) {
            return null;
        }
    }

    private static String pluginMetadata(Descriptor descriptor, Path jar, Path data) {
        return "{\"name\":" + quote(descriptor.name)
                + ",\"version\":" + quote(descriptor.version)
                + ",\"main\":" + quote(descriptor.main)
                + ",\"api_version\":" + quote(descriptor.apiVersion)
                + ",\"load\":" + quote(descriptor.load)
                + ",\"dependencies\":[],\"soft_dependencies\":[],\"load_before\":[],"
                + "\"provides\":[],\"folia_supported\":false,\"format\":" + quote(descriptor.format)
                + ",\"jar\":" + quote(jar.toAbsolutePath().normalize().toString())
                + ",\"data_folder\":" + quote(data.resolve(descriptor.name).toAbsolutePath().normalize().toString())
                + ",\"commands\":{},\"permissions\":{}}";
    }

    private static boolean ok(String response) {
        return response != null && response.contains("\"ok\":true");
    }

    private static String quote(String text) {
        StringBuilder out = new StringBuilder("\"");
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            switch (c) {
                case '"' -> out.append("\\\"");
                case '\\' -> out.append("\\\\");
                case '\n' -> out.append("\\n");
                case '\r' -> out.append("\\r");
                case '\t' -> out.append("\\t");
                default -> {
                    if (c < 0x20) {
                        out.append(String.format("\\u%04x", (int) c));
                    } else {
                        out.append(c);
                    }
                }
            }
        }
        return out.append('"').toString();
    }

    private record Descriptor(
            String name, String version, String main, String load, String apiVersion, String format) {
    }
}
