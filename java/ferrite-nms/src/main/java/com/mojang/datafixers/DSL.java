// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.datafixers;

/** Clean-room DSL type reference contract matching DataFixerUpper's public signature. */
public final class DSL {
    private DSL() {}

    @FunctionalInterface
    public interface TypeReference {
        String typeName();
    }
}
