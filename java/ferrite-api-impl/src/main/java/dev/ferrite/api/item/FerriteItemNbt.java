package dev.ferrite.api.item;

import io.papermc.paper.registry.RegistryAccess;
import io.papermc.paper.registry.RegistryKey;
import java.io.ByteArrayInputStream;
import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.io.UncheckedIOException;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.legacy.LegacyComponentSerializer;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.enchantments.Enchantment;
import org.bukkit.inventory.ItemRarity;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.inventory.meta.ItemMeta;
import org.bukkit.inventory.meta.Repairable;
import org.bukkit.persistence.PersistentDataContainer;
import org.bukkit.persistence.PersistentDataType;

/**
 * Clean-room 26.3 named binary NBT encoder/decoder for {@link ItemStack} snapshots over the
 * host v2 {@code nbt_base64} slot field.
 */
public final class FerriteItemNbt {

    private static final byte TAG_END = 0;
    private static final byte TAG_BYTE = 1;
    private static final byte TAG_SHORT = 2;
    private static final byte TAG_INT = 3;
    private static final byte TAG_LONG = 4;
    private static final byte TAG_FLOAT = 5;
    private static final byte TAG_DOUBLE = 6;
    private static final byte TAG_BYTE_ARRAY = 7;
    private static final byte TAG_STRING = 8;
    private static final byte TAG_LIST = 9;
    private static final byte TAG_COMPOUND = 10;
    private static final byte TAG_INT_ARRAY = 11;
    private static final byte TAG_LONG_ARRAY = 12;

    private FerriteItemNbt() {}

    /**
     * Encodes {@code stack}'s 26.3 item save NBT (`id`, `count`, `components`) as Base64, or
     * returns {@code null} when the stack is empty or has no non-default metadata.
     */
    public static String encodeItemBase64(ItemStack stack) {
        if (stack == null || stack.getType() == Material.AIR || stack.getAmount() <= 0 || !stack.hasItemMeta()) {
            return null;
        }
        ItemMeta meta = stack.getItemMeta();
        if (meta == null) {
            return null;
        }
        Map<String, NbtTag> components = encodeComponents(meta);
        if (components.isEmpty()) {
            return null;
        }
        Map<String, NbtTag> root = new LinkedHashMap<>();
        root.put("id", new NbtString(stack.getType().getKey().toString()));
        root.put("count", new NbtInt(stack.getAmount()));
        root.put("components", new NbtCompound(components));

        try {
            ByteArrayOutputStream baos = new ByteArrayOutputStream();
            DataOutputStream out = new DataOutputStream(baos);
            out.writeByte(TAG_COMPOUND);
            writeUtf(out, "");
            writeCompoundBody(out, root);
            out.flush();
            return Base64.getEncoder().encodeToString(baos.toByteArray());
        } catch (IOException error) {
            throw new UncheckedIOException(error);
        }
    }

    /**
     * Decodes a Base64 named binary NBT {@code nbt_base64} payload into an {@link ItemStack}.
     * Throws {@link IllegalArgumentException} on malformed Base64, corrupt NBT, or unknown item id.
     */
    public static ItemStack decodeItemBase64(String nbtBase64, Material fallbackMaterial, int count) {
        Objects.requireNonNull(nbtBase64, "nbtBase64");
        byte[] bytes;
        try {
            bytes = Base64.getDecoder().decode(nbtBase64);
        } catch (IllegalArgumentException invalidBase64) {
            throw new IllegalArgumentException("invalid nbt_base64", invalidBase64);
        }

        Map<String, NbtTag> root;
        try {
            DataInputStream in = new DataInputStream(new ByteArrayInputStream(bytes));
            byte rootType = in.readByte();
            if (rootType != TAG_COMPOUND) {
                throw new IllegalArgumentException("item nbt_base64 root tag must be a compound");
            }
            readUtf(in);
            root = readCompoundBody(in);
        } catch (IOException error) {
            throw new IllegalArgumentException("invalid binary nbt: " + error.getMessage(), error);
        }

        Material material = fallbackMaterial;
        if (root.get("id") instanceof NbtString idTag) {
            Material resolved = Material.matchMaterial(idTag.value());
            if (resolved == null) {
                throw new IllegalArgumentException("unknown item id in nbt: " + idTag.value());
            }
            material = resolved;
        }
        if (material == null || material == Material.AIR || count <= 0) {
            return null;
        }

        ItemStack stack = new ItemStack(material, count);
        if (root.get("components") instanceof NbtCompound components) {
            applyComponents(stack, components.entries());
        }
        return stack;
    }

