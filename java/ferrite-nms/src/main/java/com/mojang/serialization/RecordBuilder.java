// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization;

import java.util.LinkedHashMap;
import java.util.Map;
import java.util.function.UnaryOperator;

/** Builder for serialized maps/records under a {@link DynamicOps}. */
public interface RecordBuilder<T> {
    DynamicOps<T> ops();

    RecordBuilder<T> add(T key, T value);

    RecordBuilder<T> add(T key, DataResult<T> value);

    RecordBuilder<T> add(DataResult<T> key, DataResult<T> value);

    RecordBuilder<T> withErrorsFrom(DataResult<?> result);

    RecordBuilder<T> setLifecycle(Lifecycle lifecycle);

    RecordBuilder<T> mapError(UnaryOperator<String> onError);

    DataResult<T> build(T prefix);

    default DataResult<T> build(DataResult<T> prefix) {
        return prefix.flatMap(this::build);
    }

    default RecordBuilder<T> add(String key, T value) {
        return add(ops().createString(key), value);
    }

    default RecordBuilder<T> add(String key, DataResult<T> value) {
        return add(ops().createString(key), value);
    }

    abstract class AbstractBuilder<T, R> implements RecordBuilder<T> {
        private final DynamicOps<T> ops;
        private DataResult<R> builder;

        protected AbstractBuilder(DynamicOps<T> ops) {
            this.ops = ops;
            this.builder = DataResult.success(initBuilder(), Lifecycle.stable());
        }

        @Override
        public DynamicOps<T> ops() {
            return ops;
        }

        protected abstract R initBuilder();

        protected abstract DataResult<T> build(R builder, T prefix);

        @Override
        public DataResult<T> build(T prefix) {
            return builder.flatMap(b -> build(b, prefix));
        }

        protected abstract R append(T key, T value, R builder);

        @Override
        public RecordBuilder<T> add(T key, T value) {
            builder = builder.map(b -> append(key, value, b));
            return this;
        }

        @Override
        public RecordBuilder<T> add(T key, DataResult<T> value) {
            builder = builder.apply2((b, v) -> append(key, v, b), value);
            return this;
        }

        @Override
        public RecordBuilder<T> add(DataResult<T> key, DataResult<T> value) {
            builder = builder.apply3((b, k, v) -> append(k, v, b), key, value);
            return this;
        }

        @Override
        public RecordBuilder<T> withErrorsFrom(DataResult<?> result) {
            if (result.isError()) {
                builder = builder.flatMap(b -> DataResult.error(
                        () -> result.error().map(DataResult.Error::message).orElse("error"), b));
            }
            return this;
        }

        @Override
        public RecordBuilder<T> setLifecycle(Lifecycle lifecycle) {
            return this;
        }

        @Override
        public RecordBuilder<T> mapError(UnaryOperator<String> onError) {
            builder = builder.mapError(onError);
            return this;
        }
    }

    abstract class AbstractStringBuilder<T, R> extends AbstractBuilder<T, R> {
        protected AbstractStringBuilder(DynamicOps<T> ops) {
            super(ops);
        }

        protected abstract R append(String key, T value, R builder);

        @Override
        protected R append(T key, T value, R builder) {
            String keyStr = ops().getStringValue(key).getOrThrow();
            return append(keyStr, value, builder);
        }

        @Override
        public RecordBuilder<T> add(String key, T value) {
            return super.add(ops().createString(key), value);
        }

        @Override
        public RecordBuilder<T> add(String key, DataResult<T> value) {
            return super.add(ops().createString(key), value);
        }
    }

    final class MapBuilder<T> extends AbstractBuilder<T, Map<T, T>> {
        public MapBuilder(DynamicOps<T> ops) {
            super(ops);
        }

        @Override
        protected Map<T, T> initBuilder() {
            return new LinkedHashMap<>();
        }

        @Override
        protected Map<T, T> append(T key, T value, Map<T, T> builder) {
            builder.put(key, value);
            return builder;
        }

        @Override
        protected DataResult<T> build(Map<T, T> builder, T prefix) {
            return ops().mergeToMap(prefix, builder);
        }
    }
}
