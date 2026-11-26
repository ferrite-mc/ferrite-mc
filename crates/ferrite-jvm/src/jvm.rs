//! Starting and driving the process-wide JVM.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};

use jni::objects::{JObject, JString, JThrowable, JValue, JValueOwned};
use jni::{InitArgsBuilder, JNIEnv, JavaVM};

use crate::config::JvmConfig;
use crate::discovery::{self, JavaHome};
use crate::error::{JvmError, Result};
use crate::native::NativeMethod;
use crate::thread::{self, JvmThreadMode, ThreadAttach};

/// The one JVM this process can ever create. See [`Jvm::start`].
static JVM: OnceLock<JvmInner> = OnceLock::new();

/// Serializes [`Jvm::start`] so two threads cannot race into `JNI_CreateJavaVM`.
static START_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug)]
struct JvmInner {
    vm: JavaVM,
    java_home: JavaHome,
    classpath: Vec<PathBuf>,
    thread_mode: JvmThreadMode,
}

/// A handle to the JVM embedded in this process.
///
/// Starting the JVM is process-global and cannot be undone: JNI cannot fully destroy a VM, so it
/// lives until the process exits. Copies of this handle are cheap and refer to the same VM.
#[derive(Debug, Clone, Copy)]
pub struct Jvm {
    inner: &'static JvmInner,
}

impl Jvm {
    /// Finds Java 25+, loads its JVM shared library and starts the VM.
    ///
    /// The JVM shared library is loaded at runtime with the platform's dynamic loader, so Ferrite
    /// itself never links against `jvm.lib` or `libjvm`. `config.java_home` is used when set;
    /// otherwise discovery falls back to `JAVA_HOME` and then the common install directories.
    ///
    /// # Errors
    ///
    /// Returns [`JvmError::AlreadyStarted`] when a JVM already exists in this process, since JNI
    /// allows only one.
    pub fn start(config: JvmConfig) -> Result<Self> {
        let _start = START_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        if JVM.get().is_some() {
            return Err(JvmError::AlreadyStarted);
        }

        let java_home = discovery::find_java_home(config.java_home.as_deref())?;
        let library = java_home.jvm_library()?;
        let args = build_init_args(&config)?;
        tracing::debug!(
            library = %library.display(),
            classpath_entries = config.classpath.len(),
            "starting embedded JVM"
        );

        let vm = JavaVM::with_libjvm(args, || Ok(library))?;
        tracing::info!(
            java_home = %java_home.root().display(),
            java_version = java_home.version(),
            "embedded JVM started"
        );

        let _ = JVM.set(JvmInner {
            vm,
            java_home,
            classpath: config.classpath,
            thread_mode: JvmThreadMode::default(),
        });
        Ok(Self {
            inner: JVM.get().expect("the JVM was just initialized"),
        })
    }

    /// Returns the running VM, or `None` if [`Jvm::start`] has not succeeded yet.
    pub fn get() -> Option<Self> {
        JVM.get().map(|inner| Self { inner })
    }

    /// The Java installation backing this VM.
    pub fn java_home(&self) -> &JavaHome {
        &self.inner.java_home
    }

    /// Classpath entries the VM was started with.
    pub fn classpath(&self) -> &[PathBuf] {
        &self.inner.classpath
    }

    /// How the plugin bridge intends to schedule JVM work; see [`JvmThreadMode`].
    pub fn thread_mode(&self) -> JvmThreadMode {
        self.inner.thread_mode
    }

    /// Attaches the calling thread to the JVM as a daemon thread, cached thread-locally.
    ///
    /// Daemon threads do not keep the VM alive. The attachment lasts until the thread exits; see
    /// [`ThreadAttach`].
    pub fn attach_thread(&self) -> Result<ThreadAttach> {
        thread::attach(&self.inner.vm)
    }

    /// A JNI environment for the calling thread, attaching it as a daemon if necessary.
    pub fn env(&self) -> Result<JNIEnv<'static>> {
        self.attach_thread()?;
        Ok(self.inner.vm.get_env()?)
    }

    /// Runs `f` with a JNI environment for the calling thread.
    ///
    /// An exception left pending by `f` is cleared and returned as [`JvmError::JavaException`]
    /// carrying the Java stack trace, so the thread stays usable for later JNI calls.
    pub fn with_env<T>(
        &self,
        f: impl FnOnce(&mut JNIEnv<'_>) -> jni::errors::Result<T>,
    ) -> Result<T> {
        let _attachment = self.attach_thread()?;
        let mut env = self.env()?;
        let result = f(&mut env);
        if env.exception_check().unwrap_or(false) {
            return Err(JvmError::JavaException(exception_string(&mut env)));
        }
        result.map_err(JvmError::from)
    }

    /// Calls the static method `class.name` with JNI signature `signature`.
    ///
    /// ```no_run
    /// # use ferrite_jvm::{Jvm, JvmError};
    /// # use ferrite_jvm::jni::objects::JValue;
    /// # fn example(jvm: Jvm) -> Result<(), JvmError> {
    /// let sum: i32 = jvm.call_static(
    ///     "com/example/Bridge",
    ///     "add",
    ///     "(II)I",
    ///     &[JValue::Int(1), JValue::Int(2)],
    /// )?;
    /// # assert_eq!(sum, 3);
    /// # Ok(())
    /// # }
    /// ```
    pub fn call_static<T: FromJavaValue>(
        &self,
        class: &str,
        name: &str,
        signature: &str,
        args: &[JValue<'_, '_>],
    ) -> Result<T> {
        self.with_env(|env| {
            let value = env.call_static_method(class, name, signature, args)?;
            T::from_java_value(env, value)
        })
    }

    /// Binds Rust functions to the `native` methods of a Java class with `RegisterNatives`.
    ///
    /// Per the JNI specification, registering a method again for the same class replaces the
    /// previous binding.
    pub fn register_natives(&self, class: &str, methods: &[NativeMethod]) -> Result<()> {
        self.with_env(|env| {
            let methods: Vec<jni::NativeMethod> = methods
                .iter()
                .map(|method| jni::NativeMethod {
                    name: method.name.into(),
                    sig: method.signature.into(),
                    fn_ptr: method.fn_ptr,
                })
                .collect();
            env.register_native_methods(class, &methods)
        })
    }
}

