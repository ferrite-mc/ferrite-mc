package dev.ferrite.api.internal;

import java.util.concurrent.Callable;
import java.util.concurrent.Future;

/**
 * The facade's server main thread.
 *
 * <p>Bukkit events, commands and synchronous scheduler tasks must run on one thread. The
 * Rust host owns the real tick loop; it supplies an implementation bound to its server
 * thread, or uses {@link QueuedMainThread} for standalone tests. The facade never blocks
 * a region thread waiting for another region: it only queues work onto this one thread.
 */
public interface FerriteMainThread {

    boolean isPrimaryThread();

    <T> Future<T> submit(Callable<T> task);

    void execute(Runnable task);

    void shutdown();
}
