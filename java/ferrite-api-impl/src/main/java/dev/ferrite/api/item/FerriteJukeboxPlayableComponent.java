package dev.ferrite.api.item;

import java.util.LinkedHashMap;
import java.util.Map;
import java.util.Objects;
import org.bukkit.JukeboxSong;
import org.bukkit.NamespacedKey;
import org.bukkit.Registry;
import org.bukkit.inventory.meta.components.JukeboxPlayableComponent;

/**
 * Facade implementation of {@link JukeboxPlayableComponent} storing song key and
 * showInTooltip with defensive copying and serialization.
 */
public final class FerriteJukeboxPlayableComponent implements JukeboxPlayableComponent {

    private NamespacedKey songKey;
    private boolean showInTooltip = true;

    public FerriteJukeboxPlayableComponent() {
    }

    public FerriteJukeboxPlayableComponent(NamespacedKey songKey, boolean showInTooltip) {
        this.songKey = songKey;
        this.showInTooltip = showInTooltip;
    }

    public FerriteJukeboxPlayableComponent(JukeboxPlayableComponent source) {
        Objects.requireNonNull(source, "source");
        this.songKey = source.getSongKey();
        this.showInTooltip = source.isShowInTooltip();
    }

    @Override
    public JukeboxSong getSong() {
        return songKey == null ? null : Registry.JUKEBOX_SONG.get(songKey);
    }

    @Override
    public NamespacedKey getSongKey() {
        return songKey;
    }

    @Override
    public void setSong(JukeboxSong song) {
        Objects.requireNonNull(song, "song");
        this.songKey = song.getKey();
    }

    @Override
    public void setSongKey(NamespacedKey song) {
        Objects.requireNonNull(song, "song");
        this.songKey = song;
    }

    @Override
    public boolean isShowInTooltip() {
        return showInTooltip;
    }

    @Override
    public void setShowInTooltip(boolean show) {
        this.showInTooltip = show;
    }

    public FerriteJukeboxPlayableComponent copy() {
        return new FerriteJukeboxPlayableComponent(this);
    }

    @Override
    public Map<String, Object> serialize() {
        Map<String, Object> result = new LinkedHashMap<>();
        if (songKey != null) {
            result.put("song", songKey.toString());
        }
        result.put("show-in-tooltip", showInTooltip);
        return result;
    }

    public static FerriteJukeboxPlayableComponent deserialize(Map<?, ?> args) {
        Objects.requireNonNull(args, "args");
        FerriteJukeboxPlayableComponent component = new FerriteJukeboxPlayableComponent();
        Object song = args.get("song");
        if (song != null) {
            NamespacedKey key = NamespacedKey.fromString(String.valueOf(song));
            if (key == null) {
                throw new IllegalArgumentException("Invalid jukebox song key: " + song);
            }
            component.setSongKey(key);
        }
        Object show = args.get("show-in-tooltip");
        if (show instanceof Boolean flag) {
            component.setShowInTooltip(flag);
        }
        return component;
    }

    @Override
    public boolean equals(Object object) {
        if (this == object) {
            return true;
        }
        if (!(object instanceof FerriteJukeboxPlayableComponent other)) {
            return false;
        }
        return showInTooltip == other.showInTooltip && Objects.equals(songKey, other.songKey);
    }

    @Override
    public int hashCode() {
        return Objects.hash(songKey, showInTooltip);
    }

    @Override
    public String toString() {
        return "FerriteJukeboxPlayableComponent{songKey="
                + songKey
                + ", showInTooltip="
                + showInTooltip
                + "}";
    }
}