/// Conversion of a Java return value into a Rust value.
///
/// Implemented for the primitives the plugin bridge uses, `String` and `()`.
pub trait FromJavaValue: Sized {
    /// Converts `value`, which came from a JNI call on `env`.
    fn from_java_value(env: &mut JNIEnv<'_>, value: JValueOwned<'_>) -> jni::errors::Result<Self>;
}

impl FromJavaValue for () {
    fn from_java_value(_env: &mut JNIEnv<'_>, value: JValueOwned<'_>) -> jni::errors::Result<Self> {
        value.v()
    }
}

impl FromJavaValue for bool {
    fn from_java_value(_env: &mut JNIEnv<'_>, value: JValueOwned<'_>) -> jni::errors::Result<Self> {
        value.z()
    }
}

impl FromJavaValue for i32 {
    fn from_java_value(_env: &mut JNIEnv<'_>, value: JValueOwned<'_>) -> jni::errors::Result<Self> {
        value.i()
    }
}

impl FromJavaValue for i64 {
    fn from_java_value(_env: &mut JNIEnv<'_>, value: JValueOwned<'_>) -> jni::errors::Result<Self> {
        value.j()
    }
}

impl FromJavaValue for f64 {
    fn from_java_value(_env: &mut JNIEnv<'_>, value: JValueOwned<'_>) -> jni::errors::Result<Self> {
        value.d()
    }
}

impl FromJavaValue for String {
    fn from_java_value(env: &mut JNIEnv<'_>, value: JValueOwned<'_>) -> jni::errors::Result<Self> {
        let object = value.l()?;
        let string = JString::from(object);
        Ok(env.get_string(&string)?.into())
    }
}

/// Builds the `JavaVMInitArgs` for a config: classpath, heap limit and extra options.
fn build_init_args(config: &JvmConfig) -> Result<jni::InitArgs<'static>> {
    let mut builder = InitArgsBuilder::new();
    if !config.classpath.is_empty() {
        let joined = std::env::join_paths(&config.classpath)
            .map_err(|error| JvmError::Config(format!("invalid classpath: {error}")))?;
        builder = builder.option(format!("-Djava.class.path={}", joined.to_string_lossy()));
    }
    if let Some(max_heap_mb) = config.max_heap_mb {
        builder = builder.option(format!("-Xmx{max_heap_mb}m"));
    }
    for option in &config.extra_args {
        builder = builder.option(option.clone());
    }
    Ok(builder.build()?)
}

/// Renders and clears the pending Java exception as `Throwable.printStackTrace()` output.
fn exception_string(env: &mut JNIEnv<'_>) -> String {
    if !env.exception_check().unwrap_or(false) {
        return "unknown Java exception (nothing pending)".to_owned();
    }
    let throwable = match env.exception_occurred() {
        Ok(throwable) => throwable,
        Err(error) => {
            let _ = env.exception_clear();
            return format!("unknown Java exception ({error})");
        }
    };
    let _ = env.exception_clear();
    print_stack_trace(env, &throwable).unwrap_or_else(|| describe(env, &throwable))
}

/// Captures `throwable.printStackTrace(new PrintWriter(new StringWriter()))`.
fn print_stack_trace(env: &mut JNIEnv<'_>, throwable: &JThrowable<'_>) -> Option<String> {
    let writer = env.new_object("java/io/StringWriter", "()V", &[]).ok()?;
    let printer = env
        .new_object(
            "java/io/PrintWriter",
            "(Ljava/io/Writer;)V",
            &[JValue::Object(&writer)],
        )
        .ok()?;
    let throwable: &JObject<'_> = throwable;
    env.call_method(
        throwable,
        "printStackTrace",
        "(Ljava/io/PrintWriter;)V",
        &[JValue::Object(&printer)],
    )
    .ok()?;
    env.call_method(&printer, "flush", "()V", &[]).ok()?;
    let text = env
        .call_method(&writer, "toString", "()Ljava/lang/String;", &[])
        .ok()?
        .l()
        .ok()?;
    let text = JString::from(text);
    let text: String = env.get_string(&text).ok()?.into();
    Some(text.trim_end().to_owned())
}

/// Fallback when capturing the stack trace fails: `ClassName: message`.
fn describe(env: &mut JNIEnv<'_>, throwable: &JThrowable<'_>) -> String {
    let class = env.get_object_class(throwable).ok().and_then(|class| {
        let name = env
            .call_method(class, "getName", "()Ljava/lang/String;", &[])
            .ok()?
            .l()
            .ok()?;
        let name = JString::from(name);
        let name: String = env.get_string(&name).ok()?.into();
        Some(name)
    });
    let message: Option<String> = env
        .call_method(throwable, "getMessage", "()Ljava/lang/String;", &[])
        .ok()
        .and_then(|value| value.l().ok())
        .map(JString::from)
        .and_then(|message| env.get_string(&message).ok().map(Into::into));
    match (class, message) {
        (Some(class), Some(message)) => format!("{class}: {message}"),
        (Some(class), None) => class,
        _ => "java.lang.Throwable".to_owned(),
    }
}
