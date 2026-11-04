package dev.ferrite.server;

import java.util.ArrayList;
import java.util.List;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.serializer.plain.PlainTextComponentSerializer;
import org.bukkit.entity.HumanEntity;
import org.bukkit.inventory.MerchantRecipe;

/** An in-memory merchant from Server#createMerchant: recipes are kept in Java and nothing is traded. */
final class FerriteMerchant implements org.bukkit.inventory.Merchant {
    private final List<MerchantRecipe> recipes = new ArrayList<>();
    private final Component title;

    FerriteMerchant() { this.title = null; }
    FerriteMerchant(String title) { this(Component.text(java.util.Objects.requireNonNull(title, "title"))); }
    FerriteMerchant(Component title) { this.title = java.util.Objects.requireNonNull(title, "title"); }

    /** The merchant's title; Bukkit's Merchant interface has no accessor for it. */
    Component title() { return title; }
    String getTitle() { return title == null ? null : PlainTextComponentSerializer.plainText().serialize(title); }

    @Override public List<MerchantRecipe> getRecipes() { return List.copyOf(recipes); }
    @Override public void setRecipes(List<MerchantRecipe> values) {
        java.util.Objects.requireNonNull(values, "recipes");
        var copy = new ArrayList<MerchantRecipe>(values.size());
        for (var recipe : values) copy.add(java.util.Objects.requireNonNull(recipe, "recipe"));
        recipes.clear();
        recipes.addAll(copy);
    }
    @Override public MerchantRecipe getRecipe(int index) { return recipes.get(index); }
    @Override public void setRecipe(int index, MerchantRecipe recipe) { recipes.set(index, java.util.Objects.requireNonNull(recipe, "recipe")); }
    @Override public int getRecipeCount() { return recipes.size(); }
    @Override public boolean isTrading() { return false; }
    @Override public HumanEntity getTrader() { return null; }
}
