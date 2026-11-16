package dev.ferrite.api.entity;

import dev.ferrite.api.world.FerriteWorld;
import java.util.List;
import java.util.Objects;
import java.util.UUID;
import java.util.concurrent.CopyOnWriteArrayList;
import org.bukkit.GameMode;

/** Mutable server-side state behind {@link FerritePlayer}. */
public final class PlayerState {

    private final String name;
    private final UUID uniqueId;
    private final List<String> messages = new CopyOnWriteArrayList<>();

    private boolean op;
    private GameMode gameMode;
    private double health = 20.0;
    private int foodLevel = 20;
    private FerriteWorld world;
    private double x;
    private double y;
    private double z;
    private float yaw;
    private float pitch;
    private volatile boolean online;
    private boolean kicked;
    private String kickMessage;
    private FerritePlayerInventory inventory;

    public PlayerState(
            String name,
            UUID uniqueId,
            boolean op,
            GameMode gameMode,
            FerriteWorld world,
            double x,
            double y,
            double z) {
        this.name = Objects.requireNonNull(name, "name");
        this.uniqueId = Objects.requireNonNull(uniqueId, "uniqueId");
        this.op = op;
        this.gameMode = Objects.requireNonNull(gameMode, "gameMode");
        this.world = Objects.requireNonNull(world, "world");
        this.x = x;
        this.y = y;
        this.z = z;
    }

    public String name() {
        return name;
    }

    public UUID uniqueId() {
        return uniqueId;
    }

    public boolean op() {
        return op;
    }

    public void setOp(boolean op) {
        this.op = op;
    }

    public GameMode gameMode() {
        return gameMode;
    }

    public void setGameMode(GameMode gameMode) {
        this.gameMode = Objects.requireNonNull(gameMode, "gameMode");
    }

    public double health() {
        return health;
    }

    public void setHealth(double health) {
        this.health = health;
    }

    public int foodLevel() {
        return foodLevel;
    }

    public void setFoodLevel(int foodLevel) {
        this.foodLevel = foodLevel;
    }

    public FerriteWorld world() {
        return world;
    }

    public void setWorld(FerriteWorld world) {
        this.world = Objects.requireNonNull(world, "world");
    }

    public double x() {
        return x;
    }

    public double y() {
        return y;
    }

    public double z() {
        return z;
    }

    public void setPosition(double x, double y, double z, float yaw, float pitch) {
        this.x = x;
        this.y = y;
        this.z = z;
        this.yaw = yaw;
        this.pitch = pitch;
    }

    public float yaw() {
        return yaw;
    }

    public float pitch() {
        return pitch;
    }

    public boolean online() {
        return online;
    }

    public void setOnline(boolean online) {
        this.online = online;
    }

    public boolean kicked() {
        return kicked;
    }

    public String kickMessage() {
        return kickMessage;
    }

    public void kick(String message) {
        this.kicked = true;
        this.kickMessage = message;
    }

    public List<String> messages() {
        return messages;
    }

    public FerritePlayerInventory inventory() {
        return inventory;
    }

    void setInventory(FerritePlayerInventory inventory) {
        this.inventory = inventory;
    }
}
