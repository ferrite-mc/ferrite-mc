package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.lang.reflect.Proxy;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.Random;
import java.util.Set;
import java.util.UUID;
import org.bukkit.Location;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.entity.Entity;
import org.bukkit.entity.EntityType;
import org.bukkit.entity.HumanEntity;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.PlayerInventory;
import org.bukkit.loot.LootContext;
import org.bukkit.loot.LootTable;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;

class FerriteLootTableTest {
    private static FerriteServer server;
    private static org.bukkit.World world;

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        server = (FerriteServer) org.bukkit.Bukkit.getServer();
        Bootstrap.registerWorld("00000000-0000-0000-0000-0000000000e2", "loottest", 0, 0, 64, 0);
        world = org.bukkit.Bukkit.getWorld("loottest");
    }

    @Test void stoneTableDropsCobblestoneWithAFixedSeed() {
        var table = server.getLootTable(NamespacedKey.minecraft("blocks/stone"));
        assertNotNull(table);
        var context = context(0.0f, null, null);
        var first = List.copyOf(table.populateLoot(new Random(1234), context));
        var second = List.copyOf(table.populateLoot(new Random(1234), context));

        assertEquals(first, second, "the same seed reproduces the same drops");
        assertEquals(1, first.size());
        assertEquals(Material.COBBLESTONE, first.get(0).getType());
        assertEquals(1, first.get(0).getAmount());
    }

    @Test void silkTouchOnTheKillerToolSelectsTheBlockItself() {
        var table = server.getLootTable(NamespacedKey.minecraft("blocks/stone"));
        var silkTouch = tool(Material.DIAMOND_PICKAXE, "silk_touch", 1);
        var drops = List.copyOf(table.populateLoot(new Random(7), context(0.0f, killer(silkTouch), null)));

        assertEquals(List.of(Material.STONE), drops.stream().map(ItemStack::getType).toList());
    }

    @Test void zombieTableIsDeterministicAndLootingRaisesTheCap() {
        var table = server.getLootTable(NamespacedKey.minecraft("entities/zombie"));
        var noKiller = context(0.0f, null, null);
        var first = List.copyOf(table.populateLoot(new Random(99), noKiller));
        var second = List.copyOf(table.populateLoot(new Random(99), noKiller));

        assertEquals(first, second);
        assertTrue(first.stream().allMatch(stack -> stack.getType() == Material.ROTTEN_FLESH));
        assertTrue(first.stream().allMatch(stack -> stack.getAmount() >= 1 && stack.getAmount() <= 2));

        var looting = tool(Material.DIAMOND_SWORD, "looting", 3);
        var withLooting = context(0.0f, killer(looting), null);
        var allowed = Set.of(Material.ROTTEN_FLESH, Material.IRON_INGOT, Material.CARROT, Material.POTATO);
        for (int seed = 0; seed < 25; seed++) {
            var drops = table.populateLoot(new Random(seed), withLooting);
            int flesh = 0;
            for (var stack : drops) {
                assertTrue(allowed.contains(stack.getType()), "unexpected " + stack.getType());
                if (stack.getType() == Material.ROTTEN_FLESH) flesh += stack.getAmount();
            }
            assertTrue(flesh <= 5, "looting III cannot exceed five rotten flesh: " + flesh);
        }
    }

    @Test void killerAndLootedEntityFeedEntityConditions(@TempDir Path temp) throws Exception {
        var table = fixture(temp.resolve("data"), "entities", """
                {"pools":[
                  {"rolls":1,"condition":{"type":"minecraft:killed_by_player"},
                   "entries":[{"type":"minecraft:item","name":"minecraft:diamond"}]},
                  {"rolls":1,"condition":{"type":"minecraft:entity_properties","entity":"this",
                   "predicate":{"minecraft:type":"minecraft:cow"}},
                   "entries":[{"type":"minecraft:item","name":"minecraft:emerald"}]}]}
                """);
        var sword = tool(Material.DIAMOND_SWORD, "looting", 1);
        var withCow = table.populateLoot(new Random(1), context(0.0f, killer(sword), entity(EntityType.COW)));
        assertEquals(Set.of(Material.DIAMOND, Material.EMERALD),
                withCow.stream().map(ItemStack::getType).collect(java.util.stream.Collectors.toSet()));

        var withoutCow = table.populateLoot(new Random(1), context(0.0f, killer(sword), null));
        assertEquals(List.of(Material.DIAMOND),
                withoutCow.stream().map(ItemStack::getType).toList());
    }

    @Test void luckAddsBonusRolls(@TempDir Path temp) throws Exception {
        var table = fixture(temp.resolve("data"), "luck", """
                {"pools":[{"rolls":1,"bonus_rolls":1,"entries":[
                  {"type":"minecraft:item","name":"minecraft:stone"}]}]}
                """);
        assertEquals(1, table.populateLoot(new Random(1), context(0.0f, null, null)).size());
        assertEquals(3, table.populateLoot(new Random(1), context(2.0f, null, null)).size());
    }

    @Test void missingEmptyAndFailedTablesReturnNoLoot(@TempDir Path temp) throws Exception {
        var data = temp.resolve("data");
        var empty = fixture(data, "empty", """
                {"pools":[{"rolls":1,"entries":[]}]}
                """);
        var never = fixture(data, "never", """
                {"pools":[{"rolls":1,"entries":[
                  {"type":"minecraft:item","name":"minecraft:diamond",
                   "condition":{"type":"minecraft:random_chance","chance":0.0}}]}]}
                """);
        var missingReference = fixture(data, "missing_reference", """
                {"pools":[{"rolls":1,"entries":[
                  {"type":"minecraft:loot_table","value":"minecraft:no_such_table"}]}]}
                """);
        var missing = FerriteLootTable.at(new NamespacedKey("minecraft", "no_such_table"), data);
        var context = context(0.0f, null, null);

        assertTrue(empty.populateLoot(new Random(1), context).isEmpty());
        assertTrue(never.populateLoot(new Random(1), context).isEmpty());
        assertTrue(missingReference.populateLoot(new Random(1), context).isEmpty());
        assertTrue(missing.populateLoot(new Random(1), context).isEmpty());
    }

    @Test void conditionsCanPass(@TempDir Path temp) throws Exception {
        var table = fixture(temp.resolve("data"), "chance", """
                {"pools":[{"rolls":1,"entries":[
                  {"type":"minecraft:item","name":"minecraft:diamond",
                   "condition":{"type":"minecraft:random_chance","chance":1.0}}]}]}
                """);
        var drops = List.copyOf(table.populateLoot(new Random(1), context(0.0f, null, null)));

        assertEquals(1, drops.size());
        assertEquals(Material.DIAMOND, drops.get(0).getType());
    }

    @Test void setCountAndSetNbtReachTheItemMeta(@TempDir Path temp) throws Exception {
        var table = fixture(temp.resolve("data"), "nbt", """
                {"pools":[{"rolls":1,"entries":[
                  {"type":"minecraft:item","name":"minecraft:stick","modifier":[
                    {"type":"minecraft:set_count","count":3},
                    {"type":"minecraft:set_nbt","nbt":"{display:{Name:'{\\"text\\":\\"Loot Stick\\"}',Lore:['{\\"text\\":\\"A\\"}']},Enchantments:[{id:\\"minecraft:unbreaking\\",lvl:2}],CustomModelData:7}"}
                  ]}]}]}
                """);
        var drops = List.copyOf(table.populateLoot(new Random(1), context(0.0f, null, null)));

        assertEquals(1, drops.size());
        var stack = drops.get(0);
        assertEquals(Material.STICK, stack.getType());
        assertEquals(3, stack.getAmount());
        assertTrue(stack.hasItemMeta());
        var meta = stack.getItemMeta();
        assertEquals("Loot Stick", net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer
                .plainText().serialize(meta.displayName()));
        assertEquals(List.of("A"), meta.getLore());
        var unbreaking = Registry.ENCHANTMENT.get(NamespacedKey.minecraft("unbreaking"));
        assertNotNull(unbreaking);
        assertEquals(2, meta.getEnchantLevel(unbreaking));
        assertEquals(7, meta.getCustomModelData());
    }

    @Test void enchantRandomlyUsesTheConfiguredOptions(@TempDir Path temp) throws Exception {
        var table = fixture(temp.resolve("data"), "enchant", """
                {"pools":[{"rolls":1,"entries":[
                  {"type":"minecraft:item","name":"minecraft:book","modifier":{
                    "type":"minecraft:enchant_randomly","options":["minecraft:sharpness"]}}]}]}
                """);
        var drops = List.copyOf(table.populateLoot(new Random(1), context(0.0f, null, null)));

        assertEquals(1, drops.size());
        var sharpness = Registry.ENCHANTMENT.get(NamespacedKey.minecraft("sharpness"));
        assertTrue(drops.get(0).getItemMeta().getEnchantLevel(sharpness) >= 1);
    }

    @Test void fillInventoryRespectsSlotsAndMaxStackSize(@TempDir Path temp) throws Exception {
        var table = fixture(temp.resolve("data"), "fill", """
                {"pools":[{"rolls":1,"entries":[
                  {"type":"minecraft:item","name":"minecraft:stone","modifier":[
                    {"type":"minecraft:set_count","count":64}]}]}]}
                """);
        var inventory = org.bukkit.Bukkit.createInventory(null, 9);
        inventory.setMaxStackSize(16);
        inventory.setItem(0, new FerriteItemStack(Material.DIRT, 1));

        table.fillInventory(inventory, new Random(4), context(0.0f, null, null));

        assertEquals(Material.DIRT, inventory.getItem(0).getType(), "an occupied slot is not replaced");
        int total = 0;
        int used = 0;
        for (int slot = 1; slot < inventory.getSize(); slot++) {
            var stack = inventory.getItem(slot);
            if (stack == null) continue;
            assertEquals(Material.STONE, stack.getType());
            assertTrue(stack.getAmount() <= 16, "slot " + slot + " exceeds the max stack size");
            total += stack.getAmount();
            used++;
        }
        assertEquals(64, total);
        assertEquals(4, used);
    }

    private static LootContext context(float luck, HumanEntity killer, Entity looted) {
        var builder = new LootContext.Builder(new Location(world, 0, 64, 0)).luck(luck);
        if (killer != null) builder.killer(killer);
        if (looted != null) builder.lootedEntity(looted);
        return builder.build();
    }

    private static ItemStack tool(Material material, String enchantment, int level) {
        var item = new FerriteItemStack(material, 1);
        var resolved = Registry.ENCHANTMENT.get(NamespacedKey.minecraft(enchantment));
        assertNotNull(resolved, "missing enchantment " + enchantment);
        item.addUnsafeEnchantment(resolved, level);
        return item;
    }

    private static HumanEntity killer(ItemStack tool) {
        var inventory = (PlayerInventory) Proxy.newProxyInstance(
                FerriteLootTableTest.class.getClassLoader(),
                new Class<?>[]{PlayerInventory.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "getItemInMainHand" -> tool;
                    case "equals" -> proxy == args[0];
                    case "hashCode" -> System.identityHashCode(proxy);
                    case "toString" -> "LootTestInventory";
                    default -> fallback(method.getReturnType());
                });
        return (HumanEntity) Proxy.newProxyInstance(
                FerriteLootTableTest.class.getClassLoader(),
                new Class<?>[]{HumanEntity.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "getType" -> EntityType.PLAYER;
                    case "getUniqueId" -> UUID.fromString("00000000-0000-0000-0000-0000000000e3");
                    case "getInventory" -> inventory;
                    case "getFireTicks" -> 0;
                    case "isSneaking", "isSprinting", "isSwimming", "isOnGround", "isInvisible",
                            "isSilent", "isGliding" -> false;
                    case "getVehicle" -> null;
                    case "getName" -> "LootTest";
                    case "equals" -> proxy == args[0];
                    case "hashCode" -> System.identityHashCode(proxy);
                    case "toString" -> "LootTestKiller";
                    default -> fallback(method.getReturnType());
                });
    }

    private static Entity entity(EntityType type) {
        return (Entity) Proxy.newProxyInstance(
                FerriteLootTableTest.class.getClassLoader(),
                new Class<?>[]{Entity.class},
                (proxy, method, args) -> switch (method.getName()) {
                    case "getType" -> type;
                    case "getFireTicks" -> 0;
                    case "isSneaking", "isSprinting", "isSwimming", "isOnGround", "isInvisible",
                            "isSilent", "isGliding" -> false;
                    case "getVehicle" -> null;
                    case "equals" -> proxy == args[0];
                    case "hashCode" -> System.identityHashCode(proxy);
                    case "toString" -> "LootTestEntity";
                    default -> fallback(method.getReturnType());
                });
    }

    private static Object fallback(Class<?> type) {
        if (type == boolean.class) return false;
        if (type == byte.class) return (byte) 0;
        if (type == short.class) return (short) 0;
        if (type == int.class) return 0;
        if (type == long.class) return 0L;
        if (type == float.class) return 0.0f;
        if (type == double.class) return 0.0;
        if (type == char.class) return '\0';
        return null;
    }

    private static LootTable fixture(Path data, String name, String json) throws Exception {
        return fixture(data, "minecraft", name, json);
    }

    private static LootTable fixture(Path data, String namespace, String name, String json) throws Exception {
        var file = data.resolve(namespace).resolve("loot_table").resolve(name + ".json");
        Files.createDirectories(file.getParent());
        Files.writeString(file, json);
        return FerriteLootTable.at(new NamespacedKey(namespace, name), data);
    }
}
