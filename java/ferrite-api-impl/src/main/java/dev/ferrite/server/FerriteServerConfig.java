package dev.ferrite.server;

/** Paper's proxy configuration (Server#getServerConfig); Ferrite is not run behind a proxy. */
final class FerriteServerConfig implements io.papermc.paper.configuration.ServerConfiguration {
    @Override public boolean isProxyEnabled() { return false; }
    @Override public boolean isProxyOnlineMode() { return false; }
}
