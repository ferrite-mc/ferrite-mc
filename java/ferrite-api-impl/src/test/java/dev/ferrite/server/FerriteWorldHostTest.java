package dev.ferrite.server;

import static org.junit.jupiter.api.Assertions.*;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import org.bukkit.Difficulty;
import org.bukkit.Location;
import org.junit.jupiter.api.AfterAll;
import org.junit.jupiter.api.AfterEach;
import org.junit.jupiter.api.BeforeAll;
import org.junit.jupiter.api.BeforeEach;
import org.junit.jupiter.api.Test;

/**
 * World queries and setters the host answers: each test replies with the text the Rust host sends and checks the
 * value the plugin sees, and the signature strings are the ones {@code crates/ferrite-server/src/plugins} matches.
 */
class FerriteWorldHostTest {
    private static final String WORLD_ID = "00000000-0000-0000-0000-0000000000e5";
    private static org.bukkit.World world;
    private final Map<String, String> replies = new HashMap<>();
    private final List<String> calls = new ArrayList<>();

    @BeforeAll static void start() {
        System.setProperty("ferrite.native", "false");
        Bootstrap.configureDataDir(System.getProperty("test.vanilla.data"));
        Bootstrap.startServer();
    }

    @AfterAll static void stop() { unregister(); }

    private static void unregister() {
        var existing = org.bukkit.Bukkit.getWorld("host_world");
        if (existing != null) ((FerriteServer) org.bukkit.Bukkit.getServer()).unregisterWorld(existing.getUID());
    }

    @BeforeEach void install() {
        // a fresh world per test: the spawn and the remembered properties are per-world state
        unregister();
        Bootstrap.registerWorld(WORLD_ID, "host_world", 1234, 3, 64, -7);
        world = org.bukkit.Bukkit.getWorld("host_world");
        HostBridge.clearLocalProperties();
        HostBridge.transport = (owner, subject, method, args) -> {
            if (!owner.equals("World") || !subject.equals(WORLD_ID)) return null;
            calls.add(method + "|" + args);
            return replies.get(method);
        };
    }

    @AfterEach void reset() { HostBridge.transport = null; HostBridge.clearLocalProperties(); }

    private String lastCall() { return calls.get(calls.size() - 1); }

    @Test void spawnArrowShootsAlongTheNormalisedDirectionAtTheGivenSpeed() {
        replies.put("spawnArrow(Location,double,double,double,String)", "");
        var at = new Location(world, 1.5, 70, 2.5);
        var arrow = world.spawnArrow(at, new org.bukkit.util.Vector(0, 0, 4), 2.0f, 0.0f, org.bukkit.entity.Arrow.class);
        assertEquals(org.bukkit.entity.EntityType.ARROW, arrow.getType());
        assertSame(world, arrow.getWorld());
        var call = calls.stream().filter(c -> c.startsWith("spawnArrow(Location,double,double,double,String)|")).findFirst().orElseThrow();
        var args = call.substring(call.indexOf('|') + 1);
        var fields = args.split("[\\x00-\\x1f?]");
        var tail = java.util.Arrays.copyOfRange(fields, fields.length - 4, fields.length);
        assertArrayEquals(new String[] {"0.0", "0.0", "2.0", arrow.getUniqueId().toString()}, tail, args);
        assertThrows(IllegalArgumentException.class, () -> world.spawnArrow(at, new org.bukkit.util.Vector(), 1f, 0f, org.bukkit.entity.Arrow.class));
        var refused = assertThrows(UnsupportedOperationException.class, () -> world.spawnArrow(at, new org.bukkit.util.Vector(0, 1, 0), 1f, 0f, org.bukkit.entity.SpectralArrow.class));
        assertTrue(refused.getMessage().contains("plain arrows"), refused.getMessage());
    }

    @Test void infiniburnIsTheDimensionTypesBlockTag() {
        var burn = world.getInfiniburn();
        assertTrue(burn.contains(org.bukkit.Material.NETHERRACK), burn.toString());
        assertTrue(burn.contains(org.bukkit.Material.MAGMA_BLOCK), burn.toString());
        assertFalse(burn.contains(org.bukkit.Material.STONE));
    }

    // ---- time, weather, difficulty ------------------------------------------------------------------------------

    @Test void timeComesFromTheHostAndDecidesDayTime() {
        replies.put("getTime()", "6000");
        replies.put("getFullTime()", "54321");
        replies.put("getGameTime()", "99999");
        assertEquals(6000, world.getTime());
        assertEquals(54321, world.getFullTime());
        assertEquals(99999, world.getGameTime());
        assertTrue(world.isDayTime());
        replies.put("getTime()", "13000");
        assertFalse(world.isDayTime());
    }

