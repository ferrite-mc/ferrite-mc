package dev.ferrite.host;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.RejectedExecutionException;
import java.util.concurrent.ThreadFactory;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;

/**
 * Holds the {@link Runnable} bodies of plugin tasks.
 *
 * <p>Rust owns the queue, the tick counter, cancellation tokens and repeat timing. Java only
 * maps an opaque handle to a body and answers {@code Bootstrap.runTask}/{@code runTaskAsync}.
 * A throwing task is removed and reported as failed, like Bukkit cancelling a broken task.
 */
final class TaskRegistry {
    @FunctionalInterface
    interface LogSink {
        void log(int level, String plugin, String message);
    }

    static final class Task {
        private final long handle;
        private final String plugin;
        private final Runnable runnable;
        private final boolean repeating;
        private volatile long taskId;

        private Task(long handle, String plugin, Runnable runnable, boolean repeating) {
            this.handle = handle;
            this.plugin = plugin;
            this.runnable = runnable;
            this.repeating = repeating;
        }

        String plugin() {
            return plugin;
        }
    }

    private final ConcurrentHashMap<Long, Task> tasks = new ConcurrentHashMap<>();
    private final AtomicLong nextHandle = new AtomicLong(0);
    private final AtomicInteger nextThread = new AtomicInteger(0);
    private final ExecutorService async;
    private final LogSink log;

    TaskRegistry() {
        this(TaskRegistry::logToHost);
    }

    TaskRegistry(LogSink log) {
        this.log = log;
        ThreadFactory factory = runnable -> {
            Thread thread =
                    new Thread(runnable, "ferrite-plugin-async-" + nextThread.incrementAndGet());
            thread.setDaemon(true);
            return thread;
        };
        this.async = Executors.newCachedThreadPool(factory);
    }

    long register(String plugin, Runnable runnable, boolean repeating) {
        long handle = nextHandle.incrementAndGet();
        tasks.put(handle, new Task(handle, plugin, runnable, repeating));
        return handle;
    }

    void attachTaskId(long handle, long taskId) {
        Task task = tasks.get(handle);
        if (task != null) {
            task.taskId = taskId;
        }
    }

    Task byTaskId(long taskId) {
        for (Task task : tasks.values()) {
            if (task.taskId == taskId) {
                return task;
            }
        }
        return null;
    }

    String execute(long handle) {
        Task task = tasks.get(handle);
        if (task == null) {
            return missing();
        }
        try {
            task.runnable.run();
        } catch (Throwable error) {
            tasks.remove(handle);
            String stack = HostRuntime.stackTrace(error);
            log.log(
                    4,
                    task.plugin,
                    "scheduled task "
                            + task.taskId
                            + " failed: "
                            + HostRuntime.errorSummary(error)
                            + "\n"
                            + stack);
            return Json.stringify(
                    Json.object(
                            "ok",
                            false,
                            "repeat",
                            false,
                            "error",
                            "java_exception",
                            "stack",
                            stack));
        }
        if (task.repeating) {
            return Json.stringify(Json.object("ok", true, "repeat", true));
        }
        tasks.remove(handle, task);
        return Json.stringify(Json.object("ok", true, "repeat", false));
    }

    String submitAsync(long handle) {
        Task task = tasks.get(handle);
        if (task == null) {
            return missing();
        }
        try {
            async.execute(() -> execute(handle));
        } catch (RejectedExecutionException error) {
            tasks.remove(handle);
            return Json.stringify(
                    Json.object(
                            "ok",
                            false,
                            "repeat",
                            false,
                            "error",
                            "shutdown",
                            "stack",
                            HostRuntime.stackTrace(error)));
        }
        return Json.stringify(
                Json.object(
                        "ok", true, "submitted", true, "repeat", task.repeating));
    }

    boolean remove(long handle) {
        return tasks.remove(handle) != null;
    }

    boolean removeByTaskId(long taskId) {
        Task task = byTaskId(taskId);
        return task != null && tasks.remove(task.handle, task);
    }

    int cancelPlugin(String plugin) {
        int removed = 0;
        for (Task task : tasks.values()) {
            if (task.plugin.equals(plugin) && tasks.remove(task.handle, task)) {
                removed++;
            }
        }
        return removed;
    }

    int size() {
        return tasks.size();
    }

    void close() {
        tasks.clear();
        async.shutdownNow();
    }

    private static String missing() {
        return Json.stringify(
                Json.object("ok", false, "missing", true, "error", "task_not_found"));
    }

    private static void logToHost(int level, String plugin, String message) {
        HostBridge.log(level, plugin, message);
    }
}
