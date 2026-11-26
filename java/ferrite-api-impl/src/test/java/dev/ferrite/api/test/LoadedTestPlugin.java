package dev.ferrite.api.test;

/**
 * Defined a second time by {@link TestPluginFactory} through a
 * {@link dev.ferrite.api.plugin.FerritePluginClassLoader}. The duplicate definition gives the
 * plugin instance a Paper-accepted classloader while it remains a {@link TestPlugin}, so
 * tests can keep using the parent-loaded type.
 */
public final class LoadedTestPlugin extends TestPlugin {
}
