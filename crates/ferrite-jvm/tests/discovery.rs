//! Integration tests for Java installation discovery.
//!
//! If no Java 25+ installation is available the test prints a skip message and passes, since the
//! rest of the workspace must stay testable on machines without a JDK.

use ferrite_jvm::{MIN_JAVA_VERSION, find_java_home};

#[test]
fn discovers_a_java_25_installation() {
    match find_java_home(None) {
        Ok(java_home) => {
            assert!(java_home.version() >= MIN_JAVA_VERSION);
            java_home
                .jvm_library()
                .expect("a located JDK must ship a JVM library");
        }
        Err(error) => eprintln!("skipping discovery test: {error}"),
    }
}
