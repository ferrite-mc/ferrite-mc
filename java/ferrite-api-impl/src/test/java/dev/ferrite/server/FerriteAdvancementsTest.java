package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import io.papermc.paper.advancement.AdvancementDisplay;
import java.util.List;
import java.util.UUID;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.advancement.Advancement;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

class FerriteAdvancementsTest {
    private static final UUID PLAYER_ID = UUID.fromString("00000000-0000-0000-0000-0000000000c1");
    private static FerriteServer server;

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        server = (FerriteServer) org.bukkit.Bukkit.getServer();
    }

    @Test void advancementsLoadFromTheVanillaData() {
        var advancement = server.getAdvancement(NamespacedKey.minecraft("story/mine_stone"));
        assertNotNull(advancement);
        assertEquals(NamespacedKey.minecraft("story/mine_stone"), advancement.getKey());
        assertTrue(advancement.getCriteria().contains("get_stone"));
        var requirements = advancement.getRequirements().getRequirements();
        assertEquals(1, requirements.size());
        assertEquals(List.of("get_stone"), requirements.get(0).getRequiredCriteria());
        assertFalse(requirements.get(0).isStrict());

        assertNull(server.getAdvancement(NamespacedKey.minecraft("ferrite/missing")));
        assertThrows(NullPointerException.class, () -> server.getAdvancement(null));

        var parent = advancement.getParent();
        assertNotNull(parent);
        assertEquals(NamespacedKey.minecraft("story/root"), parent.getKey());
        assertSame(parent, advancement.getRoot());
        assertTrue(parent.getChildren().contains(advancement));
        var display = parent.getDisplay();
        assertNotNull(display);
        assertEquals(AdvancementDisplay.Frame.TASK, display.frame());
        assertEquals(Material.GRASS_BLOCK, display.icon().getType());
        assertEquals(NamespacedKey.minecraft("gui/advancements/backgrounds/stone"), display.backgroundPath());
        assertFalse(display.doesShowToast());
        assertFalse(display.doesAnnounceToChat());
        assertFalse(display.isHidden());
        assertEquals(display.title(), parent.displayName());
    }

    @Test void iteratorCoversTheWholeDataPack() {
        int count = 0;
        for (var ignored : (Iterable<Advancement>) server::advancementIterator) count++;
        assertTrue(count > 100, "expected the full advancement set, got " + count);
    }

    @Test void progressAwardsAndRevokesCriteria() {
        var advancement = server.getAdvancement(NamespacedKey.minecraft("story/mine_stone"));
        assertNotNull(advancement);
        var player = FerritePlayer.create(server, PLAYER_ID, "Advancer", false, 20.0, 20, 0, null);
        server.registerPlayer(player);
        try {
            var progress = player.getAdvancementProgress(advancement);
            assertSame(progress, player.getAdvancementProgress(advancement));
            assertSame(advancement, progress.getAdvancement());
            assertFalse(progress.isDone());
            assertEquals(List.of("get_stone"), List.copyOf(progress.getRemainingCriteria()));
            assertNull(progress.getDateAwarded("get_stone"));

            assertTrue(progress.awardCriteria("get_stone"));
            assertFalse(progress.awardCriteria("get_stone"), "an awarded criterion stays awarded");
            assertTrue(progress.isDone());
            assertTrue(progress.getRemainingCriteria().isEmpty());
            assertEquals(List.of("get_stone"), List.copyOf(progress.getAwardedCriteria()));
            assertNotNull(progress.getDateAwarded("get_stone"));

            assertTrue(progress.revokeCriteria("get_stone"));
            assertFalse(progress.revokeCriteria("get_stone"));
            assertFalse(progress.isDone());
            assertNull(progress.getDateAwarded("get_stone"));
            assertThrows(IllegalArgumentException.class, () -> progress.awardCriteria("nope"));
            assertThrows(IllegalArgumentException.class, () -> progress.revokeCriteria("nope"));
        } finally {
            server.unregisterPlayer(PLAYER_ID);
        }
    }
}