    private static Map<String, NbtTag> encodeComponents(ItemMeta meta) {
        Map<String, NbtTag> components = new LinkedHashMap<>();
        if (meta.hasDisplayName()) {
            Component display = meta.displayName();
            String text = display != null
                    ? LegacyComponentSerializer.legacySection().serialize(display)
                    : meta.getDisplayName();
            components.put("minecraft:custom_name", new NbtString(text));
        }
        if (meta.hasItemName()) {
            Component itemName = meta.itemName();
            String text = itemName != null
                    ? LegacyComponentSerializer.legacySection().serialize(itemName)
                    : meta.getItemName();
            components.put("minecraft:item_name", new NbtString(text));
        }
        if (meta.hasLore()) {
            List<Component> lore = meta.lore();
            List<NbtTag> lines = new ArrayList<>();
            if (lore != null) {
                for (Component line : lore) {
                    lines.add(new NbtString(LegacyComponentSerializer.legacySection().serialize(line)));
                }
            }
            if (!lines.isEmpty()) {
                components.put("minecraft:lore", new NbtList(TAG_STRING, lines));
            }
        }
        if (meta instanceof Damageable damageable && damageable.hasDamage()) {
            components.put("minecraft:damage", new NbtInt(damageable.getDamage()));
        }
        if (meta instanceof Damageable damageable && damageable.hasMaxDamage()) {
            components.put("minecraft:max_damage", new NbtInt(damageable.getMaxDamage()));
        }
        if (meta instanceof Repairable repairable && repairable.hasRepairCost()) {
            components.put("minecraft:repair_cost", new NbtInt(repairable.getRepairCost()));
        }
        if (meta.isUnbreakable()) {
            components.put("minecraft:unbreakable", new NbtCompound(Map.of()));
        }
        if (meta.isGlider()) {
            components.put("minecraft:glider", new NbtCompound(Map.of()));
        }
        if (meta.hasRarity()) {
            components.put(
                    "minecraft:rarity",
                    new NbtString(meta.getRarity().name().toLowerCase(Locale.ROOT)));
        }
        if (meta.hasEnchantmentGlintOverride()) {
            components.put(
                    "minecraft:enchantment_glint_override",
                    new NbtByte((byte) (Boolean.TRUE.equals(meta.getEnchantmentGlintOverride()) ? 1 : 0)));
        }
        if (meta.hasEnchants()) {
            Map<String, NbtTag> enchants = new LinkedHashMap<>();
            for (Map.Entry<Enchantment, Integer> entry : meta.getEnchants().entrySet()) {
                enchants.put(entry.getKey().getKey().toString(), new NbtInt(entry.getValue()));
            }
            components.put("minecraft:enchantments", new NbtCompound(enchants));
        }
        if (meta.hasCustomModelData()) {
            components.put(
                    "minecraft:custom_model_data",
                    new NbtCompound(Map.of(
                            "floats",
                            new NbtList(TAG_FLOAT, List.of(new NbtFloat((float) meta.getCustomModelData()))))));
        }
        if (meta.isHideTooltip()) {
            components.put("minecraft:hide_tooltip", new NbtCompound(Map.of()));
        }
        PersistentDataContainer pdc = meta.getPersistentDataContainer();
        if (!pdc.isEmpty()) {
            Map<String, NbtTag> customData = encodePdc(pdc);
            if (!customData.isEmpty()) {
                components.put("minecraft:custom_data", new NbtCompound(customData));
            }
        }
        return components;
    }

