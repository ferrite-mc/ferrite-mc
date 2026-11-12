package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.ArrayList;
import java.util.UUID;
import org.bukkit.Material;
import org.bukkit.map.MapView;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.Test;

/** A map view's id is the host's saved-map id, and a filled map keeps it on its way to the host. */
class FerriteMapIdTest {
    private static final UUID WORLD_ID = UUID.fromString("00000000-0000-0000-0000-0000000000ef");
    private static FerriteServer server;
    private static org.bukkit.World world;
    private static org.bukkit.entity.Player player;

    @BeforeAll static void start() throws Exception {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        server = (FerriteServer) org.bukkit.Bukkit.getServer();
        world = FerriteWorld.create(server, WORLD_ID, "map_id_world", 0L, 0, 64, 0);
        server.registerWorld(world);
        Bootstrap.registerPlayer("00000000-0000-0000-0000-0000000000ab", "Mapper", false);
        var ensure = Bootstrap.class.getDeclaredMethod("ensurePlayer", String.class, String.class);
        ensure.setAccessible(true);
        player = (org.bukkit.entity.Player) ensure.invoke(null, "00000000-0000-0000-0000-0000000000ab", "Mapper");
    }

    @AfterAll static void stop() { server.unregisterWorld(WORLD_ID); }

    @Test void createMapTakesItsIdFromTheHostAndWritesEverySettingBack() {
        var calls = new ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> {
            calls.add(method + "|" + args.replace(HostBridge.ARG, ','));
            return method.startsWith("allocateMap") ? "41" : "";
        };
        try {
            var view = server.createMap(world);
            assertEquals(41, view.getId(), "the id is the saved map's id, not a local counter");
            assertEquals(MapView.Scale.NORMAL, view.getScale());
            view.setScale(MapView.Scale.FAR);
            view.setCenterX(12);
            view.setCenterZ(-34);
            view.setLocked(true);
            view.setTrackingPosition(false);
            view.setUnlimitedTracking(true);
        } finally { HostBridge.transport = null; }
        assertEquals("allocateMap(String,int,int,int,boolean,boolean)|minecraft:overworld,2,0,0,true,false".replace("minecraft:overworld", world.getKey().toString()), calls.get(0));
        var last = calls.get(calls.size() - 1);
        assertEquals("updateMap(int,String,int,int,int,boolean,boolean,boolean)|41," + world.getKey() + ",3,12,-34,false,true,true", last);
        assertEquals(7, calls.size(), "one allocation and one update per setter");
    }

    @Test void aFilledMapReachesTheHostWithItsMapId() {
        var calls = new ArrayList<String>();
        HostBridge.transport = (owner, subject, method, args) -> {
            calls.add(method + "[" + args.replace(HostBridge.ARG, ',') + "]");
            return method.startsWith("allocateMap") ? "9" : "";
        };
        try {
            var view = server.createMap(world);
            var item = new org.bukkit.inventory.ItemStack(Material.FILLED_MAP);
            var meta = (org.bukkit.inventory.meta.MapMeta) item.getItemMeta();
            meta.setMapView(view);
            item.setItemMeta(meta);
            player.getInventory().setItem(2, item);
        } finally { HostBridge.transport = null; }
        assertTrue(calls.contains("setItemMapId(int,int)[2,9]"), calls.toString());
    }

    @Test void viewsWithoutAHostStillGetDistinctLocalIds() {
        var a = server.createMap(world);
        var b = server.createMap(world);
        assertNotEquals(a.getId(), b.getId());
    }
}
