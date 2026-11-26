package dev.ferrite.host;

import java.io.IOException;
import java.net.MalformedURLException;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.file.Path;

/**
 * One isolated class loader per plugin.
 *
 * <p>The parent is the application class loader that contains {@code ferrite-host} and Lane 2's
 * runtime jar. The plugin jar is the only child entry, so two plugins cannot see each other's
 * classes (Paper's {@code join-classpath} is not supported in contract v1).
 */
public final class PluginClassLoader extends URLClassLoader {
    private final String pluginName;

    public PluginClassLoader(String pluginName, Path jar) throws MalformedURLException {
        super(new URL[] {jar.toUri().toURL()}, HostRuntime.class.getClassLoader());
        this.pluginName = pluginName;
    }

    public String pluginName() {
        return pluginName;
    }

    @Override
    public String getName() {
        return "ferrite-plugin/" + pluginName;
    }

    @Override
    public String toString() {
        return getName();
    }

    @Override
    public void close() throws IOException {
        super.close();
    }
}
