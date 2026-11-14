package io.papermc.paper.command.brigadier.argument;

import com.mojang.brigadier.StringReader;
import com.mojang.brigadier.arguments.ArgumentType;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import com.mojang.brigadier.exceptions.SimpleCommandExceptionType;

/**
 * Paper's vanilla argument types. Ferrite has no Brigadier dispatcher or NMS argument parsers, so
 * only the types that map onto plain Bukkit values are implemented (item stack, keys, uuid, game
 * mode, world, text colours, time); the rest refuse with the type's name instead of mis-parsing.
 */
public final class FerriteArgumentProvider implements VanillaArgumentProvider {
    private interface Parser<T> { T parse(StringReader reader) throws CommandSyntaxException; }

    private static <T> ArgumentType<T> of(Parser<T> parser) { return parser::parse; }

    private static CommandSyntaxException error(StringReader reader, String message) {
        return new SimpleCommandExceptionType(() -> message).createWithContext(reader);
    }

    private static UnsupportedOperationException unsupported(String type) {
        return new UnsupportedOperationException("Ferrite: ArgumentTypes#" + type + " is not implemented: it needs the vanilla command parser, which Ferrite does not embed");
    }

    private static String word(StringReader reader) {
        int start = reader.getCursor();
        while (reader.canRead() && !Character.isWhitespace(reader.peek())) reader.skip();
        return reader.getString().substring(start, reader.getCursor());
    }

    /** {@code id} or {@code id[component data]}; the brackets may nest and contain quoted text. */
    private static String itemText(StringReader reader) {
        int start = reader.getCursor();
        while (reader.canRead() && (Character.isLetterOrDigit(reader.peek()) || "_:./-".indexOf(reader.peek()) >= 0)) reader.skip();
        if (reader.canRead() && reader.peek() == '[') {
            int depth = 0;
            char quote = 0;
            while (reader.canRead()) {
                char c = reader.read();
                if (quote != 0) { if (c == '\\' && reader.canRead()) reader.skip(); else if (c == quote) quote = 0; continue; }
                if (c == '"' || c == '\'') quote = c;
                else if (c == '[') depth++;
                else if (c == ']' && --depth == 0) break;
            }
        }
        return reader.getString().substring(start, reader.getCursor());
    }

    @Override public ArgumentType<org.bukkit.inventory.ItemStack> itemStack() {
        return of(reader -> {
            String text = itemText(reader);
            int bracket = text.indexOf('[');
            String id = bracket < 0 ? text : text.substring(0, bracket);
            var material = org.bukkit.Material.matchMaterial(id);
            if (material == null || !material.isItem()) throw error(reader, "Unknown item '" + id + "'");
            var stack = new org.bukkit.inventory.ItemStack(material);
            return bracket < 0 ? stack : org.bukkit.Bukkit.getUnsafe().modifyItemStack(stack, text.substring(bracket));
        });
    }

    @Override public ArgumentType<net.kyori.adventure.key.Key> key() {
        return of(reader -> {
            String text = word(reader);
            if (!net.kyori.adventure.key.Key.parseable(text)) throw error(reader, "Invalid key '" + text + "'");
            return net.kyori.adventure.key.Key.key(text);
        });
    }

    @Override public ArgumentType<org.bukkit.NamespacedKey> namespacedKey() {
        return of(reader -> {
            String text = word(reader);
            var key = org.bukkit.NamespacedKey.fromString(text);
            if (key == null) throw error(reader, "Invalid key '" + text + "'");
            return key;
        });
    }

    @Override public ArgumentType<java.util.UUID> uuid() {
        return of(reader -> {
            String text = word(reader);
            try { return java.util.UUID.fromString(text); }
            catch (IllegalArgumentException invalid) { throw error(reader, "Invalid UUID '" + text + "'"); }
        });
    }

    @Override public ArgumentType<org.bukkit.GameMode> gameMode() {
        return of(reader -> {
            String text = word(reader);
            for (var mode : org.bukkit.GameMode.values()) if (mode.name().equalsIgnoreCase(text)) return mode;
            throw error(reader, "Invalid game mode '" + text + "'");
        });
    }

    @Override public ArgumentType<org.bukkit.World> world() {
        return of(reader -> {
            String text = word(reader);
            var world = org.bukkit.Bukkit.getWorld(text);
            if (world == null) throw error(reader, "Unknown world '" + text + "'");
            return world;
        });
    }

    @Override public ArgumentType<net.kyori.adventure.text.format.NamedTextColor> namedColor() {
        return of(reader -> {
            String text = word(reader);
            var color = net.kyori.adventure.text.format.NamedTextColor.NAMES.value(text.toLowerCase(java.util.Locale.ROOT));
            if (color == null) throw error(reader, "Unknown colour '" + text + "'");
            return color;
        });
    }

    @Override public ArgumentType<net.kyori.adventure.text.format.TextColor> hexColor() {
        return of(reader -> {
            String text = word(reader);
            var color = net.kyori.adventure.text.format.TextColor.fromCSSHexString(text);
            if (color == null) throw error(reader, "Invalid colour '" + text + "'");
            return color;
        });
    }