    @Test void weatherAndDifficultyComeFromTheHost() {
        replies.put("hasStorm()", "true");
        replies.put("isThundering()", "false");
        replies.put("isClearWeather()", "false");
        replies.put("getWeatherDuration()", "1200");
        replies.put("getThunderDuration()", "300");
        replies.put("getClearWeatherDuration()", "0");
        replies.put("getDifficulty()", "HARD");
        assertTrue(world.hasStorm());
        assertFalse(world.isThundering());
        assertFalse(world.isClearWeather());
        assertEquals(1200, world.getWeatherDuration());
        assertEquals(300, world.getThunderDuration());
        assertEquals(0, world.getClearWeatherDuration());
        assertEquals(Difficulty.HARD, world.getDifficulty());
    }

    @Test void moonPhaseFollowsTheHostsCount() {
        replies.put("getMoonPhase()", "4");
        assertEquals(io.papermc.paper.world.MoonPhase.NEW_MOON, world.getMoonPhase());
        replies.put("getMoonPhase()", "11");
        assertEquals(io.papermc.paper.world.MoonPhase.WANING_CRESCENT, world.getMoonPhase(), "11 % 8 = 3");
    }

    @Test void moonPhaseWithoutAHostIsTheDayCountOfTheRegisteredTime() {
        unregister();
        Bootstrap.registerWorld(WORLD_ID, "host_world", 24_000L * 11 + 100, 0, 64, 0);
        var later = org.bukkit.Bukkit.getWorld("host_world");
        HostBridge.transport = null;
        assertEquals(io.papermc.paper.world.MoonPhase.WANING_CRESCENT, later.getMoonPhase(), "day 11 % 8 = 3");
        assertEquals(io.papermc.paper.world.MoonPhase.FULL_MOON, world.getMoonPhase(), "the 1234-tick world is on day 0");
    }

    @Test void settersReachTheHostWithTheSignaturesItMatches() {
        replies.put("setTime(long)", "true");
        replies.put("setFullTime(long)", "true");
        replies.put("setStorm(boolean)", "true");
        replies.put("setThundering(boolean)", "true");
        replies.put("setDifficulty(Difficulty)", "true");
        replies.put("setWeatherDuration(int)", "true");
        replies.put("setThunderDuration(int)", "true");
        replies.put("setClearWeatherDuration(int)", "true");
        world.setTime(18000);
        assertEquals("setTime(long)|18000", lastCall());
        world.setFullTime(48000);
        assertEquals("setFullTime(long)|48000", lastCall());
        world.setStorm(true);
        assertEquals("setStorm(boolean)|true", lastCall());
        world.setThundering(true);
        assertEquals("setThundering(boolean)|true", lastCall());
        world.setDifficulty(Difficulty.PEACEFUL);
        assertEquals("setDifficulty(Difficulty)|PEACEFUL", lastCall());
        world.setWeatherDuration(40);
        assertEquals("setWeatherDuration(int)|40", lastCall());
        world.setThunderDuration(50);
        assertEquals("setThunderDuration(int)|50", lastCall());
        world.setClearWeatherDuration(60);
        assertEquals("setClearWeatherDuration(int)|60", lastCall());
    }

    @Test void withoutAHostAnUnsetQueryFailsInsteadOfInventingWeather() {
        HostBridge.transport = null;
        var storm = assertThrows(UnsupportedOperationException.class, world::hasStorm);
        assertTrue(storm.getMessage().contains("World#hasStorm()"), storm.getMessage());
        assertThrows(UnsupportedOperationException.class, world::getDifficulty);
        assertThrows(UnsupportedOperationException.class, world::isThundering);
        // the registered time is the only snapshot Java keeps, so it still answers
        assertEquals(1234, world.getTime());
    }

    // ---- spawn ----------------------------------------------------------------------------------------------------

    @Test void spawnStartsAtTheRegisteredPositionAndFollowsSetSpawnLocation() {
        var start = world.getSpawnLocation();
        assertEquals(3, start.getBlockX());
        assertEquals(64, start.getBlockY());
        assertEquals(-7, start.getBlockZ());
        assertEquals(0f, start.getYaw());

        replies.put("setSpawnLocation(int,int,int,float)", "true");
        assertTrue(world.setSpawnLocation(10, 70, -5, 90f));
        assertEquals("setSpawnLocation(int,int,int,float)|10" + HostBridge.ARG + "70" + HostBridge.ARG + "-5" + HostBridge.ARG + "90.0", lastCall());
        var moved = world.getSpawnLocation();
        assertEquals(10, moved.getBlockX());
        assertEquals(70, moved.getBlockY());
        assertEquals(-5, moved.getBlockZ());
        assertEquals(90f, moved.getYaw());
        assertSame(world, moved.getWorld());

        assertTrue(world.setSpawnLocation(new Location(world, 1.9, 65.2, 2.1, 45f, 10f)));
        assertEquals("setSpawnLocation(int,int,int,float)|1" + HostBridge.ARG + "65" + HostBridge.ARG + "2" + HostBridge.ARG + "45.0", lastCall());
        assertEquals(45f, world.getSpawnLocation().getYaw());

        assertTrue(world.setSpawnLocation(8, 80, 9));
        assertEquals("setSpawnLocation(int,int,int,float)|8" + HostBridge.ARG + "80" + HostBridge.ARG + "9" + HostBridge.ARG + "0.0", lastCall());
        assertEquals(0f, world.getSpawnLocation().getYaw());
        assertEquals(80, world.getSpawnLocation().getBlockY());
    }

