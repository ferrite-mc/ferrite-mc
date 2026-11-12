package dev.ferrite.server;

import java.util.Objects;
import org.bukkit.Effect;

/**
 * The integer a vanilla level event carries for a Bukkit {@link Effect}'s data object.
 *
 * <p>Every encoding here was read off the call sites in the 26.3 server (what vanilla passes to
 * {@code ServerLevel#levelEvent} for that event id). An effect whose encoding could not be confirmed that way is
 * refused with the reason instead of guessed; a plain {@link Number} is always passed through as the raw data value.
 */
final class FerriteEffectData {
    private FerriteEffectData() {}

    static int levelEventData(Effect effect, Object data) {
        Objects.requireNonNull(effect, "effect");
        if (data == null) return 0;
        if (data instanceof Number number) return number.intValue();
        Class<?> declared = effect.getData();
        if (declared == null || !declared.isInstance(data))
            throw new IllegalArgumentException(effect + " takes " + (declared == null ? "no data" : declared.getSimpleName()) + ", not " + data.getClass().getSimpleName());
        if (data instanceof Boolean flag) return bool(effect, flag);
        // Potions send PotionContents#getColor, an opaque ARGB int (the experience bottle sends 0xFF385DC6).
        if (data instanceof org.bukkit.Color color) return color.asARGB();
        // Lightning rods send Direction.Axis#ordinal.
        if (data instanceof org.bukkit.Axis axis) return axis.ordinal();
        if (data instanceof org.bukkit.block.data.BlockData block) return FerriteBlockStates.stateId(block.getAsString());
        if (data instanceof org.bukkit.block.BlockFace face) return face(effect, face);
        if (data instanceof org.bukkit.JukeboxSong)
            throw new UnsupportedOperationException("Ferrite: " + effect + " carries the song's registry id, and the ids of data-driven registries are not in the generated reports; pass the raw id as an int");
        throw new IllegalArgumentException("Unsupported data " + data.getClass().getSimpleName() + " for " + effect);
    }

    private static int bool(Effect effect, boolean flag) {
        return switch (effect.name()) {
            // Composter#useItemOn and the vault states send 1 or 0.
            case "COMPOSTER_FILL_ATTEMPT", "VAULT_ACTIVATE", "VAULT_DEACTIVATE" -> flag ? 1 : 0;
            // DragonFireball#onHit sends 1, or -1 when the fireball is silent: true means audible.
            case "ENDER_DRAGON_BREATH" -> flag ? 1 : -1;
            default -> throw new UnsupportedOperationException("Ferrite: " + effect
                    + " takes a Boolean, but the vanilla server only sends a fixed value or an encoded particle for it, so the mapping is unverified; pass the raw int");
        };
    }

    private static int face(Effect effect, org.bukkit.block.BlockFace face) {
        if (!effect.name().equals("WHITE_SMOKE_SHOOT"))
            throw new UnsupportedOperationException("Ferrite: " + effect
                    + " takes a BlockFace, but no vanilla server call site for its event was found to confirm the encoding; pass the raw int");
        // Direction#get3DDataValue
        return switch (face) {
            case DOWN -> 0;
            case UP -> 1;
            case NORTH -> 2;
            case SOUTH -> 3;
            case WEST -> 4;
            case EAST -> 5;
            default -> throw new IllegalArgumentException(effect + " needs a cardinal or vertical face, not " + face);
        };
    }
}
