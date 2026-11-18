package dev.ferrite.host;

import java.io.IOException;
import java.io.PrintWriter;
import java.io.StringWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;

/** The one host instance in this JVM. */
public final class HostRuntime {
    private static final Set<String> EVENTS =
            Set.of("player_join", "player_quit", "block_break", "player_interact");

    private static HostRuntime instance;

    private final HostConfig config;
    private final HostApi api;
    private final HostContext context;
    private final TaskRegistry tasks;
    private final Map<String, LoadedPlugin> plugins = new LinkedHashMap<>();
    private final List<LoadedPlugin> loadOrder = new ArrayList<>();

    private HostRuntime(
            HostConfig config, HostApi api, HostContext context, TaskRegistry tasks) {
        this.config = config;
        this.api = api;
        this.context = context;
        this.tasks = tasks;
    }

    static synchronized String start(String configJson) {
        if (instance != null) {
            throw new HostException("the Ferrite plugin host is already started");
        }
        HostConfig config = HostConfig.parse(configJson);
        if (config.contractVersion() < Bootstrap.MIN_CONTRACT_VERSION
                || config.contractVersion() > Bootstrap.CONTRACT_VERSION) {
            throw new HostException(
                    "Java host contract range "
                            + Bootstrap.MIN_CONTRACT_VERSION
                            + ".."
                            + Bootstrap.CONTRACT_VERSION
                            + " does not support Rust contract "
                            + config.contractVersion());
        }
        long negotiatedVersion = config.contractVersion();
        HostApi api = createApi(config);
        TaskRegistry tasks = new TaskRegistry();
        HostContext context = new HostContext(config, api, tasks);
        HostRuntime runtime = new HostRuntime(config, api, context, tasks);
        try {
            api.onHostStarted(context);
        } catch (Throwable error) {
            tasks.close();
            throw new HostException("HostApi.onHostStarted failed", error);
        }
        instance = runtime;
        logSafely(
                2,
                "server",
                "plugin host started: api "
                        + config.apiVersion()
                        + ", contract "
                        + negotiatedVersion);
        return Json.stringify(
                Json.object(
                        "ok",
                        true,
                        "contract_version",
                        negotiatedVersion,
                        "api",
                        config.apiImplClass()));
    }

    static synchronized HostRuntime instance() {
        if (instance == null) {
            throw new HostException("the Ferrite plugin host has not started");
        }
        return instance;
    }

    synchronized String loadPlugin(String pluginJson) {
        PluginSpec spec;
        try {
            spec = PluginSpec.fromJson(Json.asObject(Json.parse(pluginJson)));
        } catch (Throwable error) {
            return loadFailure(null, "load", error);
        }
        if (plugins.containsKey(spec.name())) {
            return loadFailure(
                    spec,
                    "load",
                    new HostException("a plugin named '" + spec.name() + "' is already loaded"));
        }

        Path dataRoot = config.dataDir().toAbsolutePath().normalize();
        Path dataFolder = spec.dataFolder().toAbsolutePath().normalize();
        if (!dataFolder.startsWith(dataRoot)) {
            return loadFailure(
                    spec,
                    "load",
                    new HostException(
                            "the data folder is outside the plugin data directory: "
                                    + dataFolder));
        }

        AutoCloseable loader = null;
        HostPlugin plugin = null;
        LoadedHostPlugin adapter = null;
        String phase = "load";
        try {
            Files.createDirectories(dataFolder);
            adapter = api.loadPlugin(context, spec);
            if (adapter != null) {
                plugin = adapter.plugin();
                loader = adapter.classLoader();
            } else {
                PluginClassLoader hostLoader = new PluginClassLoader(spec.name(), spec.jar());
                loader = hostLoader;
                Class<?> main = Class.forName(spec.main(), true, hostLoader);
                if (!JavaPlugin.class.isAssignableFrom(main)) {
                    throw new HostException(
                            spec.main()
                                    + " does not extend "
                                    + JavaPlugin.class.getName()
                                    + " (the host plugin base class)");
                }
                JavaPlugin hostPlugin = (JavaPlugin) main.getDeclaredConstructor().newInstance();
                hostPlugin.attach(context, spec);
                hostPlugin.onLoad();
                plugin = hostPlugin;
            }

            phase = "enable";
            plugin.setPluginEnabled(true);
            if (adapter != null) {
                adapter.enable();
            } else {
                plugin.onEnable();
            }

            phase = "api";
            api.onPluginEnabled(plugin);
            plugin.setPluginEnabled(true);

            LoadedPlugin loaded = new LoadedPlugin(plugin, loader, adapter);
            plugins.put(spec.name(), loaded);
            loadOrder.add(loaded);
            logSafely(2, spec.name(), "enabled " + spec.version() + " (" + spec.main() + ")");
            return Json.stringify(Json.object("ok", true, "name", spec.name()));
        } catch (Throwable error) {
            if (plugin != null) {
                if (plugin.isPluginEnabled()) {
                    try {
                        if (adapter != null) {
                            adapter.disable();
                        } else {
                            plugin.onDisable();
                        }
                    } catch (Throwable disableError) {
                        logSafely(
                                4,
                                spec.name(),
                                "onDisable after a failed enable failed\n"
                                        + stackTrace(disableError));
                    }
                }
                plugin.setPluginEnabled(false);
            }
            tasks.cancelPlugin(spec.name());
            closeQuietly(loader, spec.name());
            return loadFailure(spec, phase, error);
        }
    }

