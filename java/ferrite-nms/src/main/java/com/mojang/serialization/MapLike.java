// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization;

import com.mojang.datafixers.util.Pair;
import java.util.Map;
import java.util.stream.Stream;

/** Read-only view over a serialized map. */
public interface MapLike<T> {
    T get(T key);

    T get(String key);

    Stream<Pair<T, T>> entries();

    static <T> MapLike<T> forMap(Map<T, T> map, DynamicOps<T> ops) {
        return new MapLike<>() {
            @Override
            public T get(T key) {
                return map.get(key);
            }

            @Override
            public T get(String key) {
                return map.get(ops.createString(key));
            }

            @Override
            public Stream<Pair<T, T>> entries() {
                return map.entrySet().stream().map(e -> Pair.of(e.getKey(), e.getValue()));
            }
        };
    }
}
