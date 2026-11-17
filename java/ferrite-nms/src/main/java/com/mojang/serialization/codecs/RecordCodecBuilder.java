// Clean-room Ferrite DFU compatibility code. No Mojang code or assets.
package com.mojang.serialization.codecs;

import com.mojang.datafixers.kinds.App;
import com.mojang.datafixers.kinds.K1;
import com.mojang.datafixers.util.Function3;
import com.mojang.datafixers.util.Function4;
import com.mojang.datafixers.util.Function5;
import com.mojang.serialization.Codec;
import com.mojang.serialization.DataResult;
import com.mojang.serialization.DynamicOps;
import com.mojang.serialization.MapCodec;
import com.mojang.serialization.MapDecoder;
import com.mojang.serialization.MapEncoder;
import com.mojang.serialization.MapLike;
import com.mojang.serialization.RecordBuilder;
import java.util.function.BiFunction;
import java.util.function.Function;

/** Clean-room {@code RecordCodecBuilder<O>} supporting 1-to-5 field product codecs. */
public final class RecordCodecBuilder<O, F> implements App<RecordCodecBuilder.Mu<O>, F> {

    public static final class Mu<O> implements K1 {}

    private final MapCodec<F> codec;
    private final Function<O, F> getter;

    private RecordCodecBuilder(MapCodec<F> codec, Function<O, F> getter) {
        this.codec = codec;
        this.getter = getter;
    }

    public static <O> Instance<O> instance() {
        return new Instance<>();
    }

    public static <O> MapCodec<O> mapCodec(Function<Instance<O>, ? extends App<Mu<O>, O>> builder) {
        @SuppressWarnings("unchecked")
        RecordCodecBuilder<O, O> built = (RecordCodecBuilder<O, O>) builder.apply(instance());
        return built.codec;
    }

    public static <O> Codec<O> create(Function<Instance<O>, ? extends App<Mu<O>, O>> builder) {
        return mapCodec(builder).codec();
    }

    public static <O, F> RecordCodecBuilder<O, F> of(
            Function<O, F> getter, String name, Codec<F> fieldCodec) {
        return new RecordCodecBuilder<>(fieldCodec.fieldOf(name), getter);
    }

    public static <O, F> RecordCodecBuilder<O, F> point(F instance) {
        return new RecordCodecBuilder<>(MapCodec.unit(instance), o -> instance);
    }

    public static final class Instance<O> {
        private Instance() {}

        public <A> RecordCodecBuilder<O, A> field(MapCodec.RecordCodecField<O, A> field) {
            return new RecordCodecBuilder<>(field.codec(), field.getter());
        }

        public <A> Group1<O, A> group(App<MapCodec.RecordCodecField.Mu<O>, A> f1) {
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, A> c1 = (MapCodec.RecordCodecField<O, A>) f1;
            return new Group1<>(c1);
        }

        public <A, B> Group2<O, A, B> group(
                App<MapCodec.RecordCodecField.Mu<O>, A> f1,
                App<MapCodec.RecordCodecField.Mu<O>, B> f2) {
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, A> c1 = (MapCodec.RecordCodecField<O, A>) f1;
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, B> c2 = (MapCodec.RecordCodecField<O, B>) f2;
            return new Group2<>(c1, c2);
        }

        public <A, B, C> Group3<O, A, B, C> group(
                App<MapCodec.RecordCodecField.Mu<O>, A> f1,
                App<MapCodec.RecordCodecField.Mu<O>, B> f2,
                App<MapCodec.RecordCodecField.Mu<O>, C> f3) {
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, A> c1 = (MapCodec.RecordCodecField<O, A>) f1;
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, B> c2 = (MapCodec.RecordCodecField<O, B>) f2;
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, C> c3 = (MapCodec.RecordCodecField<O, C>) f3;
            return new Group3<>(c1, c2, c3);
        }

        public <A, B, C, D> Group4<O, A, B, C, D> group(
                App<MapCodec.RecordCodecField.Mu<O>, A> f1,
                App<MapCodec.RecordCodecField.Mu<O>, B> f2,
                App<MapCodec.RecordCodecField.Mu<O>, C> f3,
                App<MapCodec.RecordCodecField.Mu<O>, D> f4) {
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, A> c1 = (MapCodec.RecordCodecField<O, A>) f1;
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, B> c2 = (MapCodec.RecordCodecField<O, B>) f2;
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, C> c3 = (MapCodec.RecordCodecField<O, C>) f3;
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, D> c4 = (MapCodec.RecordCodecField<O, D>) f4;
            return new Group4<>(c1, c2, c3, c4);
        }