    synchronized String fireEvent(String name, String payloadJson) {
        if (!EVENTS.contains(name)) {
            return errorResponse("unknown_event", "unknown event '" + name + "'", null);
        }
        Map<String, Object> payload =
                payloadJson == null || payloadJson.isBlank()
                        ? new LinkedHashMap<>()
                        : new LinkedHashMap<>(Json.asObject(Json.parse(payloadJson)));
        try {
            EventResult result =
                    api.fireEvent(name, Collections.unmodifiableMap(payload));
            if (result == null) {
                throw new HostException("HostApi.fireEvent returned null");
            }
            return Json.stringify(
                    Json.object("ok", true, "cancelled", result.cancelled()));
        } catch (Throwable error) {
            String stack = stackTrace(error);
            logSafely(
                    4,
                    "server",
                    "event '" + name + "' failed: " + errorSummary(error) + "\n" + stack);
            return errorResponse("java_exception", errorSummary(error), stack);
        }
    }

    synchronized String runCommand(String originJson, String line) {
        try {
            CommandOrigin origin = CommandOrigin.fromJson(Json.asObject(Json.parse(originJson)));
            CommandResult result = api.runCommand(origin, line);
            if (result == null) {
                throw new HostException("HostApi.runCommand returned null");
            }
            List<Object> messages = new ArrayList<>();
            for (CommandMessage message : result.messages()) {
                messages.add(
                        Json.object(
                                "player",
                                message.playerUuid(),
                                "name",
                                message.playerName(),
                                "content",
                                message.content(),
                                "adventure",
                                message.adventure()));
            }
            return Json.stringify(
                    Json.object(
                            "ok", true, "handled", result.handled(), "messages", messages));
        } catch (Throwable error) {
            String stack = stackTrace(error);
            logSafely(
                    4,
                    "server",
                    "plugin command failed: " + errorSummary(error) + "\n" + stack);
            return errorResponse("java_exception", errorSummary(error), stack);
        }
    }

    String runTask(long handle) {
        return tasks.execute(handle);
    }

    String runTaskAsync(long handle) {
        return tasks.submitAsync(handle);
    }

    String forgetTask(long handle) {
        tasks.remove(handle);
        return Json.stringify(Json.object("ok", true));
    }

    static synchronized String shutdown(String reasonJson) {
        HostRuntime runtime = instance;
        if (runtime == null) {
            return Json.stringify(
                    Json.object("ok", true, "disabled", List.of(), "failed", List.of()));
        }
        instance = null;
        return runtime.shutdownInternal(reasonJson);
    }

    static synchronized String status() {
        if (instance == null) {
            return Json.stringify(
                    Json.object("ok", true, "started", false, "plugins", List.of(), "tasks", 0));
        }
        List<String> names = new ArrayList<>();
        for (LoadedPlugin loaded : instance.loadOrder) {
            names.add(loaded.plugin.pluginName());
        }
        return Json.stringify(
                Json.object(
                        "ok",
                        true,
                        "started",
                        true,
                        "plugins",
                        names,
                        "tasks",
                        instance.tasks.size()));
    }

