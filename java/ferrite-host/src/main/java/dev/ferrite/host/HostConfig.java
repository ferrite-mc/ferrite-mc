package dev.ferrite.host;

import java.nio.file.Path;
import java.util.Map;

/** The {@code Bootstrap.start} configuration. */
final class HostConfig {
    private final long contractVersion;
    private final String apiVersion;
    private final ServerInfo server;
    private final Path pluginsDir;
    private final Path dataDir;
    private final String apiImplClass;

    private HostConfig(
            long contractVersion,
            String apiVersion,
            ServerInfo server,
            Path pluginsDir,
            Path dataDir,
            String apiImplClass) {
        this.contractVersion = contractVersion;
        this.apiVersion = apiVersion;
        this.server = server;
        this.pluginsDir = pluginsDir;
        this.dataDir = dataDir;
        this.apiImplClass = apiImplClass;
    }

    static HostConfig parse(String json) {
        Map<String, Object> root = Json.asObject(Json.parse(json));
        String apiImplClass = Json.string(root, "api_impl_class", null);
        if (apiImplClass != null && apiImplClass.isBlank()) {
            apiImplClass = null;
        }
        return new HostConfig(
                Json.longValue(root, "contract_version", 0),
                Json.requireString(root, "api_version"),
                ServerInfo.fromJson(Json.asObject(root.get("server"))),
                Path.of(Json.requireString(root, "plugins_dir")),
                Path.of(Json.requireString(root, "data_dir")),
                apiImplClass);
    }

    long contractVersion() {
        return contractVersion;
    }

    String apiVersion() {
        return apiVersion;
    }

    ServerInfo server() {
        return server;
    }

    Path pluginsDir() {
        return pluginsDir;
    }

    Path dataDir() {
        return dataDir;
    }

    String apiImplClass() {
        return apiImplClass;
    }
}
