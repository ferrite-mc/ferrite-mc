package dev.ferrite.server;

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.LiteralMessage;
import com.mojang.brigadier.builder.LiteralArgumentBuilder;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import com.mojang.brigadier.exceptions.SimpleCommandExceptionType;
import com.mojang.brigadier.tree.LiteralCommandNode;
import io.papermc.paper.command.brigadier.BasicCommand;
import io.papermc.paper.command.brigadier.CommandRegistrationFlag;
import io.papermc.paper.command.brigadier.CommandSourceStack;
import io.papermc.paper.command.brigadier.Commands;
import io.papermc.paper.plugin.configuration.PluginMeta;
import java.util.ArrayList;
import java.util.Collection;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Objects;
import java.util.Set;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.ComponentLike;
import net.kyori.adventure.text.format.NamedTextColor;
import org.bukkit.Bukkit;
import org.bukkit.Location;
import org.bukkit.command.CommandSender;
import org.bukkit.entity.Entity;
import org.bukkit.entity.Player;
import org.bukkit.plugin.Plugin;

/**
 * The {@code Commands} registrar one plugin's {@code COMMANDS} lifecycle handlers receive.
 *
 * <p>Both forms end as a Bukkit plugin command in the server's command map (so the host's command table and the
 * chat dispatcher reach them): a {@link BasicCommand} runs directly, a Brigadier literal node runs through this plugin's own
 * {@link CommandDispatcher}. Argument types that need Paper's server-side provider ({@code ArgumentTypes}) are not
 * available; the plain Brigadier types ({@code StringArgumentType}, {@code IntegerArgumentType}, ...) are.
 */
final class FerriteCommands implements Commands {
    record Registered(String label, List<String> aliases, String permission, String description) {}

    private final Plugin plugin;
    private final CommandDispatcher<CommandSourceStack> dispatcher = new CommandDispatcher<>();
    private final List<Registered> registered = new ArrayList<>();
    private final List<String> mapKeys = new ArrayList<>();

    FerriteCommands(Plugin plugin) { this.plugin = plugin; }

    synchronized List<Registered> registered() { return List.copyOf(registered); }

    /** Forgets every command registered so far, including its entries in the server's command map. */
    synchronized void clear() {
        if (Bukkit.getServer() instanceof FerriteServer server) server.removeCommandKeys(mapKeys);
        mapKeys.clear();
        registered.clear();
        dispatcher.getRoot().getChildren().clear();
    }

    @Override public CommandDispatcher<CommandSourceStack> getDispatcher() { return dispatcher; }

    @Override public Set<String> register(LiteralCommandNode<CommandSourceStack> node, String description, Collection<String> aliases) {
        return register(plugin.getPluginMeta(), node, description, aliases);
    }

    @Override public Set<String> register(PluginMeta meta, LiteralCommandNode<CommandSourceStack> node, String description, Collection<String> aliases) {
        return registerWithFlags(meta, node, description, aliases, Set.of());
    }

    @Override public synchronized Set<String> registerWithFlags(PluginMeta meta, LiteralCommandNode<CommandSourceStack> node, String description, Collection<String> aliases, Set<CommandRegistrationFlag> flags) {
        Objects.requireNonNull(meta, "meta"); Objects.requireNonNull(node, "node"); Objects.requireNonNull(aliases, "aliases"); Objects.requireNonNull(flags, "flags");
        requireOwner(meta);
        var label = label(node.getLiteral());
        var names = aliases(label, aliases);
        dispatcher.getRoot().addChild(node);
        for (var alias : names) {
            LiteralArgumentBuilder<CommandSourceStack> redirect = LiteralArgumentBuilder.<CommandSourceStack>literal(alias).requires(node.getRequirement());
            // FLATTEN_ALIASES copies the children instead of redirecting, like Paper; a redirect keeps the arguments in one place.
            if (flags.contains(CommandRegistrationFlag.FLATTEN_ALIASES)) for (var child : node.getChildren()) redirect.then(child);
            else redirect.redirect(node);
            if (node.getCommand() != null) redirect.executes(node.getCommand());
            dispatcher.getRoot().addChild(redirect.build());
        }
        return install(label, names, null, description, (sender, name, args) -> runNode(sender, name, args), (sender, name, args) -> suggestNode(sender, name, args));
    }

    @Override public Set<String> register(PluginMeta meta, String label, String description, Collection<String> aliases, BasicCommand command) {
        Objects.requireNonNull(meta, "meta"); Objects.requireNonNull(label, "label"); Objects.requireNonNull(aliases, "aliases"); Objects.requireNonNull(command, "command");
        requireOwner(meta);
        var clean = label(label);
        var names = aliases(clean, aliases);
        return install(clean, names, command.permission(), description, (sender, name, args) -> runBasic(command, sender, args), (sender, name, args) -> {
            var stack = new FerriteCommandSource(sender, null);
            return command.canUse(sender) ? new ArrayList<>(command.suggest(stack, args)) : List.of();
        });
    }

