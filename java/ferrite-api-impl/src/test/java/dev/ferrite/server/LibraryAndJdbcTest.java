package dev.ferrite.server;

import java.io.IOException;
import java.net.URLClassLoader;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.List;
import java.util.jar.JarEntry;
import java.util.jar.JarOutputStream;
import org.junit.jupiter.api.Test;
import org.junit.jupiter.api.io.TempDir;
import static org.junit.jupiter.api.Assertions.*;

class LibraryAndJdbcTest {
    @Test void bundledSqliteDriverServesPlainJdbc() throws Exception {
        try (var connection = java.sql.DriverManager.getConnection("jdbc:sqlite::memory:"); var statement = connection.createStatement()) {
            statement.execute("create table homes (player text primary key, x integer)");
            statement.execute("insert into homes values ('Steve', 42)");
            try (var rows = statement.executeQuery("select x from homes where player = 'Steve'")) {
                assertTrue(rows.next());
                assertEquals(42, rows.getInt(1));
            }
        }
    }

    @Test void bundledMysqlDriverIsRegistered() throws Exception {
        assertTrue(java.sql.DriverManager.getDriver("jdbc:mysql://localhost/test") instanceof com.mysql.cj.jdbc.Driver);
        assertEquals("org.sqlite.JDBC", Class.forName("org.sqlite.JDBC").getName());
    }

    @Test void librariesResolveWithRuntimeDependencies(@TempDir Path temp) throws Exception {
        Path repository = temp.resolve("repo");
        publish(repository, "lib", "1.0", "lib.txt", "<dependencies><dependency><groupId>dev.ferrite.test</groupId><artifactId>dep</artifactId><version>2.0</version></dependency>"
            + "<dependency><groupId>dev.ferrite.test</groupId><artifactId>tooling</artifactId><version>1.0</version><scope>test</scope></dependency></dependencies>");
        publish(repository, "dep", "2.0", "dep.txt", "");
        String previous = System.getProperty(FerriteLibraryLoader.CENTRAL_PROPERTY);
        System.setProperty(FerriteLibraryLoader.CENTRAL_PROPERTY, repository.toUri().toString());
        try {
            var urls = new FerriteLibraryLoader(temp.resolve("libraries").toFile()).resolve(List.of("dev.ferrite.test:lib:1.0"));
            assertEquals(2, urls.size(), "the library and its runtime dependency, not test-scoped ones: " + urls);
            try (var loader = new URLClassLoader(urls.toArray(java.net.URL[]::new), null)) {
                assertNotNull(loader.getResource("lib.txt"));
                assertNotNull(loader.getResource("dep.txt"));
            }
            assertTrue(Files.exists(temp.resolve("libraries/dev/ferrite/test/dep/2.0/dep-2.0.jar")), "downloads land in libraries/");
            assertTrue(new FerriteLibraryLoader(temp.resolve("libraries").toFile()).resolve(List.of()).isEmpty());
        } finally {
            if (previous == null) System.clearProperty(FerriteLibraryLoader.CENTRAL_PROPERTY); else System.setProperty(FerriteLibraryLoader.CENTRAL_PROPERTY, previous);
        }
    }

    private static void publish(Path repository, String artifact, String version, String resource, String dependencies) throws IOException {
        Path dir = repository.resolve("dev/ferrite/test/" + artifact + "/" + version);
        Files.createDirectories(dir);
        Files.writeString(dir.resolve(artifact + "-" + version + ".pom"), "<project><modelVersion>4.0.0</modelVersion><groupId>dev.ferrite.test</groupId><artifactId>" + artifact + "</artifactId><version>" + version + "</version>" + dependencies + "</project>");
        try (var jar = new JarOutputStream(Files.newOutputStream(dir.resolve(artifact + "-" + version + ".jar")))) {
            jar.putNextEntry(new JarEntry(resource));
            jar.write(artifact.getBytes());
            jar.closeEntry();
        }
    }
}
