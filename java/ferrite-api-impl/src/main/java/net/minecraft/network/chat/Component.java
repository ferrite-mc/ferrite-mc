package net.minecraft.network.chat;

/** Text component. Backed by an Adventure component; shim of the Mojang-mapped interface. */
public interface Component {
    /** Plain text with formatting removed. */
    String getString();
    /** The Adventure form of this component. */
    net.kyori.adventure.text.Component adventure();

    static Component literal(String text) { return dev.ferrite.server.NmsComponent.of(net.kyori.adventure.text.Component.text(text)); }
    static Component translatable(String key) { return dev.ferrite.server.NmsComponent.of(net.kyori.adventure.text.Component.translatable(key)); }
    static Component translatable(String key, Object... args) {
        java.util.List<net.kyori.adventure.text.Component> list = new java.util.ArrayList<>();
        for (Object arg : args) list.add(arg instanceof Component c ? c.adventure() : net.kyori.adventure.text.Component.text(String.valueOf(arg)));
        return dev.ferrite.server.NmsComponent.of(net.kyori.adventure.text.Component.translatable(key, list));
    }
    static Component empty() { return dev.ferrite.server.NmsComponent.of(net.kyori.adventure.text.Component.empty()); }
    static Component nullToEmpty(String text) { return text == null ? empty() : literal(text); }
}
