package dev.ferrite.api.entity;

import com.destroystokyo.paper.profile.PlayerProfile;
import com.destroystokyo.paper.profile.ProfileProperty;
import java.net.URL;
import java.util.Collection;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.LinkedHashSet;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import org.bukkit.profile.PlayerTextures;

/** Clean-room in-memory {@link PlayerProfile} backing player/server profile APIs. */
public final class FerritePlayerProfile implements PlayerProfile {

    private UUID id;
    private String name;
    private final Map<String, ProfileProperty> properties = new LinkedHashMap<>();
    private final FerritePlayerTextures textures = new FerritePlayerTextures();

    public FerritePlayerProfile(UUID id, String name) {
        this.id = id;
        this.name = name;
    }

    private FerritePlayerProfile(FerritePlayerProfile other) {
        this.id = other.id;
        this.name = other.name;
        this.properties.putAll(other.properties);
        this.textures.copyFrom(other.textures);
    }

    @Override
    public String getName() {
        return name;
    }

    @Override
    public String setName(String name) {
        String previous = this.name;
        this.name = name;
        return previous;
    }

    @Override
    public UUID getId() {
        return id;
    }

    @Override
    public UUID setId(UUID uuid) {
        UUID previous = this.id;
        this.id = uuid;
        return previous;
    }

    @Override
    public UUID getUniqueId() {
        return id;
    }

    @Override
    public PlayerTextures getTextures() {
        return textures;
    }

    @Override
    public void setTextures(PlayerTextures textures) {
        if (textures == null) {
            this.textures.clear();
            return;
        }
        this.textures.setSkin(textures.getSkin(), textures.getSkinModel());
        this.textures.setCape(textures.getCape());
    }

    @Override
    public Set<ProfileProperty> getProperties() {
        return Collections.unmodifiableSet(new LinkedHashSet<>(properties.values()));
    }

    @Override
    public boolean hasProperty(String property) {
        return property != null && properties.containsKey(property);
    }

    @Override
    public void setProperty(ProfileProperty property) {
        Objects.requireNonNull(property, "property");
        properties.put(property.getName(), property);
    }

    @Override
    public void setProperties(Collection<ProfileProperty> properties) {
        Objects.requireNonNull(properties, "properties");
        clearProperties();
        for (ProfileProperty property : properties) {
            setProperty(property);
        }
    }

    @Override
    public boolean removeProperty(String property) {
        return property != null && properties.remove(property) != null;
    }

    @Override
    public void clearProperties() {
        properties.clear();
    }

    @Override
    public boolean isComplete() {
        return id != null && name != null && !name.isEmpty();
    }

    @Override
    public boolean completeFromCache() {
        return isComplete();
    }

    @Override
    public boolean completeFromCache(boolean onlineMode) {
        return isComplete();
    }

    @Override
    public boolean completeFromCache(boolean lookupUUID, boolean onlineMode) {
        return isComplete();
    }

    @Override
    public boolean complete(boolean textures) {
        return isComplete();
    }

    @Override
    public boolean complete(boolean textures, boolean onlineMode) {
        return isComplete();
    }

    @Override
    public CompletableFuture<PlayerProfile> update() {
        return CompletableFuture.completedFuture(clone());
    }

    @Override
    public PlayerProfile clone() {
        return new FerritePlayerProfile(this);
    }

    @Override
    public Map<String, Object> serialize() {
        Map<String, Object> result = new LinkedHashMap<>();
        if (id != null) {
            result.put("uniqueId", id.toString());
        }
        if (name != null) {
            result.put("name", name);
        }
        return result;
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof FerritePlayerProfile profile
                && Objects.equals(id, profile.id)
                && Objects.equals(name, profile.name)
                && Objects.equals(properties, profile.properties);
    }

    @Override
    public int hashCode() {
        return Objects.hash(id, name, properties);
    }

    private static final class FerritePlayerTextures implements PlayerTextures {
        private URL skin;
        private SkinModel skinModel = SkinModel.CLASSIC;
        private URL cape;
        private long timestamp;

        void copyFrom(FerritePlayerTextures other) {
            this.skin = other.skin;
            this.skinModel = other.skinModel;
            this.cape = other.cape;
            this.timestamp = other.timestamp;
        }

        @Override
        public boolean isEmpty() {
            return skin == null && cape == null;
        }

        @Override
        public void clear() {
            skin = null;
            skinModel = SkinModel.CLASSIC;
            cape = null;
            timestamp = 0L;
        }

        @Override
        public URL getSkin() {
            return skin;
        }

        @Override
        public void setSkin(URL skin) {
            setSkin(skin, SkinModel.CLASSIC);
        }

        @Override
        public void setSkin(URL skin, SkinModel skinModel) {
            this.skin = skin;
            this.skinModel = skinModel == null ? SkinModel.CLASSIC : skinModel;
            this.timestamp = System.currentTimeMillis();
        }

        @Override
        public SkinModel getSkinModel() {
            return skinModel;
        }

        @Override
        public URL getCape() {
            return cape;
        }

        @Override
        public void setCape(URL cape) {
            this.cape = cape;
            this.timestamp = System.currentTimeMillis();
        }

        @Override
        public long getTimestamp() {
            return timestamp;
        }

        @Override
        public boolean isSigned() {
            return false;
        }
    }
}
