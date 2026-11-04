package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.ArrayList;
import java.util.UUID;
import net.kyori.adventure.text.Component;
import org.bukkit.Material;
import org.bukkit.map.MapRenderer;
import org.bukkit.map.MapView;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

class FerriteMerchantMapTest {
    private static final UUID WORLD_ID = UUID.fromString("00000000-0000-0000-0000-0000000000ee");
    private static FerriteServer server;
    private static org.bukkit.World world;

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        server = (FerriteServer) org.bukkit.Bukkit.getServer();
        world = FerriteWorld.create(server, WORLD_ID, "merchant_map_world", 0L, 0, 64, 0);
        server.registerWorld(world);
    }

    @AfterAll static void stop() { server.unregisterWorld(WORLD_ID); }

    @Test
    void merchantRecipesRoundTripAndCopiesAreDefensive() {
        var merchant = server.createMerchant();
        assertTrue(merchant.getRecipes().isEmpty());
        assertEquals(0, merchant.getRecipeCount());
        assertFalse(merchant.isTrading());
        assertNull(merchant.getTrader());

        var first = new org.bukkit.inventory.MerchantRecipe(new org.bukkit.inventory.ItemStack(Material.EMERALD, 3), 8);
        first.addIngredient(new org.bukkit.inventory.ItemStack(Material.WHEAT, 20));
        var input = new ArrayList<org.bukkit.inventory.MerchantRecipe>();
        input.add(first);
        merchant.setRecipes(input);
        assertEquals(1, merchant.getRecipeCount());
        assertSame(first, merchant.getRecipe(0));

        input.add(first);
        assertEquals(1, merchant.getRecipeCount(), "setRecipes copies the caller's list");
        var view = merchant.getRecipes();
        assertEquals(1, view.size());
        assertThrows(UnsupportedOperationException.class, () -> view.add(first), "getRecipes returns a copy");

        var second = new org.bukkit.inventory.MerchantRecipe(new org.bukkit.inventory.ItemStack(Material.DIAMOND, 1), 4);
        merchant.setRecipe(0, second);
        assertSame(second, merchant.getRecipe(0));
        assertEquals(Material.DIAMOND, merchant.getRecipe(0).getResult().getType());
        assertThrows(IndexOutOfBoundsException.class, () -> merchant.getRecipe(5));
        assertThrows(IndexOutOfBoundsException.class, () -> merchant.setRecipe(5, first));
    }

    @Test
    void merchantKeepsComponentAndStringTitles() {
        var title = Component.text("Component Shop");
        var fromComponent = (FerriteMerchant) server.createMerchant(title);
        assertSame(title, fromComponent.title());
        assertEquals("Component Shop", fromComponent.getTitle());

        var fromString = (FerriteMerchant) server.createMerchant("String Shop");
        assertEquals(Component.text("String Shop"), fromString.title());
        assertEquals("String Shop", fromString.getTitle());

        assertNull(((FerriteMerchant) server.createMerchant()).title());
        assertThrows(NullPointerException.class, () -> server.createMerchant((String) null));
        assertThrows(NullPointerException.class, () -> server.createMerchant((Component) null));
    }

    @Test
    void mapViewsGetIncreasingIdsAndAreLookedUpByTheSameInstance() {
        var first = server.createMap(world);
        var second = server.createMap(world);
        assertTrue(first.getId() >= 1);
        assertEquals(first.getId() + 1, second.getId());
        assertSame(first, server.getMap(first.getId()));
        assertSame(second, server.getMap(second.getId()));
        assertNull(server.getMap(-12345));

        assertEquals(MapView.Scale.NORMAL, first.getScale());
        assertSame(world, first.getWorld());
        assertEquals(0, first.getCenterX());
        assertEquals(0, first.getCenterZ());
        assertFalse(first.isVirtual());
        assertFalse(first.isLocked());
        assertTrue(first.isTrackingPosition());
        assertFalse(first.isUnlimitedTracking());

        first.setScale(MapView.Scale.FAR);
        first.setCenterX(12);
        first.setCenterZ(-34);
        first.setLocked(true);
        assertEquals(MapView.Scale.FAR, first.getScale());
        assertEquals(12, first.getCenterX());
        assertEquals(-34, first.getCenterZ());
        assertTrue(first.isLocked());
        assertEquals(MapView.Scale.NORMAL, second.getScale(), "views keep separate state");
    }

    @Test
    void mapRenderersCanBeAddedAndRemoved() {
        var view = server.createMap(world);
        assertTrue(view.getRenderers().isEmpty());
        var renderer = new MapRenderer() {
            @Override public void render(MapView map, org.bukkit.map.MapCanvas canvas, org.bukkit.entity.Player player) {}
        };
        view.addRenderer(renderer);
        assertEquals(1, view.getRenderers().size());
        assertSame(renderer, view.getRenderers().get(0));
        assertTrue(view.removeRenderer(renderer));
        assertFalse(view.removeRenderer(renderer));
        assertTrue(view.getRenderers().isEmpty());
    }
}
