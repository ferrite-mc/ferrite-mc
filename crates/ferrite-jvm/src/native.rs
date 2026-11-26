//! Rust functions bound to Java `native` methods.

use std::ffi::c_void;

/// One entry of a native method table, as passed to
/// [`Jvm::register_natives`](crate::Jvm::register_natives) and then to the JVM's
/// `RegisterNatives`.
///
/// `fn_ptr` must point to an `extern "system"` function whose parameters match `signature`:
/// `(env: *mut JNIEnv, class: jclass, ...args) -> return` for a static native method, or
/// `(env: *mut JNIEnv, this: jobject, ...args) -> return` for an instance method.
#[derive(Debug, Clone, Copy)]
pub struct NativeMethod {
    /// Java method name.
    pub name: &'static str,
    /// JNI method signature, for example `(II)I`.
    pub signature: &'static str,
    /// Function pointer; see the type-level documentation.
    pub fn_ptr: *mut c_void,
}

impl NativeMethod {
    /// Creates a table entry.
    pub const fn new(name: &'static str, signature: &'static str, fn_ptr: *mut c_void) -> Self {
        Self {
            name,
            signature,
            fn_ptr,
        }
    }
}
