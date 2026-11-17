// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.datafixers.util;

import com.mojang.datafixers.kinds.App;
import com.mojang.datafixers.kinds.K1;
import java.util.Objects;
import java.util.Optional;
import java.util.function.Consumer;
import java.util.function.Function;

/** Immutable sum type with left or right value. */
public abstract class Either<L, R> implements App<Either.Mu<R>, L> {

    public static final class Mu<R> implements K1 {
    }

    private Either() {}

    public static <L, R> Either<L, R> left(L value) {
        return new Left<>(value);
    }

    public static <L, R> Either<L, R> right(R value) {
        return new Right<>(value);
    }

    public abstract Optional<L> left();

    public abstract Optional<R> right();

    public abstract <T> T map(Function<? super L, ? extends T> l, Function<? super R, ? extends T> r);

    public <L2> Either<L2, R> mapLeft(Function<? super L, ? extends L2> l) {
        return map(v -> left(l.apply(v)), Either::right);
    }

    public <R2> Either<L, R2> mapRight(Function<? super R, ? extends R2> r) {
        return map(Either::left, v -> right(r.apply(v)));
    }

    public Either<L, R> ifLeft(Consumer<? super L> consumer) {
        left().ifPresent(consumer);
        return this;
    }

    public Either<L, R> ifRight(Consumer<? super R> consumer) {
        right().ifPresent(consumer);
        return this;
    }

    public Either<R, L> swap() {
        return map(Either::right, Either::left);
    }

    public L orThrow() {
        return map(l -> l, r -> {
            if (r instanceof Throwable t) {
                throw new RuntimeException(t);
            }
            throw new IllegalStateException(String.valueOf(r));
        });
    }

    private static final class Left<L, R> extends Either<L, R> {
        private final L value;

        private Left(L value) {
            this.value = value;
        }

        @Override
        public Optional<L> left() {
            return Optional.ofNullable(value);
        }

        @Override
        public Optional<R> right() {
            return Optional.empty();
        }

        @Override
        public <T> T map(Function<? super L, ? extends T> l, Function<? super R, ? extends T> r) {
            return l.apply(value);
        }

        @Override
        public boolean equals(Object o) {
            return this == o || o instanceof Left<?, ?> other && Objects.equals(value, other.value);
        }

        @Override
        public int hashCode() {
            return Objects.hash("left", value);
        }

        @Override
        public String toString() {
            return "Left[" + value + "]";
        }
    }

    private static final class Right<L, R> extends Either<L, R> {
        private final R value;

        private Right(R value) {
            this.value = value;
        }

        @Override
        public Optional<L> left() {
            return Optional.empty();
        }

        @Override
        public Optional<R> right() {
            return Optional.ofNullable(value);
        }

        @Override
        public <T> T map(Function<? super L, ? extends T> l, Function<? super R, ? extends T> r) {
            return r.apply(value);
        }

        @Override
        public boolean equals(Object o) {
            return this == o || o instanceof Right<?, ?> other && Objects.equals(value, other.value);
        }

        @Override
        public int hashCode() {
            return Objects.hash("right", value);
        }

        @Override
        public String toString() {
            return "Right[" + value + "]";
        }
    }
}
