package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.io.ByteArrayOutputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.util.Base64;
import net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.inventory.meta.Damageable;
import org.bukkit.persistence.PersistentDataType;
import org.junit.jupiter.api.Test;

class FerriteItemNbtTest {
    private static void named(DataOutputStream out, int type, String name) throws IOException {
        out.writeByte(type);
        out.writeUTF(name);
    }

    /** A named root compound with the fields {@code ItemStack#to_nbt} writes. */
    private static String snapshotNbt() throws IOException {
        var bytes = new ByteArrayOutputStream();
        try (var out = new DataOutputStream(bytes)) {
            out.writeByte(10);
            out.writeUTF("");
            named(out, 8, "id");
            out.writeUTF("minecraft:diamond_sword");
            named(out, 3, "count");
            out.writeInt(1);
            named(out, 10, "components");
            named(out, 3, "minecraft:damage");
            out.writeInt(7);
            named(out, 8, "minecraft:item_name");
            out.writeUTF("Snapshot Sword");
            named(out, 10, "minecraft:custom_data");
            named(out, 1, "test:flag");
            out.writeByte(1);
            out.writeByte(0);
            out.writeByte(0);
            out.writeByte(0);
        }
        return Base64.getEncoder().encodeToString(bytes.toByteArray());
    }

    @Test void decodesTheHostItemAndItsComponents() throws Exception {
        var stack = FerriteItemNbt.decode(snapshotNbt());
        assertEquals(Material.DIAMOND_SWORD, stack.getType());
        assertEquals(1, stack.getAmount());
        var meta = stack.getItemMeta();
        assertInstanceOf(Damageable.class, meta);
        assertEquals(7, ((Damageable) meta).getDamage());
        assertEquals("Snapshot Sword", PlainTextComponentSerializer.plainText().serialize(meta.displayName()));
        assertEquals(Byte.valueOf((byte) 1),
                meta.getPersistentDataContainer().get(new NamespacedKey("test", "flag"), PersistentDataType.BYTE));
    }

    @Test void bootstrapExposesTheHostInventorySnapshotMethods() throws Exception {
        assertNotNull(Bootstrap.class.getMethod("setInventorySnapshotNbt", String.class, int.class, String.class));
        assertNotNull(Bootstrap.class.getMethod("clearInventorySnapshot", String.class, int.class));
    }
}
