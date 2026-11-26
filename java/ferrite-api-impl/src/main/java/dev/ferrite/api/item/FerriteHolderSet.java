package dev.ferrite.api.item;

import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;

/**
 * Clean-room representation of a 26.3 registry holder set: either a {@code #namespace:tag}
 * reference or an explicit list of namespaced registry ids.
 */
public final class FerriteHolderSet {

    private final String tag;
    private final List<String> ids;

    private FerriteHolderSet(String tag, List<String> ids) {
        this.tag = tag;
        this.ids = ids;
    }

    public static FerriteHolderSet ofTag(String tag) {
        Objects.requireNonNull(tag, "tag");
        String normalized = tag.startsWith("#") ? tag.substring(1) : tag;
        if (normalized.isEmpty()) {
            throw new IllegalArgumentException("holder set tag cannot be empty");
        }
        return new FerriteHolderSet(normalized, null);
    }

    public static FerriteHolderSet ofIds(List<String> ids) {
        Objects.requireNonNull(ids, "ids");
        return new FerriteHolderSet(null, List.copyOf(ids));
    }

    public String tag() {
        return tag;
    }

    public List<String> ids() {
        return ids;
    }

    public Map<String, Object> serialize() {
        Map<String, Object> result = new LinkedHashMap<>();
        if (tag != null) {
            result.put("tag", tag);
        } else {
            result.put("ids", ids);
        }
        return result;
    }

    public static FerriteHolderSet deserialize(Map<?, ?> args) {
        Objects.requireNonNull(args, "args");
        Object rawTag = args.get("tag");
        if (rawTag != null) {
            return ofTag(String.valueOf(rawTag));
        }
        Object rawIds = args.get("ids");
        if (rawIds instanceof List<?> list) {
            return ofIds(list.stream().map(String::valueOf).toList());
        }
        throw new IllegalArgumentException("Expected 'tag' or 'ids' in FerriteHolderSet");
    }

    @Override
    public boolean equals(Object object) {
        if (this == object) {
            return true;
        }
        if (!(object instanceof FerriteHolderSet other)) {
            return false;
        }
        return Objects.equals(tag, other.tag) && Objects.equals(ids, other.ids);
    }

    @Override
    public int hashCode() {
        return Objects.hash(tag, ids);
    }

    @Override
    public String toString() {
        return tag != null ? "#" + tag : String.valueOf(ids);
    }
}
