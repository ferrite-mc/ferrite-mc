package dev.ferrite.server;

/** The one implementation of {@link net.minecraft.network.chat.Component}: a thin wrapper over Adventure. */
public final class NmsComponent implements net.minecraft.network.chat.Component {
    private final net.kyori.adventure.text.Component value;
    private NmsComponent(net.kyori.adventure.text.Component value) { this.value = java.util.Objects.requireNonNull(value, "value"); }
    public static net.minecraft.network.chat.Component of(net.kyori.adventure.text.Component value) { return new NmsComponent(value); }
    @Override public String getString() { return net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(value); }
    @Override public net.kyori.adventure.text.Component adventure() { return value; }
    @Override public boolean equals(Object other) { return other instanceof NmsComponent c && c.value.equals(value); }
    @Override public int hashCode() { return value.hashCode(); }
    @Override public String toString() { return getString(); }
}
