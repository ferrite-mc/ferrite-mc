package dev.ferrite.server;

import java.net.URI;
import java.util.ArrayList;
import java.util.List;
import net.kyori.adventure.text.Component;
import org.bukkit.ServerLinks;

/** The server links plugins register (Server#getServerLinks), kept in memory. */
final class FerriteServerLinks implements ServerLinks {
    private final List<ServerLink> links = new ArrayList<>();

    private record Link(Type type, Component name, URI url) implements ServerLink {
        @Override public Type getType() { return type; }
        @Override public Component displayName() {
            return type == null ? name : Component.translatable("known_server_link." + type.name().toLowerCase(java.util.Locale.ROOT));
        }
        @Override public String getDisplayName() {
            return net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer.plainText().serialize(displayName());
        }
        @Override public URI getUrl() { return url; }
    }

    @Override public synchronized ServerLink getLink(Type type) {
        java.util.Objects.requireNonNull(type, "type");
        for (var link : links) if (link.getType() == type) return link;
        return null;
    }
    @Override public synchronized List<ServerLink> getLinks() { return List.copyOf(links); }
    @Override public synchronized ServerLink setLink(Type type, URI url) {
        java.util.Objects.requireNonNull(type, "type");
        java.util.Objects.requireNonNull(url, "url");
        links.removeIf(link -> link.getType() == type);
        return addLink(type, url);
    }
    @Override public synchronized ServerLink addLink(Type type, URI url) {
        java.util.Objects.requireNonNull(type, "type");
        java.util.Objects.requireNonNull(url, "url");
        var link = new Link(type, null, url);
        links.add(link);
        return link;
    }
    @Override public synchronized ServerLink addLink(Component name, URI url) {
        java.util.Objects.requireNonNull(name, "name");
        java.util.Objects.requireNonNull(url, "url");
        var link = new Link(null, name, url);
        links.add(link);
        return link;
    }
    @Override public ServerLink addLink(String name, URI url) {
        java.util.Objects.requireNonNull(name, "name");
        return addLink(Component.text(name), url);
    }
    @Override public synchronized boolean removeLink(ServerLink link) { return links.remove(link); }
    @Override public synchronized ServerLinks copy() {
        var copy = new FerriteServerLinks();
        copy.links.addAll(links);
        return copy;
    }
}