    private static void applyComponents(ItemStack stack, Map<String, NbtTag> components) {
        ItemMeta meta = stack.getItemMeta();
        if (meta == null) {
            return;
        }
        if (components.get("minecraft:custom_name") instanceof NbtString nameTag) {
            meta.displayName(LegacyComponentSerializer.legacySection().deserialize(nameTag.value()));
        }
        if (components.get("minecraft:item_name") instanceof NbtString itemNameTag) {
            meta.itemName(LegacyComponentSerializer.legacySection().deserialize(itemNameTag.value()));
        }
        if (components.get("minecraft:lore") instanceof NbtList loreTag) {
            List<Component> lines = new ArrayList<>();
            for (NbtTag entry : loreTag.elements()) {
                if (entry instanceof NbtString line) {
                    lines.add(LegacyComponentSerializer.legacySection().deserialize(line.value()));
                }
            }
            meta.lore(lines);
        }
        if (components.get("minecraft:damage") instanceof NbtInt dmg && meta instanceof Damageable damageable) {
            damageable.setDamage(dmg.value());
        }
        if (components.get("minecraft:max_damage") instanceof NbtInt maxDmg && meta instanceof Damageable damageable) {
            damageable.setMaxDamage(maxDmg.value());
        }
        if (components.get("minecraft:repair_cost") instanceof NbtInt repair && meta instanceof Repairable repairable) {
            repairable.setRepairCost(repair.value());
        }
        if (components.containsKey("minecraft:unbreakable")) {
            meta.setUnbreakable(true);
        }
        if (components.containsKey("minecraft:glider")) {
            meta.setGlider(true);
        }
        if (components.get("minecraft:rarity") instanceof NbtString rarityTag) {
            meta.setRarity(ItemRarity.valueOf(rarityTag.value().toUpperCase(Locale.ROOT)));
        }
        if (components.get("minecraft:enchantment_glint_override") instanceof NbtByte glint) {
            meta.setEnchantmentGlintOverride(glint.value() != 0);
        }
        if (components.get("minecraft:enchantments") instanceof NbtCompound enchants) {
            var registry = RegistryAccess.registryAccess().getRegistry(RegistryKey.ENCHANTMENT);
            for (Map.Entry<String, NbtTag> entry : enchants.entries().entrySet()) {
                if (entry.getValue() instanceof NbtInt level) {
                    NamespacedKey key = NamespacedKey.fromString(entry.getKey());
                    Enchantment enchant = key == null ? null : registry.get(key);
                    if (enchant != null) {
                        meta.addEnchant(enchant, level.value(), true);
                    }
                }
            }
        }
        if (components.get("minecraft:custom_model_data") instanceof NbtCompound cmd
                && cmd.entries().get("floats") instanceof NbtList floats
                && !floats.elements().isEmpty()
                && floats.elements().get(0) instanceof NbtFloat first) {
            meta.setCustomModelData(Math.round(first.value()));
        }
        if (components.containsKey("minecraft:hide_tooltip")) {
            meta.setHideTooltip(true);
        }
        if (components.get("minecraft:custom_data") instanceof NbtCompound customData) {
            decodePdc(customData.entries(), meta.getPersistentDataContainer());
        }
        stack.setItemMeta(meta);
    }

    private static Map<String, NbtTag> encodePdc(PersistentDataContainer pdc) {
        Map<String, NbtTag> out = new LinkedHashMap<>();
        for (NamespacedKey key : pdc.getKeys()) {
            NbtTag tag = encodePdcKey(pdc, key);
            if (tag != null) {
                out.put(key.toString(), tag);
            }
        }
        return out;
    }

    private static NbtTag encodePdcKey(PersistentDataContainer pdc, NamespacedKey key) {
        if (pdc.has(key, PersistentDataType.BYTE)) {
            return new NbtByte(pdc.get(key, PersistentDataType.BYTE));
        }
        if (pdc.has(key, PersistentDataType.SHORT)) {
            return new NbtShort(pdc.get(key, PersistentDataType.SHORT));
        }
        if (pdc.has(key, PersistentDataType.INTEGER)) {
            return new NbtInt(pdc.get(key, PersistentDataType.INTEGER));
        }
        if (pdc.has(key, PersistentDataType.LONG)) {
            return new NbtLong(pdc.get(key, PersistentDataType.LONG));
        }
        if (pdc.has(key, PersistentDataType.FLOAT)) {
            return new NbtFloat(pdc.get(key, PersistentDataType.FLOAT));
        }
        if (pdc.has(key, PersistentDataType.DOUBLE)) {
            return new NbtDouble(pdc.get(key, PersistentDataType.DOUBLE));
        }
        if (pdc.has(key, PersistentDataType.BOOLEAN)) {
            return new NbtByte((byte) (Boolean.TRUE.equals(pdc.get(key, PersistentDataType.BOOLEAN)) ? 1 : 0));
        }
        if (pdc.has(key, PersistentDataType.STRING)) {
            return new NbtString(pdc.get(key, PersistentDataType.STRING));
        }
        if (pdc.has(key, PersistentDataType.BYTE_ARRAY)) {
            return new NbtByteArray(pdc.get(key, PersistentDataType.BYTE_ARRAY));
        }
        if (pdc.has(key, PersistentDataType.INTEGER_ARRAY)) {
            return new NbtIntArray(pdc.get(key, PersistentDataType.INTEGER_ARRAY));
        }
        if (pdc.has(key, PersistentDataType.LONG_ARRAY)) {
            return new NbtLongArray(pdc.get(key, PersistentDataType.LONG_ARRAY));
        }
        if (pdc.has(key, PersistentDataType.TAG_CONTAINER)) {
            PersistentDataContainer nested = pdc.get(key, PersistentDataType.TAG_CONTAINER);
            return nested == null ? null : new NbtCompound(encodePdc(nested));
        }
        return null;
    }

