// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization;

/** Stability lifecycle marker for codecs and results. */
public sealed interface Lifecycle {
    static Lifecycle experimental() {
        return Experimental.INSTANCE;
    }

    static Lifecycle stable() {
        return Stable.INSTANCE;
    }

    default Lifecycle add(Lifecycle other) {
        if (this == Experimental.INSTANCE || other == Experimental.INSTANCE) {
            return Experimental.INSTANCE;
        }
        return Stable.INSTANCE;
    }

    final class Experimental implements Lifecycle {
        static final Experimental INSTANCE = new Experimental();

        private Experimental() {}

        @Override
        public String toString() {
            return "Experimental";
        }
    }

    final class Stable implements Lifecycle {
        static final Stable INSTANCE = new Stable();

        private Stable() {}

        @Override
        public String toString() {
            return "Stable";
        }
    }
}