        public <A, B, C, D, E> Group5<O, A, B, C, D, E> group(
                App<MapCodec.RecordCodecField.Mu<O>, A> f1,
                App<MapCodec.RecordCodecField.Mu<O>, B> f2,
                App<MapCodec.RecordCodecField.Mu<O>, C> f3,
                App<MapCodec.RecordCodecField.Mu<O>, D> f4,
                App<MapCodec.RecordCodecField.Mu<O>, E> f5) {
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, A> c1 = (MapCodec.RecordCodecField<O, A>) f1;
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, B> c2 = (MapCodec.RecordCodecField<O, B>) f2;
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, C> c3 = (MapCodec.RecordCodecField<O, C>) f3;
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, D> c4 = (MapCodec.RecordCodecField<O, D>) f4;
            @SuppressWarnings("unchecked")
            MapCodec.RecordCodecField<O, E> c5 = (MapCodec.RecordCodecField<O, E>) f5;
            return new Group5<>(c1, c2, c3, c4, c5);
        }

        public <T> RecordCodecBuilder<O, T> point(T value) {
            return RecordCodecBuilder.point(value);
        }
    }

    public static final class Group1<O, A> {
        private final MapCodec.RecordCodecField<O, A> f1;

        private Group1(MapCodec.RecordCodecField<O, A> f1) {
            this.f1 = f1;
        }

        public RecordCodecBuilder<O, O> apply(Instance<O> ignored, Function<A, O> fn) {
            MapCodec<O> mc = MapCodec.of(
                    new MapEncoder<>() {
                        @Override
                        public <T> RecordBuilder<T> encode(O input, DynamicOps<T> ops, RecordBuilder<T> prefix) {
                            return f1.codec().encode(f1.getter().apply(input), ops, prefix);
                        }
                    },
                    new MapDecoder<>() {
                        @Override
                        public <T> DataResult<O> decode(DynamicOps<T> ops, MapLike<T> input) {
                            return f1.codec().decode(ops, input).map(fn);
                        }
                    });
            return new RecordCodecBuilder<>(mc, Function.identity());
        }
    }

    public static final class Group2<O, A, B> {
        private final MapCodec.RecordCodecField<O, A> f1;
        private final MapCodec.RecordCodecField<O, B> f2;

        private Group2(MapCodec.RecordCodecField<O, A> f1, MapCodec.RecordCodecField<O, B> f2) {
            this.f1 = f1;
            this.f2 = f2;
        }

        public RecordCodecBuilder<O, O> apply(Instance<O> ignored, BiFunction<A, B, O> fn) {
            MapCodec<O> mc = MapCodec.of(
                    new MapEncoder<>() {
                        @Override
                        public <T> RecordBuilder<T> encode(O input, DynamicOps<T> ops, RecordBuilder<T> prefix) {
                            RecordBuilder<T> r = f1.codec().encode(f1.getter().apply(input), ops, prefix);
                            return f2.codec().encode(f2.getter().apply(input), ops, r);
                        }
                    },
                    new MapDecoder<>() {
                        @Override
                        public <T> DataResult<O> decode(DynamicOps<T> ops, MapLike<T> input) {
                            return f1.codec().decode(ops, input).apply2(fn, f2.codec().decode(ops, input));
                        }
                    });
            return new RecordCodecBuilder<>(mc, Function.identity());
        }
    }

    public static final class Group3<O, A, B, C> {
        private final MapCodec.RecordCodecField<O, A> f1;
        private final MapCodec.RecordCodecField<O, B> f2;
        private final MapCodec.RecordCodecField<O, C> f3;

        private Group3(
                MapCodec.RecordCodecField<O, A> f1,
                MapCodec.RecordCodecField<O, B> f2,
                MapCodec.RecordCodecField<O, C> f3) {
            this.f1 = f1;
            this.f2 = f2;
            this.f3 = f3;
        }

        public RecordCodecBuilder<O, O> apply(Instance<O> ignored, Function3<A, B, C, O> fn) {
            MapCodec<O> mc = MapCodec.of(
                    new MapEncoder<>() {
                        @Override
                        public <T> RecordBuilder<T> encode(O input, DynamicOps<T> ops, RecordBuilder<T> prefix) {
                            RecordBuilder<T> r = f1.codec().encode(f1.getter().apply(input), ops, prefix);
                            r = f2.codec().encode(f2.getter().apply(input), ops, r);
                            return f3.codec().encode(f3.getter().apply(input), ops, r);
                        }
                    },
                    new MapDecoder<>() {
                        @Override
                        public <T> DataResult<O> decode(DynamicOps<T> ops, MapLike<T> input) {
                            return f1.codec().decode(ops, input).apply3(
                                    fn, f2.codec().decode(ops, input), f3.codec().decode(ops, input));
                        }
                    });
            return new RecordCodecBuilder<>(mc, Function.identity());
        }
    }

