package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import net.kyori.adventure.bossbar.BossBar;
import net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer;
import org.bukkit.NamespacedKey;
import org.bukkit.entity.Player;
import org.junit.jupiter.api.Test;

/** Bukkit boss bars must show, update and hide the Adventure bar the host already sends. */
class FerriteBossBarTest {
    private static Player viewer(List<BossBar> shown) {
        var id = UUID.randomUUID();
        return (Player) Proxy.newProxyInstance(FerriteBossBarTest.class.getClassLoader(), new Class<?>[]{Player.class}, (proxy, method, args) -> switch (method.getName()) {
            case "showBossBar" -> { shown.add((BossBar) args[0]); yield null; }
            case "hideBossBar" -> { shown.remove((BossBar) args[0]); yield null; }
            case "getUniqueId" -> id;
            case "getName" -> "Viewer-" + id;
            case "hashCode" -> id.hashCode();
            case "equals" -> proxy == args[0];
            case "toString" -> "Viewer[" + id + "]";
            default -> throw new UnsupportedOperationException(method.getName());
        });
    }

    private static String plain(BossBar bar) {
        return PlainTextComponentSerializer.plainText().serialize(bar.name());
    }

    @Test void viewersSeeTheBarAndEveryAttributeChange() {
        var bar = new FerriteBossBar(new NamespacedKey("ferrite_test", "boss"),
                "Boss", org.bukkit.boss.BarColor.RED, org.bukkit.boss.BarStyle.SEGMENTED_10,
                org.bukkit.boss.BarFlag.DARKEN_SKY);
        var shown = new ArrayList<BossBar>();
        var player = viewer(shown);

        bar.addPlayer(player);
        assertEquals(1, shown.size(), "adding a viewer must show the bar");
        var adventure = shown.getFirst();
        assertEquals("Boss", plain(adventure));
        assertEquals(1.0f, adventure.progress());
        assertEquals(BossBar.Color.RED, adventure.color());
        assertEquals(BossBar.Overlay.NOTCHED_10, adventure.overlay());
        assertTrue(adventure.flags().contains(BossBar.Flag.DARKEN_SCREEN));

        bar.setTitle("Next");
        bar.setProgress(0.25);
        bar.setColor(org.bukkit.boss.BarColor.BLUE);
        bar.setStyle(org.bukkit.boss.BarStyle.SEGMENTED_20);
        bar.addFlag(org.bukkit.boss.BarFlag.PLAY_BOSS_MUSIC);
        bar.removeFlag(org.bukkit.boss.BarFlag.DARKEN_SKY);

        assertEquals("Next", plain(adventure));
        assertEquals(0.25f, adventure.progress());
        assertEquals(BossBar.Color.BLUE, adventure.color());
        assertEquals(BossBar.Overlay.NOTCHED_20, adventure.overlay());
        assertTrue(adventure.flags().contains(BossBar.Flag.PLAY_BOSS_MUSIC));
        assertFalse(adventure.flags().contains(BossBar.Flag.DARKEN_SCREEN));

        assertEquals("Next", bar.getTitle());
        assertEquals(0.25, bar.getProgress());
        assertEquals(org.bukkit.boss.BarColor.BLUE, bar.getColor());
        assertEquals(org.bukkit.boss.BarStyle.SEGMENTED_20, bar.getStyle());
        assertTrue(bar.hasFlag(org.bukkit.boss.BarFlag.PLAY_BOSS_MUSIC));
        assertFalse(bar.hasFlag(org.bukkit.boss.BarFlag.DARKEN_SKY));

        assertThrows(IllegalArgumentException.class, () -> bar.setProgress(1.01));
        assertThrows(IllegalArgumentException.class, () -> bar.setProgress(-0.01));
    }

    @Test void hidingAndViewerChangesOnlyAffectTheSelectedPlayers() {
        var bar = new FerriteBossBar(new NamespacedKey("ferrite_test", "boss"),
                "Boss", org.bukkit.boss.BarColor.WHITE, org.bukkit.boss.BarStyle.SOLID);
        var shown = new ArrayList<BossBar>();
        var first = viewer(shown);
        var second = viewer(shown);

        bar.addPlayer(first);
        bar.addPlayer(second);
        bar.addPlayer(second);
        assertEquals(2, shown.size(), "a viewer is not added twice");
        assertEquals(2, bar.getPlayers().size());

        bar.setVisible(false);
        assertTrue(shown.isEmpty());
        assertFalse(bar.isVisible());
        bar.addPlayer(first);
        assertTrue(shown.isEmpty(), "a hidden bar is not shown to a new viewer");

        bar.show();
        assertTrue(bar.isVisible());
        assertEquals(2, shown.size());
        bar.hide();
        assertTrue(shown.isEmpty());

        bar.show();
        assertEquals(2, shown.size(), "showing again restores every viewer");
        bar.removePlayer(first);
        assertEquals(1, shown.size());
        assertEquals(List.of(second), bar.getPlayers());
        bar.removePlayer(first);
        assertEquals(1, shown.size(), "removing an absent viewer is harmless");
        bar.removeAll();
        assertTrue(shown.isEmpty());
        assertTrue(bar.getPlayers().isEmpty());
    }
}
