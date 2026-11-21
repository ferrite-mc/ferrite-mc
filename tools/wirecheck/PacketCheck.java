import com.mojang.serialization.Lifecycle;
import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;
import java.io.FileDescriptor;
import java.io.FileOutputStream;
import java.io.PrintStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import net.minecraft.SharedConstants;
import net.minecraft.core.HolderSet;
import net.minecraft.core.MappedRegistry;
import net.minecraft.core.RegistrationInfo;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.component.DataComponentMap;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.ProtocolInfo;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.chat.ChatType;
import net.minecraft.network.chat.ChatTypeDecoration;
import net.minecraft.network.chat.Style;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.configuration.ConfigurationProtocols;
import net.minecraft.network.protocol.game.GameProtocols;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.valueproviders.ConstantInt;
import net.minecraft.world.attribute.EnvironmentAttributeMap;
import net.minecraft.world.level.CardinalLighting;
import net.minecraft.world.level.dimension.DimensionType;

/** Decodes recorded clientbound packets with the real vanilla codecs. Input lines: "<phase> <name> <hex frame>". */
public class PacketCheck {
    /**
     * The packet codecs resolve registry holders while decoding (`login` needs dimension types,
     * `player_chat` needs chat types), so {@code RegistryAccess.EMPTY} rejects valid packets.
     * Built-in registries cover dimension types; chat types are a datapack registry, so register
     * enough placeholders to cover the ids Ferrite sends (sorted `chat_type` entries).
     */
    private static RegistryAccess registryAccess() {
        Map<ResourceKey<? extends Registry<?>>, Registry<?>> registries = new HashMap<>();
        RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries.REGISTRY).registries()
                .forEach(entry -> registries.put(entry.key(), entry.value()));
        MappedRegistry<ChatType> chatTypes = new MappedRegistry<>(Registries.CHAT_TYPE, Lifecycle.stable());
        for (int i = 0; i < 64; i++) {
            ChatTypeDecoration decoration = new ChatTypeDecoration("ferrite.chat." + i, List.of(), Style.EMPTY);
            chatTypes.register(
                    ResourceKey.create(Registries.CHAT_TYPE, Identifier.fromNamespaceAndPath("ferrite", "chat_" + i)),
                    new ChatType(decoration, decoration),
                    RegistrationInfo.BUILT_IN);
        }
        chatTypes.freeze();
        registries.put(Registries.CHAT_TYPE, chatTypes);

        // Dimension types are a datapack registry too; `login` holds one by id.
        MappedRegistry<DimensionType> dimensionTypes = new MappedRegistry<>(Registries.DIMENSION_TYPE, Lifecycle.stable());
        for (int i = 0; i < 64; i++) {
            DimensionType type = new DimensionType(
                    false, true, false, false, 1.0 + i, -64, 384, 384,
                    HolderSet.empty(), 0.0f,
                    new DimensionType.MonsterSettings(ConstantInt.of(0), 0),
                    DimensionType.Skybox.OVERWORLD, CardinalLighting.Type.DEFAULT,
                    EnvironmentAttributeMap.EMPTY, HolderSet.empty(), Optional.empty());
            dimensionTypes.register(
                    ResourceKey.create(Registries.DIMENSION_TYPE, Identifier.fromNamespaceAndPath("ferrite", "dimension_" + i)),
                    type,
                    RegistrationInfo.BUILT_IN);
        }
        dimensionTypes.freeze();
        registries.put(Registries.DIMENSION_TYPE, dimensionTypes);

        // Mob variants & sound variants are 26.3 datapack registries referenced by entity metadata holders.
        for (ResourceKey<? extends Registry<?>> regKey : List.<ResourceKey<? extends Registry<?>>>of(
                Registries.CAT_VARIANT,
                Registries.CAT_SOUND_VARIANT,
                Registries.CHICKEN_VARIANT,
                Registries.CHICKEN_SOUND_VARIANT,
                Registries.COW_VARIANT,
                Registries.COW_SOUND_VARIANT,
                Registries.FROG_VARIANT,
                Registries.PIG_VARIANT,
                Registries.PIG_SOUND_VARIANT,
                Registries.WOLF_VARIANT,
                Registries.WOLF_SOUND_VARIANT,
                Registries.ZOMBIE_NAUTILUS_VARIANT)) {
            MappedRegistry<Object> reg = uncheckedRegistry(regKey);
            for (int i = 0; i < 32; i++) {
                reg.register(
                        uncheckedKey(regKey, Identifier.fromNamespaceAndPath("ferrite", "entry_" + i)),
                        new Object(),
                        RegistrationInfo.BUILT_IN);
            }
            reg.freeze();
            registries.put(regKey, reg);
        }
        return new RegistryAccess.ImmutableRegistryAccess(registries);
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static MappedRegistry<Object> uncheckedRegistry(ResourceKey<? extends Registry<?>> key) {
        return new MappedRegistry<>((ResourceKey) key, Lifecycle.stable());
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static ResourceKey<Object> uncheckedKey(ResourceKey<? extends Registry<?>> reg, Identifier id) {
        return ResourceKey.create((ResourceKey) reg, id);
    }

    public static void main(String[] args) throws Exception {
        // Bootstrap redirects System.out into the game logger; keep a handle on the real stdout.
        PrintStream out = new PrintStream(new FileOutputStream(FileDescriptor.out), true);
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        // Built-in registries (entity types, items, ...) so registry-backed fields decode, plus the
        // datapack registries Ferrite sends by id (chat types, dimension types).
        RegistryAccess registries = registryAccess();
        // Item default components are bound by datapack loading, which never runs here. Stacks only
        // need a bound map to decode, so give unbound items an empty one.
        BuiltInRegistries.ITEM.listElements().filter(item -> !item.areComponentsBound())
            .forEach(item -> item.bindComponents(DataComponentMap.EMPTY));
        ProtocolInfo<?> play = GameProtocols.CLIENTBOUND_TEMPLATE.bind(RegistryFriendlyByteBuf.decorator(registries));
        ProtocolInfo<?> config = ConfigurationProtocols.CLIENTBOUND;
        int ok = 0, bad = 0;
        for (String line : Files.readAllLines(Path.of(args[0]))) {
            String[] p = line.split(" ");
            // The hex frame is the last field; connection-end records also end in `-`.
            if (p.length < 3 || p[p.length - 1].equals("-")) {
                out.println("SKIP " + line);
                continue;
            }
            ByteBuf buf = Unpooled.wrappedBuffer(HexFormat.of().parseHex(p[2]));
            ProtocolInfo<?> info = p[0].equals("play") ? play : config;
            try {
                Packet<?> packet = (Packet<?>) info.codec().decode(buf);
                if (buf.readableBytes() != 0) {
                    bad++;
                    out.println("BAD  " + p[0] + " " + p[1] + " " + buf.readableBytes() + " bytes left over after decoding");
                } else {
                    ok++;
                    out.println("OK   " + p[0] + " " + p[1]);
                }
            } catch (Throwable t) {
                bad++;
                StringBuilder why = new StringBuilder();
                for (Throwable c = t; c != null; c = c.getCause()) why.append(" <- ").append(c.getClass().getSimpleName()).append(": ").append(c.getMessage());
                out.println("BAD  " + p[0] + " " + p[1] + why);
            }
        }
        out.println("SUMMARY ok=" + ok + " bad=" + bad);
        System.exit(bad == 0 ? 0 : 1);
    }
}
