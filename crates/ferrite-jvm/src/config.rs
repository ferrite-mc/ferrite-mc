//! Configuration for [`Jvm::start`](crate::Jvm::start).

use std::path::PathBuf;

/// Everything needed to start the embedded JVM.
#[derive(Debug, Clone, Default)]
pub struct JvmConfig {
    /// Explicit JDK root. When set it must be a Java 25+ home; discovery does not fall back to
    /// other installations. When `None`, `JAVA_HOME` and the common install directories are
    /// searched.
    pub java_home: Option<PathBuf>,

    /// Entries for `java.class.path`, in order.
    pub classpath: Vec<PathBuf>,

    /// Value for `-Xmx`, in megabytes. `None` leaves the JVM's ergonomic default (a quarter of
    /// the machine's memory).
    pub max_heap_mb: Option<u32>,

    /// Additional JVM options, passed through verbatim.
    pub extra_args: Vec<String>,
}
