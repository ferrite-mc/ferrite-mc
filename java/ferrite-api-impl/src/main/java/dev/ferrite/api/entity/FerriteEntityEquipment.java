package dev.ferrite.api.entity;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import dev.ferrite.api.internal.Unsupported;
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

    private final Entity holder;
    private final UUID entityUuid;
    private final EnumMap<EquipmentSlot, ItemStack> slots = new EnumMap<>(EquipmentSlot.class);

    public FerriteEntityEquipment(
            Entity holder, UUID entityUuid, Map<EquipmentSlot, ItemStack> initial) {
        this.holder = Objects.requireNonNull(holder, "holder");
        this.entityUuid = Objects.requireNonNull(entityUuid, "entityUuid");
        if (initial != null) {
            for (var entry : initial.entrySet()) {
                if (isSupportedSlot(entry.getKey()) && entry.getValue() != null && !entry.getValue().getType().isAir()) {
                    slots.put(entry.getKey(), entry.getValue().clone());
                }
            }
        }
    }

    private static boolean isSupportedSlot(EquipmentSlot slot) {
        return switch (slot) {
            case HAND, OFF_HAND, FEET, LEGS, CHEST, HEAD -> true;
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

    @Override
    @Deprecated
    public float getItemInHandDropChance() {
        throw Unsupported.exception("EntityEquipment#getItemInHandDropChance");
    }

    @Override
    @Deprecated
    public void setItemInHandDropChance(float chance) {
        throw Unsupported.exception("EntityEquipment#setItemInHandDropChance");
    }

    @Override
    public float getItemInMainHandDropChance() {
        throw Unsupported.exception("EntityEquipment#getItemInMainHandDropChance");
    }

    @Override
    public void setItemInMainHandDropChance(float chance) {
        throw Unsupported.exception("EntityEquipment#setItemInMainHandDropChance");
    }

    @Override
    public float getItemInOffHandDropChance() {
        throw Unsupported.exception("EntityEquipment#getItemInOffHandDropChance");
    }

    @Override
    public void setItemInOffHandDropChance(float chance) {
        throw Unsupported.exception("EntityEquipment#setItemInOffHandDropChance");
    }

    @Override
    public float getHelmetDropChance() {
        throw Unsupported.exception("EntityEquipment#getHelmetDropChance");
    }

    @Override
    public void setHelmetDropChance(float chance) {
        throw Unsupported.exception("EntityEquipment#setHelmetDropChance");
    }

    @Override
    public float getChestplateDropChance() {
        throw Unsupported.exception("EntityEquipment#getChestplateDropChance");
    }

    @Override
    public void setChestplateDropChance(float chance) {
        throw Unsupported.exception("EntityEquipment#setChestplateDropChance");
    }

    @Override
    public float getLeggingsDropChance() {
        throw Unsupported.exception("EntityEquipment#getLeggingsDropChance");
    }

    @Override
    public void setLeggingsDropChance(float chance) {
        throw Unsupported.exception("EntityEquipment#setLeggingsDropChance");
    }

    @Override
    public float getBootsDropChance() {
        throw Unsupported.exception("EntityEquipment#getBootsDropChance");
    }

    @Override
    public void setBootsDropChance(float chance) {
        throw Unsupported.exception("EntityEquipment#setBootsDropChance");
    }

    @Override
    public Entity getHolder() {
        return holder;
    }

    @Override
    public float getDropChance(EquipmentSlot slot) {
        throw Unsupported.exception("EntityEquipment#getDropChance");
    }

    @Override
    public void setDropChance(EquipmentSlot slot, float chance) {
        throw Unsupported.exception("EntityEquipment#setDropChance");
    }
}
