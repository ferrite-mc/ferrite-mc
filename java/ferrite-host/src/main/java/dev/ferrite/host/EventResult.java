package dev.ferrite.host;

import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.Map;

/** What listeners left behind for one cancellable or mutable event. */
public final class EventResult {
    private static final EventResult PASS = new EventResult(false, Map.of());
    private static final EventResult CANCELLED = new EventResult(true, Map.of());

    private final boolean cancelled;
    private final Map<String, Object> mutations;

    private EventResult(boolean cancelled, Map<String, Object> mutations) {
        this.cancelled = cancelled;
        this.mutations = mutations.isEmpty()
                ? Map.of()
                : Collections.unmodifiableMap(new LinkedHashMap<>(mutations));
    }

    /** The event proceeds. */
    public static EventResult pass() {
        return PASS;
    }

    /** A listener cancelled the event. */
    public static EventResult cancel() {
        return CANCELLED;
    }

    /** Returns an event result carrying optional listener mutations back to Rust. */
    public static EventResult withMutations(boolean cancelled, Map<String, Object> mutations) {
        if (mutations == null || mutations.isEmpty()) {
            return cancelled ? CANCELLED : PASS;
        }
        return new EventResult(cancelled, mutations);
    }

    public boolean cancelled() {
        return cancelled;
    }

    public Map<String, Object> mutations() {
        return mutations;
    }
}
