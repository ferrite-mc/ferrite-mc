package dev.ferrite.host;

import java.nio.file.Path;
import java.util.Map;
import java.util.Objects;

/** The host services handed to a {@link HostApi} implementation and to plugins. */
public final class HostContext {
    private final HostConfig config;
    private final HostApi api;
    private final TaskRegistry tasks;
    private final HostLogger serverLogger = new HostLogger("server");

    HostContext(HostConfig config, HostApi api, TaskRegistry tasks) {
        this.config = config;
        this.api = api;
        this.tasks = tasks;
    }

    /** Paper API version this host emulates, for example {@code 26.3}. */
    public String apiVersion() {
        return config.apiVersion();
    }

    /** Server identity from the start config. */
    public ServerInfo server() {
        return config.server();
    }

    /** Directory Rust scanned for plugin jars. */
    public Path pluginsDir() {
        return config.pluginsDir();
    }

    /** Parent directory of per-plugin data folders. */
    public Path dataDir() {
        return config.dataDir();
    }

    /** The installed SPI implementation. */
    public HostApi api() {
        return api;
    }

    /** A logger tagged {@code server}. */
    public HostLogger serverLogger() {
        return serverLogger;
    }

    /**
     * Schedules a plugin task.
     *
     * <p>Delay and period are in server ticks; period 0 is a one-shot. A task may only be
     * scheduled while its plugin is enabled. Sync tasks run on the server tick thread; async
     * tasks run on the host executor.
     */
    public long schedule(
            HostPlugin plugin,
            Runnable task,
            long delayTicks,
            long periodTicks,
            boolean async) {
        Objects.requireNonNull(plugin, "plugin");
        Objects.requireNonNull(task, "task");
        if (!plugin.isPluginEnabled()) {
            throw new IllegalStateException("plugin '" + plugin.pluginName() + "' is not enabled");
        }
        long handle = tasks.register(plugin.pluginName(), task, periodTicks > 0);
        try {
            Map<String, Object> result =
                    HostBridge.callJson(
                            "scheduler.schedule",
                            Json.object(
                                    "plugin",
                                    plugin.pluginName(),
                                    "handle",
                                    handle,
                                    "delay_ticks",
                                    Math.max(0, delayTicks),
                                    "period_ticks",
                                    Math.max(0, periodTicks),
                                    "async",
                                    async));
            long taskId = Json.longValue(result, "task_id", -1);
            if (taskId <= 0) {
                throw new HostException("the host returned an invalid task id");
            }
            tasks.attachTaskId(handle, taskId);
            return taskId;
        } catch (RuntimeException error) {
            tasks.remove(handle);
            throw error;
        }
    }

    /** Cancels a task created by {@link #schedule}. */
    public boolean cancel(long taskId) {
        TaskRegistry.Task task = tasks.byTaskId(taskId);
        if (task == null) {
            return false;
        }
        Map<String, Object> result =
                HostBridge.callJson(
                        "scheduler.cancel",
                        Json.object("plugin", task.plugin(), "task_id", taskId));
        boolean cancelled = Json.boolValue(result, "cancelled", false);
        if (cancelled) {
            tasks.removeByTaskId(taskId);
        }
        return cancelled;
    }
}
