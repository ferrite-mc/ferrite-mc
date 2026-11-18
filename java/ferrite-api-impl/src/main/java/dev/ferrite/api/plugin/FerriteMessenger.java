package dev.ferrite.api.plugin;

import org.bukkit.plugin.messaging.StandardMessenger;

/**
 * In-memory plugin messaging channel registry backed by Bukkit's {@link StandardMessenger}.
 *
 * <p>Supports incoming/outgoing channel registration, listener registration validation, and
 * {@link #dispatchIncomingMessage} routing. Actual network forwarding over Minecraft protocol
 * plugin-message packets is outside the current host contract.
 */
public final class FerriteMessenger extends StandardMessenger {
}