    @Test void aRefusedSpawnMoveLeavesTheSpawnAlone() {
        replies.put("setSpawnLocation(int,int,int,float)", "true");
        assertTrue(world.setSpawnLocation(20, 60, 20, 10f));
        replies.put("setSpawnLocation(int,int,int,float)", "false");
        assertFalse(world.setSpawnLocation(99, 99, 99, 99f));
        assertEquals(20, world.getSpawnLocation().getBlockX());
        assertEquals(10f, world.getSpawnLocation().getYaw());
    }

    // ---- world border -----------------------------------------------------------------------------------------------

    @Test void borderReadsTheHostsBorder() {
        replies.put("borderSize()", "100.0");
        replies.put("borderCenterX()", "1.5");
        replies.put("borderCenterZ()", "-2.5");
        replies.put("borderDamageBuffer()", "5.0");
        replies.put("borderDamageAmount()", "0.2");
        replies.put("borderWarningTicks()", "300");
        replies.put("borderWarningDistance()", "5");
        var border = world.getWorldBorder();
        assertSame(world, border.getWorld());
        assertEquals(100.0, border.getSize());
        var center = border.getCenter();
        assertEquals(1.5, center.getX());
        assertEquals(-2.5, center.getZ());
        assertEquals(5.0, border.getDamageBuffer());
        assertEquals(0.2, border.getDamageAmount());
        assertEquals(300, border.getWarningTimeTicks());
        assertEquals(5, border.getWarningDistance());
        assertTrue(border.isInside(new Location(world, 51.5, 64, 47.5)));
        assertFalse(border.isInside(new Location(world, 52.0, 64, -2.5)));
        assertEquals(5.9999968E7, border.getMaxSize());
        assertEquals(2.9999984E7, border.getMaxCenterCoordinate());
    }

    @Test void borderWritesUseTheHostsBorderCalls() {
        replies.put("borderSet(double,long)", "true");
        replies.put("borderCenter(double,double)", "true");
        replies.put("borderDamageBuffer(double)", "true");
        replies.put("borderDamageAmount(double)", "true");
        replies.put("borderWarningTime(int)", "true");
        replies.put("borderWarningDistance(int)", "true");
        var border = world.getWorldBorder();
        border.setSize(200.0);
        assertEquals("borderSet(double,long)|200.0" + HostBridge.ARG + "0", lastCall());
        border.changeSize(50.0, 100);
        assertEquals("borderSet(double,long)|50.0" + HostBridge.ARG + "100", lastCall());
        border.setCenter(3.0, 4.0);
        assertEquals("borderCenter(double,double)|3.0" + HostBridge.ARG + "4.0", lastCall());
        border.setCenter(new Location(world, 5.0, 64, 6.0));
        assertEquals("borderCenter(double,double)|5.0" + HostBridge.ARG + "6.0", lastCall());
        border.setDamageBuffer(2.0);
        assertEquals("borderDamageBuffer(double)|2.0", lastCall());
        border.setDamageAmount(1.5);
        assertEquals("borderDamageAmount(double)|1.5", lastCall());
        border.setWarningTimeTicks(60);
        assertEquals("borderWarningTime(int)|60", lastCall());
        border.setWarningDistance(7);
        assertEquals("borderWarningDistance(int)|7", lastCall());

        calls.clear();
        border.reset();
        assertEquals(List.of(
                "borderSet(double,long)|5.9999968E7" + HostBridge.ARG + "0",
                "borderCenter(double,double)|0.0" + HostBridge.ARG + "0.0",
                "borderDamageBuffer(double)|5.0",
                "borderDamageAmount(double)|0.2",
                "borderWarningTime(int)|300",
                "borderWarningDistance(int)|5"), calls);
    }

