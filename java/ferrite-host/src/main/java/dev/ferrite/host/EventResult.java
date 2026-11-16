package dev.ferrite.host;

/** What listeners left behind for one cancellable event. */
public final class EventResult {
    private static final EventResult PASS = new EventResult(false);
    private static final EventResult CANCELLED = new EventResult(true);

    private final boolean cancelled;

    private EventResult(boolean cancelled) {
        this.cancelled = cancelled;
    }

    /** The event proceeds. */
    public static EventResult pass() {
        return PASS;
    }

    /** A listener cancelled the event. */
    public static EventResult cancel() {
        return CANCELLED;
    }

    public boolean cancelled() {
        return cancelled;
    }
}
