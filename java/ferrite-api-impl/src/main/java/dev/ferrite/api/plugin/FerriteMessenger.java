package dev.ferrite.api.plugin;

import java.util.Arrays;
import java.util.Objects;
import java.util.UUID;
import org.bukkit.entity.Player;
import org.bukkit.plugin.Plugin;
import org.bukkit.plugin.messaging.PluginMessageListener;
import org.bukkit.plugin.messaging.PluginMessageListenerRegistration;
import org.bukkit.plugin.messaging.StandardMessenger;

/**
 * Plugin messaging channel registry and dispatcher backed by Bukkit's {@link StandardMessenger}
 * and bridged to the Rust host when running under {@link dev.ferrite.api.FerriteHostApi}.
 */
public final class FerriteMessenger extends StandardMessenger {

    /** Optional host bridge for syncing channel ownership and sending outbound packets. */
    public interface ChannelBridge {
        void register(String plugin, String channel, String direction);

        void unregister(String plugin, String channel, String direction);

        void send(String plugin, UUID playerUuid, String channel, byte[] data);
    }

    private volatile ChannelBridge bridge;

    public void setBridge(ChannelBridge bridge) {
        this.bridge = bridge;
    }

    @Override
    public void registerOutgoingPluginChannel(Plugin plugin, String channel) {
        super.registerOutgoingPluginChannel(plugin, channel);
        ChannelBridge active = bridge;
        if (active != null) {
            active.register(plugin.getName(), validateAndCorrectChannel(channel), "outgoing");
        }
    }

    @Override
    public void unregisterOutgoingPluginChannel(Plugin plugin, String channel) {
        super.unregisterOutgoingPluginChannel(plugin, channel);
        ChannelBridge active = bridge;
        if (active != null) {
            active.unregister(plugin.getName(), validateAndCorrectChannel(channel), "outgoing");
        }
    }

    @Override
    public void unregisterOutgoingPluginChannel(Plugin plugin) {
        super.unregisterOutgoingPluginChannel(plugin);
        ChannelBridge active = bridge;
        if (active != null) {
            active.unregister(plugin.getName(), null, "outgoing");
        }
    }

    @Override
    public PluginMessageListenerRegistration registerIncomingPluginChannel(
            Plugin plugin, String channel, PluginMessageListener listener) {
        PluginMessageListenerRegistration reg =
                super.registerIncomingPluginChannel(plugin, channel, listener);
        ChannelBridge active = bridge;
        if (active != null) {
            active.register(plugin.getName(), reg.getChannel(), "incoming");
        }
        return reg;
    }

    @Override
    public void unregisterIncomingPluginChannel(
            Plugin plugin, String channel, PluginMessageListener listener) {
        super.unregisterIncomingPluginChannel(plugin, channel, listener);
        String corrected = validateAndCorrectChannel(channel);
        ChannelBridge active = bridge;
        if (active != null && !isIncomingChannelRegistered(plugin, corrected)) {
            active.unregister(plugin.getName(), corrected, "incoming");
        }
    }

    @Override
    public void unregisterIncomingPluginChannel(Plugin plugin, String channel) {
        super.unregisterIncomingPluginChannel(plugin, channel);
        ChannelBridge active = bridge;
        if (active != null) {
            active.unregister(plugin.getName(), validateAndCorrectChannel(channel), "incoming");
        }
    }

    @Override
    public void unregisterIncomingPluginChannel(Plugin plugin) {
        super.unregisterIncomingPluginChannel(plugin);
        ChannelBridge active = bridge;
        if (active != null) {
            active.unregister(plugin.getName(), null, "incoming");
        }
    }

    @Override
    public void dispatchIncomingMessage(Player source, String channel, byte[] message) {
        Objects.requireNonNull(message, "message");
        super.dispatchIncomingMessage(source, channel, Arrays.copyOf(message, message.length));
    }

    /**
     * Validates Bukkit ownership/channel/size rules and forwards the copied payload to the host.
     */
    public void sendPluginMessage(Plugin source, UUID playerUuid, String channel, byte[] message) {
        validatePluginMessage(this, source, channel, message);
        String corrected = validateAndCorrectChannel(channel);
        ChannelBridge active = bridge;
        if (active != null) {
            active.send(
                    source.getName(),
                    playerUuid,
                    corrected,
                    Arrays.copyOf(message, message.length));
        }
    }
}