    private static void decodePdc(Map<String, NbtTag> entries, PersistentDataContainer pdc) {
        for (Map.Entry<String, NbtTag> entry : entries.entrySet()) {
            NamespacedKey key = NamespacedKey.fromString(entry.getKey());
            if (key == null) {
                continue;
            }
            switch (entry.getValue()) {
                case NbtByte b -> pdc.set(key, PersistentDataType.BYTE, b.value());
                case NbtShort s -> pdc.set(key, PersistentDataType.SHORT, s.value());
                case NbtInt i -> pdc.set(key, PersistentDataType.INTEGER, i.value());
                case NbtLong l -> pdc.set(key, PersistentDataType.LONG, l.value());
                case NbtFloat f -> pdc.set(key, PersistentDataType.FLOAT, f.value());
                case NbtDouble d -> pdc.set(key, PersistentDataType.DOUBLE, d.value());
                case NbtString str -> pdc.set(key, PersistentDataType.STRING, str.value());
                case NbtByteArray ba -> pdc.set(key, PersistentDataType.BYTE_ARRAY, ba.value());
                case NbtIntArray ia -> pdc.set(key, PersistentDataType.INTEGER_ARRAY, ia.value());
                case NbtLongArray la -> pdc.set(key, PersistentDataType.LONG_ARRAY, la.value());
                case NbtCompound nested -> {
                    PersistentDataContainer child = pdc.getAdapterContext().newPersistentDataContainer();
                    decodePdc(nested.entries(), child);
                    pdc.set(key, PersistentDataType.TAG_CONTAINER, child);
                }
                case NbtList ignored -> {}
            }
        }
    }

    private static void writeCompoundBody(DataOutputStream out, Map<String, NbtTag> entries)
            throws IOException {
        for (Map.Entry<String, NbtTag> entry : entries.entrySet()) {
            out.writeByte(entry.getValue().typeId());
            writeUtf(out, entry.getKey());
            entry.getValue().write(out);
        }
        out.writeByte(TAG_END);
    }

    private static Map<String, NbtTag> readCompoundBody(DataInputStream in) throws IOException {
        Map<String, NbtTag> entries = new LinkedHashMap<>();
        while (true) {
            byte type = in.readByte();
            if (type == TAG_END) {
                return entries;
            }
            String key = readUtf(in);
            entries.put(key, readTagPayload(type, in));
        }
    }

    private static NbtTag readTagPayload(byte type, DataInputStream in) throws IOException {
        return switch (type) {
            case TAG_BYTE -> new NbtByte(in.readByte());
            case TAG_SHORT -> new NbtShort(in.readShort());
            case TAG_INT -> new NbtInt(in.readInt());
            case TAG_LONG -> new NbtLong(in.readLong());
            case TAG_FLOAT -> new NbtFloat(in.readFloat());
            case TAG_DOUBLE -> new NbtDouble(in.readDouble());
            case TAG_BYTE_ARRAY -> {
                int len = in.readInt();
                byte[] arr = new byte[len];
                in.readFully(arr);
                yield new NbtByteArray(arr);
            }
            case TAG_STRING -> new NbtString(readUtf(in));
            case TAG_LIST -> {
                byte elemType = in.readByte();
                int len = in.readInt();
                List<NbtTag> list = new ArrayList<>(Math.max(0, len));
                for (int i = 0; i < len; i++) {
                    list.add(readTagPayload(elemType, in));
                }
                yield new NbtList(elemType, list);
            }
            case TAG_COMPOUND -> new NbtCompound(readCompoundBody(in));
            case TAG_INT_ARRAY -> {
                int len = in.readInt();
                int[] arr = new int[len];
                for (int i = 0; i < len; i++) {
                    arr[i] = in.readInt();
                }
                yield new NbtIntArray(arr);
            }
            case TAG_LONG_ARRAY -> {
                int len = in.readInt();
                long[] arr = new long[len];
                for (int i = 0; i < len; i++) {
                    arr[i] = in.readLong();
                }
                yield new NbtLongArray(arr);
            }
            default -> throw new IOException("unknown NBT tag type " + type);
        };
    }