    @Override public Set<String> register(String label, String description, Collection<String> aliases, BasicCommand command) {
        return register(plugin.getPluginMeta(), label, description, aliases, command);
    }

    private void requireOwner(PluginMeta meta) {
        if (!plugin.getPluginMeta().getName().equals(meta.getName())) throw new IllegalArgumentException("Ferrite: " + plugin.getName() + " cannot register commands for another plugin (" + meta.getName() + ")");
    }

    private static String label(String raw) {
        var label = raw.toLowerCase(Locale.ROOT);
        if (label.isEmpty() || label.chars().anyMatch(c -> Character.isWhitespace(c) || c == ':')) throw new IllegalArgumentException("Invalid command label '" + raw + "': it must be non-empty and contain no whitespace or ':'");
        return label;
    }

    private static List<String> aliases(String label, Collection<String> raw) {
        var names = new LinkedHashSet<String>();
        for (var alias : raw) { var clean = label(alias); if (!clean.equals(label)) names.add(clean); }
        return List.copyOf(names);
    }

    private interface Run { boolean run(CommandSender sender, String label, String[] args); }
    private interface Suggest { List<String> suggest(CommandSender sender, String label, String[] args); }

    private Set<String> install(String label, List<String> aliases, String permission, String description, Run run, Suggest suggest) {
        if (!(Bukkit.getServer() instanceof FerriteServer server)) throw new IllegalStateException("Ferrite: no server to register commands on");
        var accepted = server.registerPluginCommand(plugin, label, description, aliases, (sender, command, used, args) -> run.run(sender, used, args), (sender, command, used, args) -> suggest.suggest(sender, used, args), permission, mapKeys);
        registered.add(new Registered(label, aliases, permission == null ? "" : permission, description == null ? "" : description));
        return accepted;
    }

    private static boolean runBasic(BasicCommand command, CommandSender sender, String[] args) {
        if (!command.canUse(sender)) return false;
        try { command.execute(new FerriteCommandSource(sender, null), args); }
        catch (CommandSyntaxException error) { sender.sendMessage(Component.text(error.getRawMessage().getString(), NamedTextColor.RED)); }
        return true;
    }

    private boolean runNode(CommandSender sender, String label, String[] args) {
        var line = line(label, args);
        try { dispatcher.execute(line, new FerriteCommandSource(sender, null)); }
        catch (CommandSyntaxException error) { sender.sendMessage(Component.text(error.getMessage(), NamedTextColor.RED)); }
        return true;
    }

    private List<String> suggestNode(CommandSender sender, String label, String[] args) {
        var parse = dispatcher.parse(line(label, args), new FerriteCommandSource(sender, null));
        return dispatcher.getCompletionSuggestions(parse).join().getList().stream().map(suggestion -> suggestion.getText()).toList();
    }

    /** The dispatcher line for a label that may carry a {@code plugin:} prefix. */
    private static String line(String label, String[] args) {
        var name = label.substring(label.indexOf(':') + 1);
        return args.length == 0 ? name : name + " " + String.join(" ", args);
    }

    /** The source a command runs as: the sender, the entity it is (if any) and where it stands. */
    private record FerriteCommandSource(CommandSender sender, Location at, Entity as) implements CommandSourceStack {
        FerriteCommandSource(CommandSender sender, Location at) { this(sender, at, sender instanceof Entity entity ? entity : null); }
        @Override public Location getLocation() {
            if (at != null) return at.clone();
            if (as != null) return as.getLocation();
            return Bukkit.getWorlds().get(0).getSpawnLocation();
        }
        @Override public CommandSender getSender() { return sender; }
        @Override public Entity getExecutor() { return as; }
        @Override public Player getPlayerOrThrow() throws CommandSyntaxException {
            if (as instanceof Player player) return player;
            throw new SimpleCommandExceptionType(new LiteralMessage("Player expected")).create();
        }
        @Override public Entity getEntityOrThrow() throws CommandSyntaxException {
            if (as != null) return as;
            throw new SimpleCommandExceptionType(new LiteralMessage("Entity expected")).create();
        }
        @Override public CommandSourceStack withLocation(Location location) { return new FerriteCommandSource(sender, Objects.requireNonNull(location, "location"), as); }
        @Override public CommandSourceStack withExecutor(Entity executor) { return new FerriteCommandSource(sender, at, Objects.requireNonNull(executor, "executor")); }
        @Override public void sendReply(ComponentLike message) { sender.sendMessage(message); }
        @Override public void sendSuccess(ComponentLike message, boolean broadcastToOps) { sender.sendMessage(message); }
        @Override public void sendFailure(ComponentLike message) { sender.sendMessage(message.asComponent().colorIfAbsent(NamedTextColor.RED)); }
    }
}