    public static final class Group4<O, A, B, C, D> {
        private final MapCodec.RecordCodecField<O, A> f1;
        private final MapCodec.RecordCodecField<O, B> f2;
        private final MapCodec.RecordCodecField<O, C> f3;
        private final MapCodec.RecordCodecField<O, D> f4;

        private Group4(
                MapCodec.RecordCodecField<O, A> f1,
                MapCodec.RecordCodecField<O, B> f2,
                MapCodec.RecordCodecField<O, C> f3,
                MapCodec.RecordCodecField<O, D> f4) {
            this.f1 = f1;
            this.f2 = f2;
            this.f3 = f3;
            this.f4 = f4;
        }

        public RecordCodecBuilder<O, O> apply(Instance<O> ignored, Function4<A, B, C, D, O> fn) {
            MapCodec<O> mc = MapCodec.of(
                    new MapEncoder<>() {
                        @Override
                        public <T> RecordBuilder<T> encode(O input, DynamicOps<T> ops, RecordBuilder<T> prefix) {
                            RecordBuilder<T> r = f1.codec().encode(f1.getter().apply(input), ops, prefix);
                            r = f2.codec().encode(f2.getter().apply(input), ops, r);
                            r = f3.codec().encode(f3.getter().apply(input), ops, r);
                            return f4.codec().encode(f4.getter().apply(input), ops, r);
                        }
                    },
                    new MapDecoder<>() {
                        @Override
                        public <T> DataResult<O> decode(DynamicOps<T> ops, MapLike<T> input) {
                            return f1.codec().decode(ops, input).flatMap(a ->
                                    f2.codec().decode(ops, input).flatMap(b ->
                                            f3.codec().decode(ops, input).flatMap(c ->
                                                    f4.codec().decode(ops, input).map(d ->
                                                            fn.apply(a, b, c, d)))));
                        }
                    });
            return new RecordCodecBuilder<>(mc, Function.identity());
        }
    }

    public static final class Group5<O, A, B, C, D, E> {
        private final MapCodec.RecordCodecField<O, A> f1;
        private final MapCodec.RecordCodecField<O, B> f2;
        private final MapCodec.RecordCodecField<O, C> f3;
        private final MapCodec.RecordCodecField<O, D> f4;
        private final MapCodec.RecordCodecField<O, E> f5;

        private Group5(
                MapCodec.RecordCodecField<O, A> f1,
                MapCodec.RecordCodecField<O, B> f2,
                MapCodec.RecordCodecField<O, C> f3,
                MapCodec.RecordCodecField<O, D> f4,
                MapCodec.RecordCodecField<O, E> f5) {
            this.f1 = f1;
            this.f2 = f2;
            this.f3 = f3;
            this.f4 = f4;
            this.f5 = f5;
        }

        public RecordCodecBuilder<O, O> apply(Instance<O> ignored, Function5<A, B, C, D, E, O> fn) {
            MapCodec<O> mc = MapCodec.of(
                    new MapEncoder<>() {
                        @Override
                        public <T> RecordBuilder<T> encode(O input, DynamicOps<T> ops, RecordBuilder<T> prefix) {
                            RecordBuilder<T> r = f1.codec().encode(f1.getter().apply(input), ops, prefix);
                            r = f2.codec().encode(f2.getter().apply(input), ops, r);
                            r = f3.codec().encode(f3.getter().apply(input), ops, r);
                            r = f4.codec().encode(f4.getter().apply(input), ops, r);
                            return f5.codec().encode(f5.getter().apply(input), ops, r);
                        }
                    },
                    new MapDecoder<>() {
                        @Override
                        public <T> DataResult<O> decode(DynamicOps<T> ops, MapLike<T> input) {
                            return f1.codec().decode(ops, input).flatMap(a ->
                                    f2.codec().decode(ops, input).flatMap(b ->
                                            f3.codec().decode(ops, input).flatMap(c ->
                                                    f4.codec().decode(ops, input).flatMap(d ->
                                                            f5.codec().decode(ops, input).map(e ->
                                                                    fn.apply(a, b, c, d, e))))));
                        }
                    });
            return new RecordCodecBuilder<>(mc, Function.identity());
        }
    }
}
