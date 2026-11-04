package dev.ferrite.server;

import java.util.ArrayList;
import java.util.List;
import org.bukkit.Material;
import org.bukkit.NamespacedKey;
import org.bukkit.inventory.ItemCraftResult;
import org.bukkit.inventory.ItemStack;
import org.bukkit.inventory.Recipe;

/** Crafting grid lookups answered by the server's recipe book through the host. */
final class FerriteCrafting {
    private FerriteCrafting() {}

    /** The matched recipe id and result. */
    record Match(NamespacedKey id, ItemStack result) {}

    private static boolean empty(ItemStack stack) { return stack == null || stack.getType().isAir() || stack.getAmount() <= 0; }

    /** The recipe the grid crafts, or null. The grid has 4 (2x2) or 9 (3x3) slots. */
    static Match match(ItemStack[] matrix) {
        if (matrix == null || (matrix.length != 4 && matrix.length != 9)) throw new IllegalArgumentException("Crafting matrix must have 4 or 9 slots");
        var names = new ArrayList<String>();
        for (var stack : matrix) names.add(empty(stack) ? "AIR" : stack.getType().name());
        String reply;
        try {
            reply = (String) HostBridge.call("Server", "server", "craftMatrix(int,String)", String.class, matrix.length == 4 ? 2 : 3, names);
        } catch (UnsupportedOperationException noRecipe) {
            return null;
        }
        if (reply == null || reply.isEmpty()) return null;
        var parts = reply.split(String.valueOf(HostBridge.ITEM), -1);
        if (parts.length != 3) return null;
        var material = Material.matchMaterial(parts[1]);
        if (material == null) return null;
        return new Match(NamespacedKey.fromString(parts[0]), new ItemStack(material, Integer.parseInt(parts[2])));
    }

    static Recipe recipe(ItemStack[] matrix) {
        var match = match(matrix);
        return match == null ? null : FerriteRecipes.get(match.id());
    }

    /** One of every ingredient is consumed; an unmatched grid crafts nothing and stays as it is. */
    static ItemCraftResult craft(ItemStack[] matrix) {
        var match = match(matrix);
        var out = new ItemStack[matrix.length];
        for (int i = 0; i < matrix.length; i++) out[i] = matrix[i] == null ? null : matrix[i].clone();
        if (match == null) return new Result(new ItemStack(Material.AIR), out, List.of());
        for (int i = 0; i < out.length; i++) {
            if (empty(out[i])) { out[i] = null; continue; }
            if (out[i].getAmount() <= 1) out[i] = null; else out[i].setAmount(out[i].getAmount() - 1);
        }
        return new Result(match.result(), out, List.of());
    }

    private record Result(ItemStack result, ItemStack[] matrix, List<ItemStack> overflow) implements ItemCraftResult {
        @Override public ItemStack getResult() { return result.clone(); }
        @Override public ItemStack[] getResultingMatrix() { return matrix.clone(); }
        @Override public List<ItemStack> getOverflowItems() { return overflow; }
    }
}
