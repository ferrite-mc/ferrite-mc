package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import com.destroystokyo.paper.entity.ai.Goal;
import com.destroystokyo.paper.entity.ai.GoalKey;
import com.destroystokyo.paper.entity.ai.GoalType;
import io.papermc.paper.datapack.Datapack;
import io.papermc.paper.datapack.DatapackSource;
import io.papermc.paper.potion.PotionMix;
import java.util.EnumSet;
import java.util.List;
import java.util.UUID;
import net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.entity.Mob;
import org.bukkit.inventory.BrewingRecipe;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.RecipeChoice;
import org.bukkit.potion.PotionEffectType;
import org.bukkit.potion.PotionType;
import org.bukkit.util.BlockVector;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

class FerriteServerObjectsTest {
    private static FerriteServer server;

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        server = (FerriteServer) org.bukkit.Bukkit.getServer();
    }

    @Test void potionBrewerMixesGoThroughTheRecipeManager() {
        var brewer = server.getPotionBrewer();
        assertSame(brewer, server.getPotionBrewer());
        var key = new NamespacedKey("ferrite_test", "speed_mix");
        var mix = new PotionMix(key, new ItemStack(Material.POTION),
                new RecipeChoice.MaterialChoice(Material.POTION), new RecipeChoice.MaterialChoice(Material.SUGAR));
        brewer.addPotionMix(mix);
        assertInstanceOf(BrewingRecipe.class, server.getRecipe(key));
        brewer.removePotionMix(key);
        assertNull(server.getRecipe(key));
        brewer.addPotionMix(mix);
        brewer.resetPotionMixes();
        assertNull(server.getRecipe(key), "reset removes the mixes this brewer added");

        var effect = brewer.createEffect(PotionEffectType.SPEED, 200, 1);
        assertEquals(PotionEffectType.SPEED, effect.getType());
        assertEquals(200, effect.getDuration());
        assertEquals(1, effect.getAmplifier());
        assertTrue(brewer.getEffectsFromDamage(4).isEmpty());
        var base = brewer.getEffects(PotionType.SWIFTNESS, false, false);
        assertEquals(1, base.size());
        var baseEffect = base.iterator().next();
        assertEquals(PotionEffectType.SPEED, baseEffect.getType());
        assertEquals(0, baseEffect.getAmplifier());
        var upgraded = brewer.getEffects(PotionType.SWIFTNESS, true, false).iterator().next();
        assertEquals(1, upgraded.getAmplifier());
        var extended = brewer.getEffects(PotionType.SWIFTNESS, false, true).iterator().next();
        assertTrue(extended.getDuration() > baseEffect.getDuration());
        assertThrows(IllegalArgumentException.class, () -> brewer.getEffects(PotionType.WATER, true, false));

        assertTrue(PotionType.SWIFTNESS.isUpgradeable());
        assertTrue(PotionType.SWIFTNESS.isExtendable());
        assertEquals(2, PotionType.SWIFTNESS.getMaxLevel());
        assertEquals(PotionEffectType.SPEED, PotionType.SWIFTNESS.getEffectType());
        assertFalse(PotionType.SWIFTNESS.isInstant());
        assertFalse(PotionType.LONG_SWIFTNESS.isUpgradeable());
        assertFalse(PotionType.LONG_SWIFTNESS.isExtendable());
        assertEquals(1, PotionType.LONG_SWIFTNESS.getMaxLevel());
        assertTrue(PotionType.HEALING.isInstant());
        assertNull(PotionType.WATER.getEffectType());
        assertTrue(PotionType.WATER.getPotionEffects().isEmpty());
        var turtle = PotionType.TURTLE_MASTER.getPotionEffects();
        assertEquals(List.of(PotionEffectType.SLOWNESS, PotionEffectType.RESISTANCE),
                turtle.stream().map(org.bukkit.potion.PotionEffect::getType).toList());
        assertEquals(List.of(400, 400), turtle.stream().map(org.bukkit.potion.PotionEffect::getDuration).toList());
        assertEquals(List.of(3, 2), turtle.stream().map(org.bukkit.potion.PotionEffect::getAmplifier).toList());
        assertThrows(IllegalArgumentException.class,
                () -> server.getUnsafe().getInternalPotionData(NamespacedKey.minecraft("not_a_potion")));
    }

    @Test void timedProfileBansExpireAndPardonTheHost() throws Exception {
        var calls = new java.util.concurrent.CopyOnWriteArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> { calls.add(method); return ""; };
        var id = UUID.fromString("00000000-0000-0000-0000-00000000ba11");
        var player = FerritePlayer.create(server, id, "TimedBan", false, 20.0, 20, 0, null);
        server.registerPlayer(player);
        try {
            assertNotNull(player.ban("later", java.time.Duration.ofMillis(50), "test", false));
            assertTrue(player.isBanned());
            var deadline = System.nanoTime() + java.util.concurrent.TimeUnit.SECONDS.toNanos(2);
            while (player.isBanned() && System.nanoTime() < deadline) Thread.sleep(10);
            assertFalse(player.isBanned(), "the timed ban expires");
            assertTrue(calls.contains("banProfile(String,String)"));
            assertTrue(calls.contains("pardonProfile(String)"));
        } finally {
            server.unregisterPlayer(id);
            HostBridge.transport = null;
        }
    }

    @Test void serverConfigReflectsTheMissingProxy() {
        var config = server.getServerConfig();
        assertSame(config, server.getServerConfig());
        assertFalse(config.isProxyEnabled());
        assertFalse(config.isProxyOnlineMode());
    }

    @Test void mobGoalsRegistryKeepsGoalsByPriority() {
        var goals = server.getMobGoals();
        var mob = mob(UUID.fromString("00000000-0000-0000-0000-0000000000e1"));
        var lookKey = GoalKey.of(Mob.class, new NamespacedKey("ferrite_test", "look_goal"));
        var moveKey = GoalKey.of(Mob.class, new NamespacedKey("ferrite_test", "move_goal"));
        Goal<Mob> look = goal(lookKey, GoalType.LOOK);
        Goal<Mob> move = goal(moveKey, GoalType.MOVE);

        assertFalse(goals.hasGoal(mob, lookKey));
        assertNull(goals.getGoal(mob, lookKey));
        assertTrue(goals.getAllGoals(mob).isEmpty());

        goals.addGoal(mob, 2, look);
        goals.addGoal(mob, 1, move);
        assertTrue(goals.hasGoal(mob, lookKey));
        assertSame(look, goals.getGoal(mob, lookKey));
        assertEquals(List.of(move, look), List.copyOf(goals.getAllGoals(mob)), "lower priority value comes first");
        assertEquals(List.of(look), List.copyOf(goals.getGoals(mob, lookKey)));
        assertEquals(List.of(look), List.copyOf(goals.getAllGoals(mob, GoalType.LOOK)));
        assertEquals(List.of(move), List.copyOf(goals.getAllGoalsWithout(mob, GoalType.LOOK)));
        assertTrue(goals.getRunningGoals(mob).isEmpty(), "the host owns AI execution");
        assertTrue(goals.getRunningGoals(mob, GoalType.MOVE).isEmpty());
        assertTrue(goals.getRunningGoalsWithout(mob, GoalType.MOVE).isEmpty());

        goals.removeGoal(mob, lookKey);
        assertFalse(goals.hasGoal(mob, lookKey));
        goals.removeGoal(mob, move);
        assertTrue(goals.getAllGoals(mob).isEmpty());

        goals.addGoal(mob, 0, look);
        goals.addGoal(mob, 0, move);
        goals.removeAllGoals(mob, GoalType.MOVE);
        assertEquals(List.of(look), List.copyOf(goals.getAllGoals(mob)));
        goals.removeAllGoals(mob);
        assertTrue(goals.getAllGoals(mob).isEmpty());
        assertThrows(IllegalArgumentException.class, () -> goals.addGoal(mob, -1, look));
    }

    @Test void datapackManagerReportsOnlyTheVanillaPack() {
        var manager = server.getDatapackManager();
        assertEquals(List.of("vanilla"), manager.getPacks().stream().map(Datapack::getName).toList());
        var pack = manager.getPack("vanilla");
        assertNotNull(pack);
        assertSame(pack, manager.getPacks().iterator().next());
        assertTrue(pack.isEnabled());
        assertTrue(manager.getEnabledPacks().contains(pack));
        assertTrue(pack.isRequired());
        assertEquals(DatapackSource.BUILT_IN, pack.getSource());
        assertEquals(Datapack.Compatibility.COMPATIBLE, pack.getCompatibility());
        assertTrue(pack.getRequiredFeatures().isEmpty());
        assertEquals("Vanilla", PlainTextComponentSerializer.plainText().serialize(pack.computeDisplayName()));
        assertNull(manager.getPack("ferrite_test_missing"));
        assertThrows(IllegalArgumentException.class, () -> pack.setEnabled(false), "the vanilla pack is required");
        assertTrue(pack.isEnabled());
        // The unit test runs without the Rust host, so the reload reaches the bridge and fails there.
        assertEquals("Ferrite: not implemented: Server#reloadData()",
                assertThrows(UnsupportedOperationException.class, manager::refreshPacks).getMessage());
    }

    @Test void structureManagerRegistersStructuresInMemory() {
        var structures = server.getStructureManager();
        assertSame(structures, server.getStructureManager());
        assertTrue(structures.getStructures().isEmpty());

        var structure = structures.createStructure();
        assertEquals(new BlockVector(0, 0, 0), structure.getSize());
        assertEquals(0, structure.getPaletteCount());
        assertTrue(structure.getPalettes().isEmpty());
        assertEquals(0, structure.getEntityCount());
        assertTrue(structure.getPersistentDataContainer().isEmpty());

        var key = new NamespacedKey("ferrite_test", "hut");
        assertNull(structures.getStructure(key));
        assertNull(structures.registerStructure(key, structure));
        assertSame(structure, structures.getStructure(key));
        assertSame(structure, structures.getStructures().get(key));
        var replacement = structures.createStructure();
        assertSame(structure, structures.registerStructure(key, replacement));
        assertSame(replacement, structures.getStructure(key));
        assertSame(replacement, structures.unregisterStructure(key));
        assertNull(structures.getStructure(key));

        assertThrows(IllegalStateException.class, () -> structures.saveStructure(key), "an unregistered key cannot be saved");
        assertNull(structures.loadStructure(key), "a key without a template file loads nothing");
        assertThrows(NullPointerException.class,
                () -> structure.place((org.bukkit.Location) null, false, org.bukkit.block.structure.StructureRotation.NONE, org.bukkit.block.structure.Mirror.NONE, 0, 1.0f, new java.util.Random()));
    }

    @Test void entityFactoryRejectsSnapshotsUntilTheHostParsesThem() {
        var factory = server.getEntityFactory();
        assertSame(factory, server.getEntityFactory());
        assertEquals("Ferrite: not implemented: EntityFactory#createEntitySnapshot",
                assertThrows(UnsupportedOperationException.class,
                        () -> factory.createEntitySnapshot("{id:\"minecraft:pig\"}")).getMessage());
    }

    private static Goal<Mob> goal(GoalKey<Mob> key, GoalType... types) {
        return new Goal<>() {
            @Override public boolean shouldActivate() { return false; }
            @Override public GoalKey<Mob> getKey() { return key; }
            @Override public EnumSet<GoalType> getTypes() { return EnumSet.copyOf(List.of(types)); }
        };
    }

    private static Mob mob(UUID id) {
        return (Mob) java.lang.reflect.Proxy.newProxyInstance(FerriteServerObjectsTest.class.getClassLoader(),
                new Class<?>[]{Mob.class}, (proxy, method, args) -> switch (method.getName()) {
                    case "getUniqueId" -> id;
                    case "equals" -> proxy == args[0];
                    case "hashCode" -> id.hashCode();
                    case "toString" -> "Mob[" + id + "]";
                    default -> null;
                });
    }
}
