package dev.ferrite.api.entity;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import dev.ferrite.api.internal.Unsupported;
import dev.ferrite.api.item.FerriteItemNbt;
import dev.ferrite.host.Bootstrap;
import java.util.EnumMap;
import java.util.Map;
import java.util.Objects;
import java.util.UUID;
import org.bukkit.Material;
import org.bukkit.entity.Entity;
import org.bukkit.inventory.EntityEquipment;
import org.bukkit.inventory.EquipmentSlot;
import org.bukkit.inventory.ItemStack;

/**
 * Six-slot {@link EntityEquipment} facade for non-player living entities.
 *
 * <p>Reads return defensive copies of the snapshot slots; writes update the snapshot and route
 * asynchronously to the entity's owning region via {@code entity.equipment_set_async} when the
 * embedded host is attached.
 */
public final class FerriteEntityEquipment implements EntityEquipment {

    public static final float DEFAULT_DROP_CHANCE = 0.085f;

    private final Entity holder;
    private final UUID entityUuid;
    private final EnumMap<EquipmentSlot, ItemStack> slots = new EnumMap<>(EquipmentSlot.class);
    private final EnumMap<EquipmentSlot, Float> dropChances = new EnumMap<>(EquipmentSlot.class);

    public FerriteEntityEquipment(
            Entity holder, UUID entityUuid, Map<EquipmentSlot, ItemStack> initial) {
        this(holder, entityUuid, initial, Map.of());
    }

    public FerriteEntityEquipment(
            Entity holder,
            UUID entityUuid,
            Map<EquipmentSlot, ItemStack> initial,
            Map<EquipmentSlot, Float> initialDropChances) {
        this.holder = Objects.requireNonNull(holder, "holder");
        this.entityUuid = Objects.requireNonNull(entityUuid, "entityUuid");
        for (EquipmentSlot slot : EquipmentSlot.values()) {
            if (isSupportedSlot(slot)) {
                dropChances.put(slot, DEFAULT_DROP_CHANCE);
            }
        }
        if (initial != null) {
            for (var entry : initial.entrySet()) {
                if (isSupportedSlot(entry.getKey()) && entry.getValue() != null && !entry.getValue().getType().isAir()) {
                    slots.put(entry.getKey(), entry.getValue().clone());
                }
            }
        }
        if (initialDropChances != null) {
            for (var entry : initialDropChances.entrySet()) {
                if (isSupportedSlot(entry.getKey())
                        && entry.getValue() != null
                        && Float.isFinite(entry.getValue())) {
                    dropChances.put(entry.getKey(), entry.getValue());
                }
            }
        }
    }

    private static boolean isSupportedSlot(EquipmentSlot slot) {
        return switch (slot) {
            case HAND, OFF_HAND, FEET, LEGS, CHEST, HEAD, BODY, SADDLE -> true;
            default -> false;
        };
    }

    @Override
    public void setItem(EquipmentSlot slot, ItemStack item) {
        setItem(slot, item, false);
    }

    @Override
    public void setItem(EquipmentSlot slot, ItemStack item, boolean silent) {
        Objects.requireNonNull(slot, "slot");
        if (!isSupportedSlot(slot)) {
            throw Unsupported.exception("EntityEquipment#setItem(" + slot + ")");
        }
        ItemStack normalized = (item == null || item.getType().isAir() || item.getAmount() <= 0)
                ? null
                : item.clone();
        routeHostEquipmentWrite(slot, normalized);
        if (normalized == null) {
            slots.remove(slot);
        } else {
            slots.put(slot, normalized);
        }
    }

    @Override
    public ItemStack getItem(EquipmentSlot slot) {
        Objects.requireNonNull(slot, "slot");
        if (!isSupportedSlot(slot)) {
            throw Unsupported.exception("EntityEquipment#getItem(" + slot + ")");
        }
        ItemStack stored = slots.get(slot);
        return stored == null ? null : stored.clone();
    }

    @Override
    public ItemStack getItemInMainHand() {
        ItemStack item = getItem(EquipmentSlot.HAND);
        return item != null ? item : new ItemStack(Material.AIR);
    }

    @Override
    public void setItemInMainHand(ItemStack item) {
        setItem(EquipmentSlot.HAND, item);
    }

    @Override
    public void setItemInMainHand(ItemStack item, boolean silent) {
        setItem(EquipmentSlot.HAND, item, silent);
    }

    @Override
    public ItemStack getItemInOffHand() {
        ItemStack item = getItem(EquipmentSlot.OFF_HAND);
        return item != null ? item : new ItemStack(Material.AIR);
    }