    private String shutdownInternal(String reasonJson) {
        logSafely(2, "server", "plugin host shutting down: " + reasonJson);
        try {
            api.onHostStopping();
        } catch (Throwable error) {
            logSafely(4, "server", "HostApi.onHostStopping failed\n" + stackTrace(error));
        }

        List<String> disabled = new ArrayList<>();
        List<Object> failed = new ArrayList<>();
        for (int i = loadOrder.size() - 1; i >= 0; i--) {
            LoadedPlugin loaded = loadOrder.get(i);
            String name = loaded.plugin.pluginName();
            try {
                if (loaded.adapter != null) {
                    loaded.adapter.disable();
                } else {
                    loaded.plugin.onDisable();
                }
                disabled.add(name);
            } catch (Throwable error) {
                failed.add(
                        Json.object(
                                "name", name, "error", errorSummary(error)));
                logSafely(4, name, "onDisable failed\n" + stackTrace(error));
            } finally {
                loaded.plugin.setPluginEnabled(false);
                try {
                    api.onPluginDisabled(loaded.plugin);
                } catch (Throwable error) {
                    logSafely(
                            4,
                            name,
                            "HostApi.onPluginDisabled failed\n" + stackTrace(error));
                }
                tasks.cancelPlugin(name);
                closeQuietly(loaded.loader, name);
            }
        }
        plugins.clear();
        loadOrder.clear();
        tasks.close();
        return Json.stringify(Json.object("ok", true, "disabled", disabled, "failed", failed));
    }

    private static HostApi createApi(HostConfig config) {
        if (config.apiImplClass() == null) {
            return new NoopHostApi();
        }
        try {
            Class<?> type =
                    Class.forName(
                            config.apiImplClass(), true, HostRuntime.class.getClassLoader());
            Object instance = type.getDeclaredConstructor().newInstance();
            if (!(instance instanceof HostApi api)) {
                throw new HostException(
                        config.apiImplClass()
                                + " does not implement "
                                + HostApi.class.getName());
            }
            return api;
        } catch (ReflectiveOperationException error) {
            throw new HostException(
                    "cannot create HostApi " + config.apiImplClass(), error);
        }
    }

    private static String loadFailure(PluginSpec spec, String phase, Throwable error) {
        String name = spec == null ? null : spec.name();
        String message = errorSummary(error);
        String stack = stackTrace(error);
        logSafely(
                4,
                name == null ? "server" : name,
                "failed during " + phase + ": " + message + "\n" + stack);
        return Json.stringify(
                Json.object(
                        "ok",
                        false,
                        "name",
                        name,
                        "phase",
                        phase,
                        "error",
                        message,
                        "stack",
                        stack));
    }

    static String errorResponse(String code, String message, String stack) {
        return Json.stringify(
                Json.object("ok", false, "code", code, "error", message, "stack", stack));
    }

    static String stackTrace(Throwable error) {
        StringWriter writer = new StringWriter();
        error.printStackTrace(new PrintWriter(writer));
        return writer.toString().stripTrailing();
    }

    static String errorSummary(Throwable error) {
        Throwable cause = error;
        while (cause.getCause() != null && cause.getCause() != cause) {
            cause = cause.getCause();
        }
        String message = cause.getMessage();
        return cause.getClass().getName()
                + (message == null || message.isBlank() ? "" : ": " + message);
    }

    private static void logSafely(int level, String plugin, String message) {
        try {
            HostBridge.log(level, plugin, message);
        } catch (RuntimeException | LinkageError ignored) {
            // Logging must not turn a plugin failure into a host failure.
        }
    }

    private static void closeQuietly(AutoCloseable loader, String name) {
        if (loader == null) {
            return;
        }
        try {
            loader.close();
        } catch (Exception error) {
            logSafely(3, name, "closing the plugin class loader failed: " + error);
        }
    }

    private record LoadedPlugin(
            HostPlugin plugin, AutoCloseable loader, LoadedHostPlugin adapter) {
    }
}