    /** A duration such as {@code 10}, {@code 5s}, {@code 2d} or {@code 1t}, in ticks. */
    @Override public ArgumentType<Integer> time(int minimum) {
        return of(reader -> {
            String text = word(reader);
            if (text.isEmpty()) throw error(reader, "Expected a time");
            int unit = 1;
            char last = text.charAt(text.length() - 1);
            String number = text;
            switch (last) {
                case 'd' -> { unit = 24000; number = text.substring(0, text.length() - 1); }
                case 's' -> { unit = 20; number = text.substring(0, text.length() - 1); }
                case 't' -> number = text.substring(0, text.length() - 1);
                default -> { }
            }
            float value;
            try { value = Float.parseFloat(number); } catch (NumberFormatException invalid) { throw error(reader, "Invalid time '" + text + "'"); }
            int ticks = Math.round(value * unit);
            if (ticks < minimum) throw error(reader, "Time must not be before " + minimum);
            return ticks;
        });
    }

    @Override public <T> ArgumentType<io.papermc.paper.registry.TypedKey<T>> resourceKey(io.papermc.paper.registry.RegistryKey<T> registryKey) {
        return of(reader -> {
            String text = word(reader);
            if (!net.kyori.adventure.key.Key.parseable(text)) throw error(reader, "Invalid key '" + text + "'");
            return io.papermc.paper.registry.TypedKey.create(registryKey, net.kyori.adventure.key.Key.key(text));
        });
    }

    @Override public <T> ArgumentType<T> resource(io.papermc.paper.registry.RegistryKey<T> registryKey) {
        return of(reader -> {
            String text = word(reader);
            if (!net.kyori.adventure.key.Key.parseable(text)) throw error(reader, "Invalid key '" + text + "'");
            @SuppressWarnings({"unchecked", "rawtypes"})
            Object found = io.papermc.paper.registry.RegistryAccess.registryAccess().getRegistry((io.papermc.paper.registry.RegistryKey) registryKey).get(net.kyori.adventure.key.Key.key(text));
            @SuppressWarnings("unchecked") T value = (T) found;
            if (value == null) throw error(reader, "Unknown " + registryKey.key().asString() + " '" + text + "'");
            return value;
        });
    }

    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.resolvers.selector.EntitySelectorArgumentResolver> entity() { throw unsupported("entity"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.resolvers.selector.PlayerSelectorArgumentResolver> player() { throw unsupported("player"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.resolvers.selector.EntitySelectorArgumentResolver> entities() { throw unsupported("entities"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.resolvers.selector.PlayerSelectorArgumentResolver> players() { throw unsupported("players"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.resolvers.PlayerProfileListResolver> playerProfiles() { throw unsupported("playerProfiles"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.resolvers.BlockPositionResolver> blockPosition() { throw unsupported("blockPosition"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.resolvers.ColumnBlockPositionResolver> columnBlockPosition() { throw unsupported("columnBlockPosition"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.resolvers.FinePositionResolver> finePosition(boolean centerIntegers) { throw unsupported("finePosition"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.resolvers.ColumnFinePositionResolver> columnFinePosition(boolean centerIntegers) { throw unsupported("columnFinePosition"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.resolvers.RotationResolver> rotation() { throw unsupported("rotation"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.resolvers.AngleResolver> angle() { throw unsupported("angle"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.AxisSet> axes() { throw unsupported("axes"); }
    @Override public ArgumentType<org.bukkit.block.BlockState> blockState() { throw unsupported("blockState"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.predicate.BlockInWorldPredicate> blockInWorldPredicate() { throw unsupported("blockInWorldPredicate"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.predicate.ItemStackPredicate> itemStackPredicate() { throw unsupported("itemStackPredicate"); }
    @Override public ArgumentType<net.kyori.adventure.text.Component> component() { throw unsupported("component"); }
    @Override public ArgumentType<net.kyori.adventure.text.format.Style> style() { throw unsupported("style"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.SignedMessageResolver> signedMessage() { throw unsupported("signedMessage"); }
    @Override public ArgumentType<org.bukkit.scoreboard.DisplaySlot> scoreboardDisplaySlot() { throw unsupported("scoreboardDisplaySlot"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.range.IntegerRangeProvider> integerRange() { throw unsupported("integerRange"); }
    @Override public ArgumentType<io.papermc.paper.command.brigadier.argument.range.DoubleRangeProvider> doubleRange() { throw unsupported("doubleRange"); }
    @Override public ArgumentType<org.bukkit.HeightMap> heightMap() { throw unsupported("heightMap"); }
    @Override public ArgumentType<org.bukkit.scoreboard.Criteria> objectiveCriteria() { throw unsupported("objectiveCriteria"); }
    @Override public ArgumentType<io.papermc.paper.entity.LookAnchor> entityAnchor() { throw unsupported("entityAnchor"); }
    @Override public ArgumentType<org.bukkit.block.structure.Mirror> templateMirror() { throw unsupported("templateMirror"); }
    @Override public ArgumentType<org.bukkit.block.structure.StructureRotation> templateRotation() { throw unsupported("templateRotation"); }
}
