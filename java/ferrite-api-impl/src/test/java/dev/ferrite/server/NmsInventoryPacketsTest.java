package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.UUID;
import net.minecraft.network.chat.Component;
import net.minecraft.network.protocol.game.ClientboundContainerClosePacket;
import net.minecraft.network.protocol.game.ClientboundContainerSetContentPacket;
import net.minecraft.network.protocol.game.ClientboundContainerSetSlotPacket;
import net.minecraft.network.protocol.game.ClientboundOpenScreenPacket;
import net.minecraft.world.inventory.MenuType;
import net.minecraft.world.item.ItemStack;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class NmsInventoryPacketsTest {
    private FerriteServer server;
    private UUID id;
    private org.bukkit.entity.Player player;
    private List<String> calls;

    @BeforeEach
    void startServer() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
        server = (FerriteServer) org.bukkit.Bukkit.getServer();
        id = UUID.randomUUID();
        player = FerritePlayer.create(server, id, "NmsInventory", false, 20.0, 20, 0, null);
        server.registerPlayer(player);
        calls = new ArrayList<>();
        HostBridge.transport = (owner, subject, method, args) -> {
            calls.add(owner + "|" + subject + "|" + method + "|" + args);
            return method.startsWith("send(") ? null : "";
        };
    }

    @AfterEach
    void stopServer() {
        HostBridge.transport = null;
        server.unregisterPlayer(id);
    }

    @Test
    void aPluginShulkerBoxInventoryIsSentToTheHostAsAShulkerBox() {
        var inventory = server.createInventory(null, org.bukkit.event.inventory.InventoryType.SHULKER_BOX, "Loot");
        inventory.setItem(26, bukkit(org.bukkit.Material.EMERALD, 5));
        player.openInventory(inventory);
        var call = calls.stream().filter(c -> c.contains("|openInventory(Inventory)|")).findFirst().orElseThrow(() -> new AssertionError(calls.toString()));
        var wire = call.substring(call.indexOf("|openInventory(Inventory)|") + "|openInventory(Inventory)|".length()).split("\u001e", -1);
        assertEquals("Loot", wire[0]);
        assertEquals("27", wire[1]);
        assertEquals("SHULKER_BOX", wire[2]);
        assertEquals("EMERALD\u001d5", wire[2 + 26 + 1]);
        assertEquals(3 + 27, wire.length);
    }

    @Test
    void opensSynchronizesAndClosesOneChestScreen() {
        var connection = ((org.bukkit.craftbukkit.entity.CraftPlayer) player).getHandle().connection;
        connection.send(new ClientboundOpenScreenPacket(7, MenuType.GENERIC_9x3, Component.literal("Shop")));

        var view = player.getOpenInventory();
        assertEquals(27, view.getTopInventory().getSize());
        assertEquals("Shop", view.getTitle());
        assertTrue(calls.getFirst().contains("|openInventory(Inventory)|Shop\u001e27\u001eCHEST"), calls.getFirst());

        List<ItemStack> contents = new ArrayList<>(Collections.nCopies(27 + 36, ItemStack.EMPTY));
        contents.set(0, ItemStack.of(bukkit(org.bukkit.Material.DIAMOND, 3)));
        contents.set(27, ItemStack.of(bukkit(org.bukkit.Material.STONE, 2)));
        connection.send(new ClientboundContainerSetContentPacket(7, 1, contents, ItemStack.of(bukkit(org.bukkit.Material.EMERALD, 1))));

        assertEquals(org.bukkit.Material.DIAMOND, view.getTopInventory().getItem(0).getType());
        assertEquals(3, view.getTopInventory().getItem(0).getAmount());
        assertEquals(org.bukkit.Material.STONE, view.getItem(27).getType());
        assertEquals(org.bukkit.Material.EMERALD, view.getCursor().getType());
        assertTrue(calls.stream().anyMatch(call -> call.endsWith("|updateInventory()|")), calls.toString());

        connection.send(new ClientboundContainerSetSlotPacket(7, 2, 1, ItemStack.of(bukkit(org.bukkit.Material.GOLD_INGOT, 4))));
        assertEquals(org.bukkit.Material.GOLD_INGOT, view.getTopInventory().getItem(1).getType());
        assertEquals(4, view.getTopInventory().getItem(1).getAmount());

        connection.send(new ClientboundContainerSetSlotPacket(7, 3, -1, ItemStack.of(bukkit(org.bukkit.Material.APPLE, 1))));
        assertEquals(org.bukkit.Material.APPLE, view.getCursor().getType());

        // Packets that arrive out of order with an older state id are stale, not applied.
        List<ItemStack> staleContents = new ArrayList<>(Collections.nCopies(27 + 36, ItemStack.EMPTY));
        staleContents.set(0, ItemStack.of(bukkit(org.bukkit.Material.COAL, 9)));
        connection.send(new ClientboundContainerSetContentPacket(7, 1, staleContents, ItemStack.EMPTY));
        assertEquals(org.bukkit.Material.DIAMOND, view.getTopInventory().getItem(0).getType());
        assertEquals(3, view.getTopInventory().getItem(0).getAmount());
        connection.send(new ClientboundContainerSetSlotPacket(7, 1, 1, ItemStack.of(bukkit(org.bukkit.Material.IRON_INGOT, 2))));
        assertEquals(org.bukkit.Material.GOLD_INGOT, view.getTopInventory().getItem(1).getType());
        assertEquals(4, view.getTopInventory().getItem(1).getAmount());

        connection.send(new ClientboundContainerClosePacket(7));
        assertTrue(calls.stream().anyMatch(call -> call.contains("|closeInventory(Reason)|UNKNOWN")), calls.toString());
        assertNotSame(view, player.getOpenInventory());
    }

    @Test
    void unsupportedMenusAndUnknownContainerIdsFailClearly() {
        var connection = ((org.bukkit.craftbukkit.entity.CraftPlayer) player).getHandle().connection;

        var unsupportedMenu = assertThrows(UnsupportedOperationException.class,
                () -> connection.send(new ClientboundOpenScreenPacket(8, MenuType.HOPPER, Component.literal("Hopper"))));
        assertTrue(unsupportedMenu.getMessage().contains("HOPPER"), unsupportedMenu.getMessage());

        connection.send(new ClientboundOpenScreenPacket(5, MenuType.SHULKER_BOX, Component.literal("Box")));
        connection.send(new ClientboundContainerClosePacket(5));

        assertThrows(IllegalArgumentException.class,
                () -> connection.send(new ClientboundOpenScreenPacket(0, MenuType.GENERIC_9x1, Component.literal("Bad"))));
        assertThrows(IllegalStateException.class,
                () -> connection.send(new ClientboundContainerSetContentPacket(7, 0, List.of(), ItemStack.EMPTY)));
        assertThrows(IllegalStateException.class,
                () -> connection.send(new ClientboundContainerClosePacket(7)));

        connection.send(new ClientboundOpenScreenPacket(9, MenuType.GENERIC_9x1, Component.literal("Tiny")));
        assertThrows(IllegalArgumentException.class,
                () -> connection.send(new ClientboundContainerSetContentPacket(9, 0, List.of(ItemStack.EMPTY), ItemStack.EMPTY)));
        connection.send(new ClientboundContainerClosePacket(9));

        connection.send(new ClientboundOpenScreenPacket(10, MenuType.GENERIC_9x1, Component.literal("Stale")));
        player.closeInventory();
        assertThrows(IllegalStateException.class,
                () -> connection.send(new ClientboundContainerSetContentPacket(10, 0, List.of(), ItemStack.EMPTY)));

        var unsupportedPacket = assertThrows(UnsupportedOperationException.class, () -> connection.send(new UnsupportedPacket()));
        assertTrue(unsupportedPacket.getMessage().contains("NMS#send(UnsupportedPacket)"), unsupportedPacket.getMessage());
    }

    private static org.bukkit.inventory.ItemStack bukkit(org.bukkit.Material material, int amount) {
        return new org.bukkit.inventory.ItemStack(material, amount);
    }

    private static final class UnsupportedPacket implements net.minecraft.network.protocol.Packet<Object> {}
}
