//! End-to-end tests for the embedded JVM bootstrap.
//!
//! A JVM can only be created once per process, so everything lives in this single test binary:
//! [`bridge`] compiles the Java source once, starts the JVM once, and every test reuses it. Tests
//! take a global lock so they run serially against that one JVM.
//!
//! If no Java 25+ installation is available the tests print a skip message and pass, since the
//! rest of the workspace must stay testable on machines without a JDK.

use std::ffi::c_void;
use std::fs;
use std::process::Command;
use std::sync::{Mutex, MutexGuard, OnceLock};

use ferrite_jvm::jni::objects::JValue;
use ferrite_jvm::jni::sys::{JNIEnv, jclass, jint};
use ferrite_jvm::{
    JavaHome, Jvm, JvmConfig, JvmError, MIN_JAVA_VERSION, NativeMethod, find_java_home,
};

/// `Bridge.callAdd` is plain Java and calls the `native` `add`, which is bound to [`rust_add`].
const BRIDGE_JAVA: &str = r#"
public final class Bridge {
    public static native int add(int a, int b);

    public static int callAdd(int a, int b) {
        return add(a, b);
    }

    public static int boom() {
        throw new IllegalStateException("kaboom from Java");
    }
}
"#;

/// Java's `add`, implemented in Rust and registered with `RegisterNatives`.
extern "system" fn rust_add(_env: *mut JNIEnv, _class: jclass, a: jint, b: jint) -> jint {
    a + b
}

/// Serializes the tests against the single JVM.
static SERIAL: Mutex<()> = Mutex::new(());
static BRIDGE: OnceLock<Option<Bridge>> = OnceLock::new();

struct Bridge {
    jvm: Jvm,
    /// Kept alive so the compiled classes stay on disk for the VM's lifetime.
    _classes: tempfile::TempDir,
}

/// Compiles [`BRIDGE_JAVA`] and starts the JVM on the first call.
///
/// Returns `None` (and prints why) when no Java 25+ installation is available.
fn bridge() -> Option<&'static Bridge> {
    BRIDGE
        .get_or_init(|| {
            let java_home = match find_java_home(None) {
                Ok(java_home) => java_home,
                Err(error) => {
                    eprintln!("skipping JVM tests: no Java {MIN_JAVA_VERSION}+ found ({error})");
                    return None;
                }
            };
            let classes = compile_bridge(&java_home);
            let jvm = Jvm::start(JvmConfig {
                java_home: Some(java_home.root().to_owned()),
                classpath: vec![classes.path().to_owned()],
                max_heap_mb: Some(256),
                extra_args: Vec::new(),
            })
            .expect("JVM should start");
            Some(Bridge {
                jvm,
                _classes: classes,
            })
        })
        .as_ref()
}

fn compile_bridge(java_home: &JavaHome) -> tempfile::TempDir {
    let classes = tempfile::tempdir().expect("temp dir");
    let source = classes.path().join("Bridge.java");
    fs::write(&source, BRIDGE_JAVA).expect("write Bridge.java");
    let output = Command::new(java_home.javac())
        .arg("-d")
        .arg(classes.path())
        .arg(&source)
        .output()
        .unwrap_or_else(|error| panic!("failed to run {}: {error}", java_home.javac().display()));
    assert!(
        output.status.success(),
        "javac failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    classes
}

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|poison| poison.into_inner())
}

#[test]
fn discovers_a_java_25_installation() {
    let _serial = serial();
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

#[test]
fn rust_native_is_called_from_java() {
    let _serial = serial();
    let Some(bridge) = bridge() else { return };
    bridge
        .jvm
        .register_natives(
            "Bridge",
            &[NativeMethod::new("add", "(II)I", rust_add as *mut c_void)],
        )
        .expect("register Bridge.add");
    let sum: i32 = bridge
        .jvm
        .call_static(
            "Bridge",
            "callAdd",
            "(II)I",
            &[JValue::Int(2), JValue::Int(3)],
        )
        .expect("call Bridge.callAdd");
    assert_eq!(sum, 5);
}

#[test]
fn java_exception_becomes_a_rust_error_with_stack_trace() {
    let _serial = serial();
    let Some(bridge) = bridge() else { return };
    let error = bridge
        .jvm
        .call_static::<i32>("Bridge", "boom", "()I", &[])
        .expect_err("Bridge.boom must throw");
    let message = error.to_string();
    assert!(
        message.contains("kaboom from Java"),
        "error should contain the Java message:\n{message}"
    );
    assert!(
        message.contains("Bridge.boom"),
        "error should contain the Java stack trace:\n{message}"
    );
}

#[test]
fn a_second_jvm_is_rejected() {
    let _serial = serial();
    let Some(_bridge) = bridge() else { return };
    let error = Jvm::start(JvmConfig {
        max_heap_mb: Some(64),
        ..JvmConfig::default()
    })
    .expect_err("a second JVM must not start");
    assert!(
        matches!(error, JvmError::AlreadyStarted),
        "unexpected error: {error}"
    );
}
