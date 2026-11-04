package dev.ferrite.server;

import com.destroystokyo.paper.profile.ProfileProperty;
import java.net.URL;
import java.util.*;
import java.util.concurrent.CompletableFuture;
import org.bukkit.profile.PlayerTextures;

/** A player profile held in memory: id, name, properties and skin/cape URLs. */
final class FerriteProfile implements com.destroystokyo.paper.profile.PlayerProfile {
    private UUID id;
    private String name;
    private final Map<String, ProfileProperty> properties = new LinkedHashMap<>();
    private final Textures textures = new Textures();

    FerriteProfile(UUID id, String name) {
        if (id == null && name == null) throw new IllegalArgumentException("Either id or name must be provided");
        this.id = id;
        this.name = name;
    }

    private static final class Textures implements PlayerTextures {
        URL skin, cape;
        SkinModel model = SkinModel.CLASSIC;
        long timestamp;
        @Override public boolean isEmpty() { return skin == null && cape == null; }
        @Override public void clear() { skin = null; cape = null; model = SkinModel.CLASSIC; timestamp = 0; }
        @Override public URL getSkin() { return skin; }
        @Override public void setSkin(URL url) { setSkin(url, SkinModel.CLASSIC); }
        @Override public void setSkin(URL url, SkinModel m) { skin = url; model = url == null || m == null ? SkinModel.CLASSIC : m; timestamp = System.currentTimeMillis(); }
        @Override public SkinModel getSkinModel() { return model; }
        @Override public URL getCape() { return cape; }
        @Override public void setCape(URL url) { cape = url; timestamp = System.currentTimeMillis(); }
        @Override public long getTimestamp() { return timestamp; }
        @Override public boolean isSigned() { return false; }
    }

    @Override public UUID getUniqueId() { return id; }
    @Override public UUID getId() { return id; }
    @Override public UUID setId(UUID id) { UUID old = this.id; this.id = id; return old; }
    @Override public String getName() { return name; }
    @Override public String setName(String name) { String old = this.name; this.name = name; return old; }
    @Override public PlayerTextures getTextures() { return textures; }
    @Override public void setTextures(PlayerTextures t) {
        textures.clear();
        if (t != null) { textures.skin = t.getSkin(); textures.cape = t.getCape(); textures.model = t.getSkinModel(); textures.timestamp = t.getTimestamp(); }
    }
    @Override public Set<ProfileProperty> getProperties() { return new LinkedHashSet<>(properties.values()); }
    @Override public boolean hasProperty(String property) { return properties.containsKey(property); }
    @Override public void setProperty(ProfileProperty property) { properties.put(property.getName(), property); }
    @Override public void setProperties(Collection<ProfileProperty> values) { values.forEach(this::setProperty); }
    @Override public boolean removeProperty(String property) { return properties.remove(property) != null; }
    @Override public void clearProperties() { properties.clear(); }
    @Override public boolean isComplete() { return id != null && name != null; }
    @Override public boolean completeFromCache() { return isComplete(); }
    @Override public boolean completeFromCache(boolean onlineMode) { return isComplete(); }
    @Override public boolean completeFromCache(boolean lookupUUID, boolean onlineMode) { return isComplete(); }
    @Override public boolean complete(boolean textures) { return isComplete(); }
    @Override public boolean complete(boolean textures, boolean onlineMode) { return isComplete(); }
    @Override public CompletableFuture<com.destroystokyo.paper.profile.PlayerProfile> update() { return CompletableFuture.completedFuture(this); }
    @Override public FerriteProfile clone() {
        FerriteProfile copy = new FerriteProfile(id, name);
        copy.properties.putAll(properties);
        copy.textures.skin = textures.skin; copy.textures.cape = textures.cape;
        copy.textures.model = textures.model; copy.textures.timestamp = textures.timestamp;
        return copy;
    }
    @Override public Map<String, Object> serialize() {
        Map<String, Object> map = new LinkedHashMap<>();
        if (id != null) map.put("uniqueId", id.toString());
        if (name != null) map.put("name", name);
        return map;
    }
    @Override public boolean equals(Object o) {
        return o instanceof FerriteProfile p && Objects.equals(id, p.id) && Objects.equals(name, p.name) && properties.equals(p.properties);
    }
    @Override public int hashCode() { return Objects.hash(id, name, properties); }
}
