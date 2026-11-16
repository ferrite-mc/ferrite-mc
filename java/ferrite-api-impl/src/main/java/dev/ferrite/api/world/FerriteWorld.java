package dev.ferrite.api.world;

import dev.ferrite.api.FerriteRuntime;
import dev.ferrite.api.block.FerriteBlock;
import dev.ferrite.api.internal.Unsupported;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;
import org.bukkit.GameRule;
import org.bukkit.Location;
import org.bukkit.World;
import org.bukkit.block.Block;
import org.bukkit.entity.Player;

/**
 * First {@link World} slice backed by an in-process {@link BlockStore}.
 *
 * <p>Time/weather/gamerule reads are facade state. Game-rule writes and world creation are
 * refused until Lane 1 provides the owner-region op; they never pretend to succeed.
 */
public final class FerriteWorld extends UnsupportedWorldBase {

    private final FerriteRuntime runtime;
    private final WorldSpec spec;
    private final BlockStore blocks;
    private final Map<String, Object> gameRules = new HashMap<>();

    private long time;
    private boolean storm;
    private boolean thundering;
    private int weatherDuration = 6000;
    private int thunderDuration = 6000;

    public FerriteWorld(FerriteRuntime runtime, WorldSpec spec) {
        this.runtime = runtime;
        this.spec = spec;
        this.blocks = new InMemoryBlockStore(spec.defaultBlock());
        this.time = spec.time();
        // String keys on purpose: the Paper API GameRule constants initialize
        // RegistryAccess, which only a CraftBukkit server installs. The facade must not
        // trip that initializer while building its own worlds.
        this.gameRules.put("doDaylightCycle", Boolean.TRUE);
        this.gameRules.put("doWeatherCycle", Boolean.TRUE);
        this.gameRules.put("doMobSpawning", Boolean.TRUE);
        this.gameRules.put("keepInventory", Boolean.FALSE);
    }

    /** The block backend; the host replaces it with an adapter over its region ops. */
    public BlockStore blockStore() {
        return blocks;
    }

    @Override
    public String getName() {
        return spec.name();
    }

    @Override
    public UUID getUID() {
        return spec.uniqueId();
    }

    @Override
    public World.Environment getEnvironment() {
        return spec.environment();
    }

    @Override
    public Location getSpawnLocation() {
        return new Location(this, spec.spawnX(), spec.spawnY(), spec.spawnZ());
    }

    @Override
    public Block getBlockAt(int x, int y, int z) {
        return new FerriteBlock(this, x, y, z);
    }

    @Override
    public List<Player> getPlayers() {
        return runtime.onlinePlayersIn(this);
    }

    @Override
    public List<org.bukkit.entity.Entity> getEntities() {
        return List.copyOf(runtime.onlinePlayersIn(this));
    }

    @Override
    public long getTime() {
        return time;
    }

    @Override
    public void setTime(long time) {
        this.time = time;
    }

    @Override
    public boolean hasStorm() {
        return storm;
    }

    @Override
    public void setStorm(boolean storm) {
        this.storm = storm;
    }

    @Override
    public boolean isThundering() {
        return thundering;
    }

    @Override
    public void setThundering(boolean thundering) {
        this.thundering = thundering;
    }

    @Override
    public int getWeatherDuration() {
        return weatherDuration;
    }

    @Override
    public void setWeatherDuration(int duration) {
        if (duration < 0) {
            throw new IllegalArgumentException("weather duration must be >= 0");
        }
        this.weatherDuration = duration;
    }

    @Override
    public int getThunderDuration() {
        return thunderDuration;
    }

    @Override
    public void setThunderDuration(int duration) {
        if (duration < 0) {
            throw new IllegalArgumentException("thunder duration must be >= 0");
        }
        this.thunderDuration = duration;
    }

    @Override
    public String[] getGameRules() {
        return gameRules.keySet().toArray(String[]::new);
    }

    @Override
    public String getGameRuleValue(String rule) {
        Object value = gameRules.get(rule);
        return value == null ? null : value.toString();
    }

    @Override
    @SuppressWarnings("unchecked")
    public <T> T getGameRuleValue(GameRule<T> rule) {
        Object value = gameRules.get(rule.getName());
        return value == null ? rule.getDefaultValue() : (T) value;
    }

    @Override
    public boolean setGameRuleValue(String rule, String value) {
        throw Unsupported.exception("World#setGameRuleValue");
    }

    @Override
    public boolean equals(Object object) {
        return object instanceof FerriteWorld other && other.spec.uniqueId().equals(spec.uniqueId());
    }

    @Override
    public int hashCode() {
        return spec.uniqueId().hashCode();
    }

    @Override
    public String toString() {
        return "FerriteWorld{" + spec.name() + "}";
    }
}
