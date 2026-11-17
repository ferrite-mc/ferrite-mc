// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.datafixers.util;

@FunctionalInterface
public interface Function5<T1, T2, T3, T4, T5, R> {
    R apply(T1 t1, T2 t2, T3 t3, T4 t4, T5 t5);
}
