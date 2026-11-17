package dev.ferrite.server;

import java.io.ByteArrayInputStream;
import java.io.DataInputStream;
import java.io.IOException;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.bukkit.Material;
import org.bukkit.inventory.ItemStack;

/** Reads the host's 26.3 item save NBT without depending on NMS being on the API classpath. */
final class ItemNbt {
    private ItemNbt() {}

    static ItemStack parse(byte[] bytes) {
        try (var input = new DataInputStream(new ByteArrayInputStream(bytes))) {
            int type = input.readUnsignedByte();
            if (type != 10) throw new IllegalArgumentException("item snapshot root is not a compound");
            input.readUTF();
            var root = compound(input);
            var id = root.get("id");
            var count = root.get("count");
            if (!(id instanceof String name) || !(count instanceof Number amount)) {
                throw new IllegalArgumentException("item snapshot is missing id/count");
            }
            var material = Material.matchMaterial(name);
            if (material == null) throw new IllegalArgumentException("unknown item " + name);
            ItemStack stack = new FerriteItemStack(material, amount.intValue());
            if (root.get("components") instanceof Map<?, ?> components) {
                var values = new LinkedHashMap<String, Object>();
                components.forEach((key, value) -> values.put(String.valueOf(key), value));
                stack = ItemComponents.applyComponents(stack, values);
            }
            return stack;
        } catch (IOException error) {
            throw new IllegalArgumentException("invalid item snapshot NBT", error);
        }
    }

    private static Object tag(DataInputStream input, int type) throws IOException {
        return switch (type) {
            case 1 -> input.readByte();
            case 2 -> input.readShort();
            case 3 -> input.readInt();
            case 4 -> input.readLong();
            case 5 -> input.readFloat();
            case 6 -> input.readDouble();
            case 7 -> { int length = input.readInt(); var value = new byte[length]; input.readFully(value); yield value; }
            case 8 -> input.readUTF();
            case 9 -> {
                int element = input.readUnsignedByte();
                int length = input.readInt();
                var value = new ArrayList<Object>(length);
                for (int i = 0; i < length; i++) value.add(tag(input, element));
                yield value;
            }
            case 10 -> compound(input);
            case 11 -> { int length = input.readInt(); var value = new int[length]; for (int i = 0; i < length; i++) value[i] = input.readInt(); yield value; }
            case 12 -> { int length = input.readInt(); var value = new long[length]; for (int i = 0; i < length; i++) value[i] = input.readLong(); yield value; }
            default -> throw new IllegalArgumentException("invalid NBT tag type " + type);
        };
    }

    private static Map<String, Object> compound(DataInputStream input) throws IOException {
        var value = new LinkedHashMap<String, Object>();
        while (true) {
            int type = input.readUnsignedByte();
            if (type == 0) return value;
            value.put(input.readUTF(), tag(input, type));
        }
    }
}