    private static void writeUtf(DataOutputStream out, String value) throws IOException {
        byte[] bytes = value.getBytes(StandardCharsets.UTF_8);
        out.writeShort(bytes.length);
        out.write(bytes);
    }

    private static String readUtf(DataInputStream in) throws IOException {
        int len = in.readUnsignedShort();
        byte[] bytes = new byte[len];
        in.readFully(bytes);
        return new String(bytes, StandardCharsets.UTF_8);
    }

    private sealed interface NbtTag
            permits NbtByte, NbtShort, NbtInt, NbtLong, NbtFloat, NbtDouble,
                    NbtByteArray, NbtString, NbtList, NbtCompound, NbtIntArray, NbtLongArray {
        byte typeId();

        void write(DataOutputStream out) throws IOException;
    }

    private record NbtByte(byte value) implements NbtTag {
        @Override
        public byte typeId() {
            return TAG_BYTE;
        }

        @Override
        public void write(DataOutputStream out) throws IOException {
            out.writeByte(value);
        }
    }

    private record NbtShort(short value) implements NbtTag {
        @Override
        public byte typeId() {
            return TAG_SHORT;
        }

        @Override
        public void write(DataOutputStream out) throws IOException {
            out.writeShort(value);
        }
    }

    private record NbtInt(int value) implements NbtTag {
        @Override
        public byte typeId() {
            return TAG_INT;
        }

        @Override
        public void write(DataOutputStream out) throws IOException {
            out.writeInt(value);
        }
    }

    private record NbtLong(long value) implements NbtTag {
        @Override
        public byte typeId() {
            return TAG_LONG;
        }

        @Override
        public void write(DataOutputStream out) throws IOException {
            out.writeLong(value);
        }
    }

    private record NbtFloat(float value) implements NbtTag {
        @Override
        public byte typeId() {
            return TAG_FLOAT;
        }

        @Override
        public void write(DataOutputStream out) throws IOException {
            out.writeFloat(value);
        }
    }

    private record NbtDouble(double value) implements NbtTag {
        @Override
        public byte typeId() {
            return TAG_DOUBLE;
        }

        @Override
        public void write(DataOutputStream out) throws IOException {
            out.writeDouble(value);
        }
    }

    private record NbtByteArray(byte[] value) implements NbtTag {
        @Override
        public byte typeId() {
            return TAG_BYTE_ARRAY;
        }

        @Override
        public void write(DataOutputStream out) throws IOException {
            out.writeInt(value.length);
            out.write(value);
        }
    }

    private record NbtString(String value) implements NbtTag {
        @Override
        public byte typeId() {
            return TAG_STRING;
        }

        @Override
        public void write(DataOutputStream out) throws IOException {
            writeUtf(out, value);
        }
    }

    private record NbtList(byte elementType, List<NbtTag> elements) implements NbtTag {
        @Override
        public byte typeId() {
            return TAG_LIST;
        }

        @Override
        public void write(DataOutputStream out) throws IOException {
            out.writeByte(elementType);
            out.writeInt(elements.size());
            for (NbtTag element : elements) {
                element.write(out);
            }
        }
    }

    private record NbtCompound(Map<String, NbtTag> entries) implements NbtTag {
        @Override
        public byte typeId() {
            return TAG_COMPOUND;
        }

        @Override
        public void write(DataOutputStream out) throws IOException {
            writeCompoundBody(out, entries);
        }
    }

    private record NbtIntArray(int[] value) implements NbtTag {
        @Override
        public byte typeId() {
            return TAG_INT_ARRAY;
        }

        @Override
        public void write(DataOutputStream out) throws IOException {
            out.writeInt(value.length);
            for (int v : value) {
                out.writeInt(v);
            }
        }
    }

    private record NbtLongArray(long[] value) implements NbtTag {
        @Override
        public byte typeId() {
            return TAG_LONG_ARRAY;
        }

        @Override
        public void write(DataOutputStream out) throws IOException {
            out.writeInt(value.length);
            for (long v : value) {
                out.writeLong(v);
            }
        }
    }
}
