package dev.ferrite.api.internal;

import java.util.concurrent.Callable;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.ThreadFactory;
import java.util.concurrent.atomic.AtomicReference;

/**
 * A dedicated, lazily started single thread used for standalone facade tests.
 *
 * <p>The real host binds {@link FerriteMainThread} to its own server thread instead. This
 * class deliberately has no autonomous tick loop: tests (or the host) call
 * {@link dev.ferrite.api.FerriteRuntime#tick()} to advance the scheduler.
 */
public final class QueuedMainThread implements FerriteMainThread {

    private final AtomicReference<Thread> thread = new AtomicReference<>();
    private final ExecutorService executor;

    public QueuedMainThread(String name) {
        ThreadFactory factory = runnable -> {
            Thread created = new Thread(runnable, name);
            created.setDaemon(false);
            thread.set(created);
            return created;
        };
        this.executor = Executors.newSingleThreadExecutor(factory);
    }

    @Override
    public boolean isPrimaryThread() {
        return Thread.currentThread() == this.thread.get();
    }

    @Override
    public <T> Future<T> submit(Callable<T> task) {
        return this.executor.submit(task);
    }

    @Override
    public void execute(Runnable task) {
        this.executor.execute(task);
    }

    @Override
    public void shutdown() {
        this.executor.shutdownNow();
    }
}