    @Override
    public void setItemInOffHand(ItemStack item) {
        setItem(EquipmentSlot.OFF_HAND, item);
    }

    @Override
    public void setItemInOffHand(ItemStack item, boolean silent) {
        setItem(EquipmentSlot.OFF_HAND, item, silent);
    }

    @Override
    @Deprecated
    public ItemStack getItemInHand() {
        return getItemInMainHand();
    }

    @Override
    @Deprecated
    public void setItemInHand(ItemStack stack) {
        setItemInMainHand(stack);
    }

    @Override
    public ItemStack getHelmet() {
        return getItem(EquipmentSlot.HEAD);
    }

    @Override
    public void setHelmet(ItemStack helmet) {
        setItem(EquipmentSlot.HEAD, helmet);
    }

    @Override
    public void setHelmet(ItemStack helmet, boolean silent) {
        setItem(EquipmentSlot.HEAD, helmet, silent);
    }

    @Override
    public ItemStack getChestplate() {
        return getItem(EquipmentSlot.CHEST);
    }

    @Override
    public void setChestplate(ItemStack chestplate) {
        setItem(EquipmentSlot.CHEST, chestplate);
    }

    @Override
    public void setChestplate(ItemStack chestplate, boolean silent) {
        setItem(EquipmentSlot.CHEST, chestplate, silent);
    }

    @Override
    public ItemStack getLeggings() {
        return getItem(EquipmentSlot.LEGS);
    }

    @Override
    public void setLeggings(ItemStack leggings) {
        setItem(EquipmentSlot.LEGS, leggings);
    }

    @Override
    public void setLeggings(ItemStack leggings, boolean silent) {
        setItem(EquipmentSlot.LEGS, leggings, silent);
    }

    @Override
    public ItemStack getBoots() {
        return getItem(EquipmentSlot.FEET);
    }

    @Override
    public void setBoots(ItemStack boots) {
        setItem(EquipmentSlot.FEET, boots);
    }

    @Override
    public void setBoots(ItemStack boots, boolean silent) {
        setItem(EquipmentSlot.FEET, boots, silent);
    }

    @Override
    public ItemStack[] getArmorContents() {
        return new ItemStack[] {getBoots(), getLeggings(), getChestplate(), getHelmet()};
    }

    @Override
    public void setArmorContents(ItemStack[] items) {
        if (items == null) {
            setBoots(null);
            setLeggings(null);
            setChestplate(null);
            setHelmet(null);
            return;
        }
        setBoots(items.length > 0 ? items[0] : null);
        setLeggings(items.length > 1 ? items[1] : null);
        setChestplate(items.length > 2 ? items[2] : null);
        setHelmet(items.length > 3 ? items[3] : null);
    }

    @Override
    public void clear() {
        setItem(EquipmentSlot.HAND, null);
        setItem(EquipmentSlot.OFF_HAND, null);
        setItem(EquipmentSlot.FEET, null);
        setItem(EquipmentSlot.LEGS, null);
        setItem(EquipmentSlot.CHEST, null);
        setItem(EquipmentSlot.HEAD, null);
    }

    private void routeHostEquipmentWrite(EquipmentSlot slot, ItemStack item) {
        JsonObject req = new JsonObject();
        req.addProperty("uuid", entityUuid.toString());
        req.addProperty("slot", slot.name());
        if (item == null || item.getType() == Material.AIR) {
            req.add("item", com.google.gson.JsonNull.INSTANCE);
        } else {
            JsonObject patch = new JsonObject();
            patch.addProperty("id", item.getType().getKey().asString());
            patch.addProperty("count", Math.max(1, Math.min(99, item.getAmount())));
            String nbtBase64 = FerriteItemNbt.encodeItemBase64(item);
            if (nbtBase64 != null) {
                patch.addProperty("nbt_base64", nbtBase64);
            }
            req.add("item", patch);
        }
        try {
            String raw = Bootstrap.entityEquipmentSetAsync(req.toString());
            JsonObject envelope = JsonParser.parseString(raw).getAsJsonObject();
            if (envelope.has("ok") && envelope.get("ok").getAsBoolean()) {
                return;
            }
            var errElem = envelope.get("error");
            String code = envelope.has("code") && envelope.get("code").isJsonPrimitive()
                    ? envelope.get("code").getAsString()
                    : errElem != null && errElem.isJsonObject() && errElem.getAsJsonObject().has("code")
                            ? errElem.getAsJsonObject().get("code").getAsString()
                            : "host_error";
            if ("state_unavailable".equals(code)
                    || "unsupported".equals(code)
                    || "java_exception".equals(code)) {
                return;
            }
            throw new IllegalStateException(
                    "entity.equipment_set_async failed (" + code + "): " + raw);
        } catch (UnsatisfiedLinkError | NoClassDefFoundError ignored) {
            // Pure-Java unit tests without JNI update the in-memory snapshot only.
        }
    }

