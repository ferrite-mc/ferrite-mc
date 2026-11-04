package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.net.URI;
import org.bukkit.NamespacedKey;
import org.bukkit.ServerLinks;
import org.junit.jupiter.api.Test;

class FerriteServerStateTest {
    @Test
    void serverLinksSetReplacesTypedLink() {
        var links = new FerriteServerLinks();
        links.setLink(ServerLinks.Type.WEBSITE, URI.create("https://a.example"));
        links.setLink(ServerLinks.Type.WEBSITE, URI.create("https://b.example"));
        links.addLink("Docs", URI.create("https://docs.example"));
        assertEquals(2, links.getLinks().size());
        assertEquals("https://b.example", links.getLink(ServerLinks.Type.WEBSITE).getUrl().toString());
        assertEquals("Docs", links.getLinks().get(1).getDisplayName());
        assertTrue(links.removeLink(links.getLinks().get(1)));
        assertEquals(1, links.copy().getLinks().size());
    }

    @Test
    void tickManagerValidatesRateAndSteps() {
        var ticks = new FerriteTickManager();
        assertTrue(ticks.isRunningNormally());
        assertFalse(ticks.stepGameIfFrozen(5));
        ticks.setFrozen(true);
        assertTrue(ticks.stepGameIfFrozen(5));
        assertEquals(5, ticks.getFrozenTicksToRun());
        assertTrue(ticks.stopStepping());
        assertThrows(IllegalArgumentException.class, () -> ticks.setTickRate(0));
        ticks.setTickRate(40);
        assertEquals(40f, ticks.getTickRate());
    }

    @Test
    void pluginRecipesCanBeAddedAndRemoved() {
        FerriteRecipes.reset();
        var key = new NamespacedKey("test", "magic");
        var recipe = new FerriteRecipes.Entry(key, new org.bukkit.inventory.ItemStack(org.bukkit.Material.STICK, 2));
        assertTrue(FerriteRecipes.add(recipe));
        assertFalse(FerriteRecipes.add(recipe));
        assertSame(recipe, FerriteRecipes.get(key));
        assertTrue(FerriteRecipes.remove(key));
        assertNull(FerriteRecipes.get(key));
        FerriteRecipes.clear();
        assertTrue(FerriteRecipes.all().isEmpty());
        FerriteRecipes.reset();
    }
}
