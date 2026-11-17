// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization;

import java.util.ArrayList;
import java.util.List;
import java.util.function.UnaryOperator;

/** Builder for serialized lists under a {@link DynamicOps}. */
public interface ListBuilder<T> {
    DynamicOps<T> ops();

    DataResult<T> build(T prefix);

    ListBuilder<T> add(T value);

    ListBuilder<T> add(DataResult<T> value);

    ListBuilder<T> withErrorsFrom(DataResult<?> result);

    ListBuilder<T> mapError(UnaryOperator<String> onError);

    default DataResult<T> build(DataResult<T> prefix) {
        return prefix.flatMap(this::build);
    }

    final class Builder<T> implements ListBuilder<T> {
        private final DynamicOps<T> ops;
        private DataResult<List<T>> builder = DataResult.success(new ArrayList<>());

        public Builder(DynamicOps<T> ops) {
            this.ops = ops;
        }

        @Override
        public DynamicOps<T> ops() {
            return ops;
        }

        @Override
        public ListBuilder<T> add(T value) {
            builder = builder.map(list -> {
                list.add(value);
                return list;
            });
            return this;
        }

        @Override
        public ListBuilder<T> add(DataResult<T> value) {
            builder = builder.apply2((list, elem) -> {
                list.add(elem);
                return list;
            }, value);
            return this;
        }

        @Override
        public ListBuilder<T> withErrorsFrom(DataResult<?> result) {
            if (result.isError()) {
                builder = builder.flatMap(list -> DataResult.error(
                        () -> result.error().map(DataResult.Error::message).orElse("error"), list));
            }
            return this;
        }

        @Override
        public ListBuilder<T> mapError(UnaryOperator<String> onError) {
            builder = builder.mapError(onError);
            return this;
        }

        @Override
        public DataResult<T> build(T prefix) {
            return builder.flatMap(list -> ops.mergeToList(prefix, list));
        }
    }
}
