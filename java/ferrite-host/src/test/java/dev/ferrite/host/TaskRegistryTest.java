package dev.ferrite.host;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertFalse;
import static org.junit.jupiter.api.Assertions.assertTrue;

import java.util.Map;
import java.util.concurrent.atomic.AtomicInteger;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.Test;

class TaskRegistryTest {
    private final TaskRegistry tasks = new TaskRegistry((level, plugin, message) -> {});
    private final AtomicInteger runs = new AtomicInteger();

    @AfterEach
    void close() {
        tasks.close();
    }

    private static Map<String, Object> result(String json) {
        return Json.asObject(Json.parse(json));
    }

    @Test
    void one_shot_runs_once_and_is_removed() {
        long handle = tasks.register("Test", runs::incrementAndGet, false);
        Map<String, Object> first = result(tasks.execute(handle));
        assertEquals(Boolean.TRUE, first.get("ok"));
        assertEquals(Boolean.FALSE, first.get("repeat"));
        assertEquals(1, runs.get());
        assertEquals(0, tasks.size());
        Map<String, Object> second = result(tasks.execute(handle));
        assertEquals(Boolean.TRUE, second.get("missing"));
    }

    @Test
    void repeating_stays_until_removed() {
        long handle = tasks.register("Test", runs::incrementAndGet, true);
        assertEquals(Boolean.TRUE, result(tasks.execute(handle)).get("repeat"));
        assertEquals(Boolean.TRUE, result(tasks.execute(handle)).get("repeat"));
        assertEquals(2, runs.get());
        assertEquals(1, tasks.size());
        assertTrue(tasks.remove(handle));
        assertEquals(0, tasks.size());
    }

    @Test
    void throwing_task_is_removed_and_reported() {
        long handle =
                tasks.register(
                        "Broken",
                        () -> {
                            throw new IllegalStateException("boom");
                        },
                        true);
        Map<String, Object> result = result(tasks.execute(handle));
        assertEquals(Boolean.FALSE, result.get("ok"));
        assertEquals(Boolean.FALSE, result.get("repeat"));
        assertEquals("java_exception", result.get("error"));
        assertEquals(0, tasks.size());
    }

    @Test
    void cancel_plugin_only_removes_that_plugins_tasks() {
        long first = tasks.register("One", runs::incrementAndGet, false);
        long second = tasks.register("Two", runs::incrementAndGet, false);
        assertEquals(1, tasks.cancelPlugin("One"));
        assertEquals(1, tasks.size());
        assertEquals(Boolean.TRUE, result(tasks.execute(first)).get("missing"));
        assertEquals(Boolean.TRUE, result(tasks.execute(second)).get("ok"));
        assertFalse(tasks.removeByTaskId(999));
    }
}