    @Test void borderRejectsValuesTheHostCannotHold() {
        var border = world.getWorldBorder();
        assertThrows(IllegalArgumentException.class, () -> border.setSize(0.5));
        assertThrows(IllegalArgumentException.class, () -> border.setSize(6.0E7));
        assertThrows(IllegalArgumentException.class, () -> border.setSize(Double.NaN));
        assertThrows(IllegalArgumentException.class, () -> border.setCenter(3.0E7, 0));
        assertThrows(IllegalArgumentException.class, () -> border.setCenter(0, -3.0E7));
        assertTrue(calls.isEmpty(), "rejected values never reach the host");
    }

    @Test void spawnMapsTheClassToItsEntityTypeAndRunsTheConsumerOnTheHandle() {
        replies.put("spawnEntity(Location,EntityType,String)", "");
        var seen = new ArrayList<org.bukkit.entity.Entity>();
        var spawned = world.spawn(new Location(world, 1, 64, 2), org.bukkit.entity.Zombie.class, seen::add);
        assertEquals(org.bukkit.entity.EntityType.ZOMBIE, spawned.getType());
        assertEquals(List.of(spawned), seen);
        assertTrue(calls.stream().anyMatch(c -> c.startsWith("spawnEntity(Location,EntityType,String)")), calls.toString());
        // Classes that more than one entity type shares cannot be spawned.
        assertThrows(IllegalArgumentException.class, () -> world.spawn(new Location(world, 0, 64, 0), org.bukkit.entity.Monster.class));
        assertThrows(IllegalArgumentException.class, () -> world.spawn(new Location(world, 0, 64, 0), org.bukkit.entity.Player.class));
    }

    @Test void createEntityKeepsTheEntityOutOfTheWorldUntilAddEntity() {
        replies.put("spawnEntity(Location,EntityType,String)", "");
        var created = world.createEntity(new Location(world, 3, 64, 3), org.bukkit.entity.Cow.class);
        assertEquals(org.bukkit.entity.EntityType.COW, created.getType());
        assertTrue(calls.stream().noneMatch(c -> c.startsWith("spawnEntity")), "not spawned yet: " + calls);
        assertSame(created, world.addEntity(created));
        assertTrue(calls.stream().anyMatch(c -> c.startsWith("spawnEntity(Location,EntityType,String)")), calls.toString());
        // The same handle cannot be added twice, and a foreign entity is refused.
        assertThrows(IllegalArgumentException.class, () -> world.addEntity(created));
        var other = world.spawn(new Location(world, 0, 64, 0), org.bukkit.entity.Cow.class);
        assertThrows(IllegalArgumentException.class, () -> world.addEntity(other));
    }

    private String explosionCall() {
        return calls.stream().filter(c -> c.startsWith("createExplosion(Location,float,boolean,boolean)|")).reduce((first, second) -> second).orElseThrow();
    }

    @Test void everyExplosionOverloadBecomesOneHostCallWithTheBukkitDefaults() {
        replies.put("createExplosion(Location,float,boolean,boolean)", "true");
        replies.put("spawnEntity(Location,EntityType,String)", "");
        var sep = String.valueOf(HostBridge.ARG);
        var at = new Location(world, 1, 64, 2);
        assertTrue(world.createExplosion(at, 3f));
        assertTrue(explosionCall().endsWith(sep + "3.0" + sep + "false" + sep + "true"), explosionCall());
        assertTrue(world.createExplosion(at, 3f, true));
        assertTrue(explosionCall().endsWith(sep + "3.0" + sep + "true" + sep + "true"), explosionCall());
        assertTrue(world.createExplosion(at, 3f, true, false));
        assertTrue(explosionCall().endsWith(sep + "3.0" + sep + "true" + sep + "false"), explosionCall());
        assertTrue(world.createExplosion(1.0, 64.0, 2.0, 4f, false, false));
        assertTrue(explosionCall().endsWith(sep + "4.0" + sep + "false" + sep + "false"), explosionCall());
        var source = world.spawn(new Location(world, 9, 70, 9), org.bukkit.entity.Zombie.class);
        assertTrue(world.createExplosion(source, 2f));
        assertTrue(explosionCall().contains("9.0"), "centred on the source entity: " + explosionCall());
        assertTrue(world.createExplosion(source, at, 2f, false, true, true));
        assertTrue(explosionCall().contains("1.0") && explosionCall().endsWith(sep + "2.0" + sep + "false" + sep + "true"), explosionCall());
        Bootstrap.registerWorld("00000000-0000-0000-0000-0000000000e6", "host_world_two", 1, 0, 64, 0);
        var other = org.bukkit.Bukkit.getWorld("host_world_two");
        try {
            assertThrows(IllegalArgumentException.class, () -> world.createExplosion(new Location(other, 0, 64, 0), 2f));
        } finally { ((FerriteServer) org.bukkit.Bukkit.getServer()).unregisterWorld(other.getUID()); }
    }
}
