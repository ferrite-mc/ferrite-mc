// Encodes the vanilla recipe book with the server's own network codecs by booting the real data
// pack loader on the user's server jar. Output is gitignored.
import com.mojang.datafixers.util.Pair;
import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.commands.Commands;
import net.minecraft.core.RegistryAccess;
import net.minecraft.network.RegistryFriendlyByteBuf;
import net.minecraft.network.protocol.game.ClientboundRecipeBookAddPacket;
import net.minecraft.network.protocol.game.ClientboundUpdateRecipesPacket;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.WorldLoader;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.permissions.LevelBasedPermissionSet;
import net.minecraft.world.item.crafting.RecipeHolder;
import net.minecraft.world.item.crafting.RecipeManager;
import net.minecraft.world.level.WorldDataConfiguration;

public final class RecipeExtractor {
    static String hex(ByteBuf buf) {
        byte[] bytes = new byte[buf.readableBytes()];
        buf.readBytes(bytes);
        StringBuilder out = new StringBuilder();
        for (byte b : bytes) out.append(String.format("%02x", b));
        return out.toString();
    }

    public static void main(String[] args) throws Exception {
        if (args.length != 1) throw new IllegalArgumentException("expected output file");
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PackRepository repository = ServerPacksSource.createVanillaTrustedRepository();
        repository.reload();
        repository.setSelected(List.of("vanilla"));
        var config = new WorldLoader.InitConfig(
                new WorldLoader.PackConfig(repository, WorldDataConfiguration.DEFAULT, false, true),
                Commands.CommandSelection.DEDICATED, LevelBasedPermissionSet.ADMIN);
        Object[] loaded = WorldLoader.<Boolean, Object[]>load(config,
                context -> new WorldLoader.DataLoadOutput<>(Boolean.TRUE, context.datapackDimensions()),
                (manager, resources, layered, cookie) -> new Object[] {resources, layered},
                Runnable::run, Runnable::run).get();
        var resources = (net.minecraft.server.ReloadableServerResources) loaded[0];
        @SuppressWarnings("unchecked")
        var layered = (net.minecraft.core.LayeredRegistryAccess<net.minecraft.server.RegistryLayer>) loaded[1];
        RegistryAccess access = layered.compositeAccess();
        RecipeManager manager = resources.getRecipeManager();
        manager.finalizeRecipeLoading(net.minecraft.world.flag.FeatureFlags.VANILLA_SET);

        StringBuilder out = new StringBuilder("{\"recipes\":[\n");
        boolean first = true;
        List<RecipeHolder<?>> holders = new ArrayList<>(manager.getRecipes());
        holders.sort(Comparator.comparing(h -> h.id().identifier().toString()));
        for (RecipeHolder<?> holder : holders) {
            List<String> wires = new ArrayList<>();
            List<Integer> ids = new ArrayList<>();
            manager.listDisplaysForRecipe(holder.id(), entry -> {
                ByteBuf buf = Unpooled.buffer();
                ClientboundRecipeBookAddPacket.Entry.STREAM_CODEC.encode(
                        new RegistryFriendlyByteBuf(buf, access),
                        new ClientboundRecipeBookAddPacket.Entry(entry, (byte) 0));
                wires.add(hex(buf));
                ids.add(entry.id().index());
            });
            if (wires.isEmpty()) continue;
            if (!first) out.append(",\n");
            first = false;
            out.append("{\"id\":\"").append(holder.id().identifier())
                .append("\",\"notify\":").append(holder.value().showNotification())
                .append(",\"entries\":[");
            for (int i = 0; i < wires.size(); i++) {
                if (i > 0) out.append(',');
                out.append("{\"index\":").append(ids.get(i)).append(",\"wire\":\"").append(wires.get(i)).append("\"}");
            }
            out.append("]}");
        }
        ByteBuf update = Unpooled.buffer();
        ClientboundUpdateRecipesPacket.STREAM_CODEC.encode(new RegistryFriendlyByteBuf(update, access),
                new ClientboundUpdateRecipesPacket(manager.getSynchronizedItemProperties(),
                        manager.getSynchronizedStonecutterRecipes()));
        out.append("\n],\"update_recipes\":\"").append(hex(update)).append("\"}\n");
        Files.writeString(Path.of(args[0]), out, StandardCharsets.UTF_8);
        System.exit(0);
    }
}
