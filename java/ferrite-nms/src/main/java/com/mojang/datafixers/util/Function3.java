// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.datafixers.util;

@FunctionalInterface
public interface Function3<T1, T2, T3, R> {
    R apply(T1 t1, T2 t2, T3 t3);
}