    private void routeHostDropChanceWrite(EquipmentSlot slot, float chance) {
        JsonObject req = new JsonObject();
        req.addProperty("uuid", entityUuid.toString());
        req.addProperty("slot", slot.name());
        req.addProperty("chance", chance);
        try {
            String raw = Bootstrap.entityDropChanceSetAsync(req.toString());
            JsonObject envelope = JsonParser.parseString(raw).getAsJsonObject();
            if (envelope.has("ok") && envelope.get("ok").getAsBoolean()) {
                return;
            }
            var errElem = envelope.get("error");
            String code = envelope.has("code") && envelope.get("code").isJsonPrimitive()
                    ? envelope.get("code").getAsString()
                    : errElem != null && errElem.isJsonObject() && errElem.getAsJsonObject().has("code")
                            ? errElem.getAsJsonObject().get("code").getAsString()
                            : "host_error";
            if ("state_unavailable".equals(code)
                    || "unsupported".equals(code)
                    || "java_exception".equals(code)) {
                return;
            }
            throw new IllegalStateException(
                    "entity.drop_chance_set_async failed (" + code + "): " + raw);
        } catch (UnsatisfiedLinkError | NoClassDefFoundError ignored) {
            // Pure-Java unit tests without JNI update the in-memory snapshot only.
        }
    }

    @Override
    @Deprecated
    public float getItemInHandDropChance() {
        return getItemInMainHandDropChance();
    }

    @Override
    @Deprecated
    public void setItemInHandDropChance(float chance) {
        setItemInMainHandDropChance(chance);
    }

    @Override
    public float getItemInMainHandDropChance() {
        return getDropChance(EquipmentSlot.HAND);
    }

    @Override
    public void setItemInMainHandDropChance(float chance) {
        setDropChance(EquipmentSlot.HAND, chance);
    }

    @Override
    public float getItemInOffHandDropChance() {
        return getDropChance(EquipmentSlot.OFF_HAND);
    }

    @Override
    public void setItemInOffHandDropChance(float chance) {
        setDropChance(EquipmentSlot.OFF_HAND, chance);
    }

    @Override
    public float getHelmetDropChance() {
        return getDropChance(EquipmentSlot.HEAD);
    }

    @Override
    public void setHelmetDropChance(float chance) {
        setDropChance(EquipmentSlot.HEAD, chance);
    }

    @Override
    public float getChestplateDropChance() {
        return getDropChance(EquipmentSlot.CHEST);
    }

    @Override
    public void setChestplateDropChance(float chance) {
        setDropChance(EquipmentSlot.CHEST, chance);
    }

    @Override
    public float getLeggingsDropChance() {
        return getDropChance(EquipmentSlot.LEGS);
    }

    @Override
    public void setLeggingsDropChance(float chance) {
        setDropChance(EquipmentSlot.LEGS, chance);
    }

    @Override
    public float getBootsDropChance() {
        return getDropChance(EquipmentSlot.FEET);
    }

    @Override
    public void setBootsDropChance(float chance) {
        setDropChance(EquipmentSlot.FEET, chance);
    }

    @Override
    public Entity getHolder() {
        return holder;
    }

    @Override
    public float getDropChance(EquipmentSlot slot) {
        Objects.requireNonNull(slot, "slot");
        if (!isSupportedSlot(slot)) {
            throw Unsupported.exception("EntityEquipment#getDropChance(" + slot + ")");
        }
        return dropChances.getOrDefault(slot, DEFAULT_DROP_CHANCE);
    }

    @Override
    public void setDropChance(EquipmentSlot slot, float chance) {
        Objects.requireNonNull(slot, "slot");
        if (!isSupportedSlot(slot)) {
            throw Unsupported.exception("EntityEquipment#setDropChance(" + slot + ")");
        }
        if (!Float.isFinite(chance)) {
            throw new IllegalArgumentException("drop chance must be finite");
        }
        routeHostDropChanceWrite(slot, chance);
        dropChances.put(slot, chance);
    }
}
