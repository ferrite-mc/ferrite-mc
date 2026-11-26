// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.datafixers;

import com.mojang.serialization.Dynamic;

/** Clean-room DataFixer interface matching DataFixerUpper's public signature. */
public interface DataFixer {
    <T> Dynamic<T> update(DSL.TypeReference type, Dynamic<T> input, int version, int newVersion);
}
