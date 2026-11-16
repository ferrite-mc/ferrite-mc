package dev.ferrite.api.item;

import io.papermc.paper.persistence.PersistentDataContainerView;
import java.io.IOException;
import java.util.Objects;
import java.util.Set;
import net.kyori.adventure.key.Key;
import org.bukkit.NamespacedKey;
import org.bukkit.persistence.PersistentDataAdapterContext;
import org.bukkit.persistence.PersistentDataContainer;
import org.bukkit.persistence.PersistentDataType;

/**
 * Read-only {@link PersistentDataContainerView} backed by a defensive snapshot of an
 * item's persistent data.
 *
 * <p>Unlike {@link FerritePersistentDataContainer}, this class does not implement
 * {@link PersistentDataContainer}, so callers cannot cast it to mutate an {@code ItemStack}
 * without going through {@code editPersistentDataContainer} or {@code setItemMeta}.
 */
public final class FerritePersistentDataContainerView implements PersistentDataContainerView {

    private final FerritePersistentDataContainer delegate;

    public FerritePersistentDataContainerView(FerritePersistentDataContainer source) {
        this.delegate = source == null ? new FerritePersistentDataContainer() : source.copy();
    }

    @Override
    public <P, C> boolean has(NamespacedKey key, PersistentDataType<P, C> type) {
        return delegate.has(key, type);
    }

    @Override
    public boolean has(NamespacedKey key) {
        return delegate.has(key);
    }

    @Override
    public <P, C> C get(NamespacedKey key, PersistentDataType<P, C> type) {
        return delegate.get(key, type);
    }

    @Override
    public <P, C> C getOrDefault(NamespacedKey key, PersistentDataType<P, C> type, C defaultValue) {
        return delegate.getOrDefault(key, type, defaultValue);
    }

    @Override
    public PersistentDataType<?, ?> getPrimitiveStorageType(Key key) {
        return delegate.getPrimitiveStorageType(key);
    }

    @Override
    public Set<NamespacedKey> getKeys() {
        return delegate.getKeys();
    }

    @Override
    public boolean isEmpty() {
        return delegate.isEmpty();
    }

    @Override
    public void copyTo(PersistentDataContainer target, boolean replace) {
        delegate.copyTo(target, replace);
    }

    @Override
    public PersistentDataAdapterContext getAdapterContext() {
        throw dev.ferrite.api.internal.Unsupported.exception(
                "PersistentDataContainerView#getAdapterContext");
    }

    @Override
    public byte[] serializeToBytes() throws IOException {
        throw dev.ferrite.api.internal.Unsupported.exception(
                "PersistentDataContainerView#serializeToBytes");
    }

    @Override
    public int getSize() {
        return delegate.getSize();
    }

    @Override
    public boolean equals(Object object) {
        if (this == object) {
            return true;
        }
        if (!(object instanceof FerritePersistentDataContainerView other)) {
            return false;
        }
        return Objects.equals(delegate, other.delegate);
    }

    @Override
    public int hashCode() {
        return delegate.hashCode();
    }
}
