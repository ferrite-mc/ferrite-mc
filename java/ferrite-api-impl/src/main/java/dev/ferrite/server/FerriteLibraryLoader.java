package dev.ferrite.server;

import java.io.File;
import java.net.MalformedURLException;
import java.net.URL;
import java.util.ArrayList;
import java.util.List;
import org.eclipse.aether.RepositorySystem;
import org.eclipse.aether.RepositorySystemSession;
import org.eclipse.aether.artifact.DefaultArtifact;
import org.eclipse.aether.collection.CollectRequest;
import org.eclipse.aether.graph.Dependency;
import org.eclipse.aether.repository.LocalRepository;
import org.eclipse.aether.repository.RemoteRepository;
import org.eclipse.aether.resolution.ArtifactResult;
import org.eclipse.aether.resolution.DependencyRequest;
import org.eclipse.aether.resolution.DependencyResolutionException;
import org.eclipse.aether.supplier.RepositorySystemSupplier;
import org.eclipse.aether.util.artifact.JavaScopes;
import org.eclipse.aether.util.filter.DependencyFilterUtils;

/**
 * Downloads the Maven coordinates a plugin lists under {@code libraries:} (with their runtime
 * dependencies) into the server's {@code libraries/} folder, as Spigot's LibraryLoader does.
 *
 * <p>The repository defaults to Maven Central and honours Paper's override property
 * {@code org.bukkit.plugin.java.LibraryLoader.centralURL}.
 */
final class FerriteLibraryLoader {
    static final String CENTRAL_PROPERTY = "org.bukkit.plugin.java.LibraryLoader.centralURL";
    private static final String CENTRAL = "https://repo.maven.apache.org/maven2/";
    private final RepositorySystem system;
    private final RepositorySystemSession session;
    private final List<RemoteRepository> repositories;

    FerriteLibraryLoader(File localRepository) {
        system = new RepositorySystemSupplier().get();
        var session = org.apache.maven.repository.internal.MavenRepositorySystemUtils.newSession();
        session.setLocalRepositoryManager(system.newLocalRepositoryManager(session, new LocalRepository(localRepository)));
        session.setSystemProperties(System.getProperties());
        session.setReadOnly();
        this.session = session;
        String central = System.getProperty(CENTRAL_PROPERTY, CENTRAL);
        repositories = system.newResolutionRepositories(session, List.of(new RemoteRepository.Builder("central", "default", central).build()));
    }

    /** The jar URLs for {@code coordinates} and everything they need at runtime. */
    List<URL> resolve(List<String> coordinates) throws DependencyResolutionException {
        if (coordinates.isEmpty()) return List.of();
        List<Dependency> dependencies = new ArrayList<>();
        for (String coordinate : coordinates) dependencies.add(new Dependency(new DefaultArtifact(coordinate), JavaScopes.RUNTIME));
        var request = new DependencyRequest(new CollectRequest((Dependency) null, dependencies, repositories), DependencyFilterUtils.classpathFilter(JavaScopes.RUNTIME));
        List<URL> urls = new ArrayList<>();
        for (ArtifactResult result : system.resolveDependencies(session, request).getArtifactResults()) {
            try { urls.add(result.getArtifact().getFile().toURI().toURL()); }
            catch (MalformedURLException e) { throw new IllegalStateException(e); }
        }
        return urls;
    }
}
