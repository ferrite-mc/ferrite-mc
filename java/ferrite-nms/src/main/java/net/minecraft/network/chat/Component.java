// Clean-room Ferrite NMS compatibility code for Minecraft 26.3. No Mojang code or assets.
package net.minecraft.network.chat;

import com.mojang.serialization.Codec;
import java.util.Objects;

/** Clean-room immutable chat component matching 26.3 {@code net.minecraft.network.chat.Component}. */
public interface Component {
    Codec<Component> CODEC = Codec.STRING.xmap(Component::literal, Component::getString);

    String getString();

    static Component literal(String text) {
        return new LiteralComponent(Objects.requireNonNull(text, "text"));
    }

    static Component empty() {
        return LiteralComponent.EMPTY;
    }

    static Component translatable(String key, Object... args) {
        return new TranslatableComponent(Objects.requireNonNull(key, "key"), args);
    }

    final class LiteralComponent implements Component {
        static final LiteralComponent EMPTY = new LiteralComponent("");
        private final String text;

        private LiteralComponent(String text) {
            this.text = text;
        }

        @Override
        public String getString() {
            return text;
        }

        @Override
        public boolean equals(Object o) {
            return this == o || o instanceof LiteralComponent other && text.equals(other.text);
        }

        @Override
        public int hashCode() {
            return text.hashCode();
        }

        @Override
        public String toString() {
            return "literal{" + text + "}";
        }
    }

    final class TranslatableComponent implements Component {
        private final String key;
        private final Object[] args;

        private TranslatableComponent(String key, Object[] args) {
            this.key = key;
            this.args = args == null ? new Object[0] : args.clone();
        }

        public String getKey() {
            return key;
        }

        @Override
        public String getString() {
            return args.length == 0 ? key : String.format(key, args);
        }

        @Override
        public String toString() {
            return "translatable{" + key + "}";
        }
    }
}
