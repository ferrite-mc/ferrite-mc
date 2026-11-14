package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.ArrayList;
import java.util.List;
import org.bukkit.boss.DragonBattle;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

class FerriteDragonBattleTest {
    private static final String END_ID = "00000000-0000-0000-0000-0000000000e7";
    private static final String OVERWORLD_ID = "00000000-0000-0000-0000-0000000000e8";
    private final List<String> calls = new ArrayList<>();
    private String state = "true,true,3,false,none,64";
    private org.bukkit.World end;

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }

    @BeforeEach void install() {
        for (var name : new String[]{"battle_world_the_end", "battle_world"}) {
            var existing = org.bukkit.Bukkit.getWorld(name);
            if (existing != null) ((FerriteServer) org.bukkit.Bukkit.getServer()).unregisterWorld(existing.getUID());
        }
        Bootstrap.registerWorld(END_ID, "battle_world_the_end", 1, 0, 64, 0);
        Bootstrap.registerWorld(OVERWORLD_ID, "battle_world", 1, 0, 64, 0);
        end = org.bukkit.Bukkit.getWorld("battle_world_the_end");
        HostBridge.transport = (owner, subject, method, args) -> {
            if (!owner.equals("World") || !subject.equals(END_ID)) return null;
            calls.add(method + "|" + args);
            return method.equals("dragonBattle()") ? state : "";
        };
    }

    @AfterEach void reset() { HostBridge.transport = null; }

    @Test void onlyTheEndHasABattle() {
        assertNull(org.bukkit.Bukkit.getWorld("battle_world").getEnderDragonBattle());
        assertNotNull(end.getEnderDragonBattle());
        assertSame(end.getEnderDragonBattle(), end.getEnderDragonBattle());
    }

    @Test void theBattleReadsTheHostSnapshot() {
        DragonBattle battle = end.getEnderDragonBattle();
        assertTrue(battle.hasBeenPreviouslyKilled());
        assertEquals(3, battle.getGatewayCount());
        assertEquals(DragonBattle.RespawnPhase.NONE, battle.getRespawnPhase());
        assertEquals(64, battle.getEndPortalLocation().getBlockY());
        state = "true,false,0,true,summoning_pillars,-";
        assertEquals(DragonBattle.RespawnPhase.SUMMONING_PILLARS, battle.getRespawnPhase());
        assertNull(battle.getEndPortalLocation());
        assertNotNull(battle.getBossBar());
    }

    @Test void changesGoToTheHostAndRespawnNeedsAKilledDragonAndNoSequence() {
        DragonBattle battle = end.getEnderDragonBattle();
        battle.setPreviouslyKilled(false);
        assertTrue(calls.contains("dragonBattleSetKilled(boolean)|false"), calls.toString());
        assertTrue(battle.setRespawnPhase(DragonBattle.RespawnPhase.SUMMONING_DRAGON));
        assertTrue(calls.contains("dragonBattleSetPhase(String)|summoning_dragon"), calls.toString());
        calls.clear();
        assertTrue(battle.initiateRespawn(List.of()));
        assertEquals(List.of("dragonBattle()|", "dragonBattleInitiateRespawn()|"), calls);
        state = "true,false,0,false,none,64";
        calls.clear();
        assertFalse(battle.initiateRespawn(List.of()), "no dragon has died");
        assertFalse(calls.contains("dragonBattleInitiateRespawn()|"));
        state = "true,true,20,false,none,64";
        assertFalse(battle.spawnNewGateway(), "the ring is complete");
        battle.resetCrystals();
        assertTrue(calls.contains("dragonBattleResetCrystals()|"), calls.toString());
        battle.spawnNewGateway(io.papermc.paper.math.Position.block(5, 70, -3));
        assertTrue(calls.stream().anyMatch(c -> c.startsWith("dragonBattleSpawnGatewayAt(int,int,int)|") && c.contains("70")), calls.toString());
    }
}
