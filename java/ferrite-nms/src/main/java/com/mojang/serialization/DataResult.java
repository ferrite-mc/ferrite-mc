// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization;

import com.mojang.datafixers.util.Either;
import com.mojang.datafixers.util.Function3;
import java.util.Objects;
import java.util.Optional;
import java.util.function.BiFunction;
import java.util.function.Consumer;
import java.util.function.Function;
import java.util.function.Supplier;
import java.util.function.UnaryOperator;

/** Immutable result carrying either a value, a partial value + error, or an error. */
public final class DataResult<R> {

    public record Error<R>(Supplier<String> messageSupplier, Optional<R> partialValue) {
        public String message() {
            return messageSupplier.get();
        }
    }

    private final Either<R, Error<R>> result;
    private final Lifecycle lifecycle;

    private DataResult(Either<R, Error<R>> result, Lifecycle lifecycle) {
        this.result = Objects.requireNonNull(result, "result");
        this.lifecycle = Objects.requireNonNull(lifecycle, "lifecycle");
    }

    public static <R> DataResult<R> success(R result) {
        return success(result, Lifecycle.experimental());
    }

    public static <R> DataResult<R> success(R result, Lifecycle lifecycle) {
        return new DataResult<>(Either.left(result), lifecycle);
    }

    public static <R> DataResult<R> error(Supplier<String> message) {
        return error(message, Lifecycle.experimental());
    }

    public static <R> DataResult<R> error(Supplier<String> message, Lifecycle lifecycle) {
        return new DataResult<>(Either.right(new Error<>(message, Optional.empty())), lifecycle);
    }

    public static <R> DataResult<R> error(Supplier<String> message, R partialValue) {
        return new DataResult<>(
                Either.right(new Error<>(message, Optional.ofNullable(partialValue))),
                Lifecycle.experimental());
    }

    public Either<R, Error<R>> get() {
        return result;
    }

    public Optional<R> result() {
        return result.left();
    }

    public Optional<Error<R>> error() {
        return result.right();
    }

    public Optional<R> resultOrPartial(Consumer<String> onError) {
        return result.map(
                Optional::ofNullable,
                err -> {
                    onError.accept(err.message());
                    return err.partialValue();
                });
    }

    public Optional<R> resultOrPartial() {
        return resultOrPartial(msg -> {});
    }

    public boolean isSuccess() {
        return result.left().isPresent();
    }

    public boolean isError() {
        return result.right().isPresent();
    }

    public Lifecycle lifecycle() {
        return lifecycle;
    }

    public R getOrThrow() {
        return getOrThrow(IllegalStateException::new);
    }

    public R getOrThrow(Function<String, ? extends RuntimeException> exceptionSupplier) {
        return result.map(r -> r, err -> {
            throw exceptionSupplier.apply(err.message());
        });
    }

    public R orThrow() {
        return getOrThrow();
    }

    public <T> DataResult<T> map(Function<? super R, ? extends T> function) {
        return result.map(
                r -> success(function.apply(r), lifecycle),
                err -> new DataResult<>(
                        Either.right(new Error<>(
                                err.messageSupplier(),
                                err.partialValue().map(function))),
                        lifecycle));
    }

    public <T> DataResult<T> flatMap(Function<? super R, ? extends DataResult<T>> function) {
        return result.map(
                r -> {
                    DataResult<T> next = function.apply(r);
                    return new DataResult<>(next.result, lifecycle.add(next.lifecycle));
                },
                err -> new DataResult<>(
                        Either.right(new Error<>(
                                err.messageSupplier(),
                                err.partialValue().flatMap(v -> function.apply(v).resultOrPartial()))),
                        lifecycle));
    }

    public DataResult<R> mapError(UnaryOperator<String> function) {
        return result.map(
                r -> this,
                err -> new DataResult<>(
                        Either.right(new Error<>(
                                () -> function.apply(err.message()),
                                err.partialValue())),
                        lifecycle));
    }

    public DataResult<R> promotePartial(Consumer<String> onError) {
        return result.map(
                r -> this,
                err -> {
                    onError.accept(err.message());
                    return err.partialValue()
                            .map(v -> success(v, lifecycle))
                            .orElseGet(() -> error(err.messageSupplier(), lifecycle));
                });
    }

    public <R2, S> DataResult<S> apply2(BiFunction<R, R2, S> function, DataResult<R2> second) {
        return flatMap(r1 -> second.map(r2 -> function.apply(r1, r2)));
    }

    public <R2, R3, S> DataResult<S> apply3(
            Function3<R, R2, R3, S> function, DataResult<R2> second, DataResult<R3> third) {
        return flatMap(r1 -> second.flatMap(r2 -> third.map(r3 -> function.apply(r1, r2, r3))));
    }

    public <T> DataResult<T> ap(DataResult<Function<R, T>> funcResult) {
        return funcResult.flatMap(this::map);
    }

    public DataResult<R> setPartial(R partial) {
        return result.map(
                r -> this,
                err -> new DataResult<>(
                        Either.right(new Error<>(err.messageSupplier(), Optional.ofNullable(partial))),
                        lifecycle));
    }

    @Override
    public boolean equals(Object o) {
        return this == o || o instanceof DataResult<?> other && Objects.equals(result, other.result);
    }

    @Override
    public int hashCode() {
        return result.hashCode();
    }

    @Override
    public String toString() {
        return result.map(r -> "DataResult[" + r + "]", e -> "DataResult.Error[" + e.message() + "]");
    }
}
