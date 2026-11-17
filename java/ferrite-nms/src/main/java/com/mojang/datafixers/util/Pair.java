// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.datafixers.util;

import com.mojang.datafixers.kinds.App;
import com.mojang.datafixers.kinds.K1;
import java.util.Map;
import java.util.Objects;
import java.util.function.Function;
import java.util.stream.Collector;
import java.util.stream.Collectors;

/** Immutable ordered pair of two values. */
public final class Pair<F, S> implements App<Pair.Mu<S>, F> {

    public static final class Mu<S> implements K1 {
    }

    private final F first;
    private final S second;

    public Pair(F first, S second) {
        this.first = first;
        this.second = second;
    }

    public static <F, S> Pair<F, S> of(F first, S second) {
        return new Pair<>(first, second);
    }

    public F getFirst() {
        return first;
    }

    public S getSecond() {
        return second;
    }

    public Pair<S, F> swap() {
        return of(second, first);
    }

    public <F2> Pair<F2, S> mapFirst(Function<? super F, ? extends F2> fn) {
        return of(fn.apply(first), second);
    }

    public <S2> Pair<F, S2> mapSecond(Function<? super S, ? extends S2> fn) {
        return of(first, fn.apply(second));
    }

    public static <F, S> Collector<Pair<F, S>, ?, Map<F, S>> toMap() {
        return Collectors.toMap(Pair::getFirst, Pair::getSecond);
    }

    @Override
    public boolean equals(Object o) {
        return this == o
                || o instanceof Pair<?, ?> p
                        && Objects.equals(first, p.first)
                        && Objects.equals(second, p.second);
    }

    @Override
    public int hashCode() {
        return Objects.hash(first, second);
    }

    @Override
    public String toString() {
        return "(" + first + ", " + second + ")";
    }
}
