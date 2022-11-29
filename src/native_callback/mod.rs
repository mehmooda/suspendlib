use jni::{
    objects::{JObject, JThrowable},
    NativeMethod,
};
use log::info;

use crate::JUnwrapJerror;

//package suspendlib
//
//class NativeCallback(private val native: Long): kotlin.Function1<Throwable?, Unit> {
//    external override fun invoke(p1: Throwable?): Unit;
//    external fun delete_native_side();
//    protected fun finalize() {
//        delete_native_side()
//    }
//}
static PRECOMPILED_DEX: &[u8] = include_bytes!("nativeCallback.dex");
static NATIVE_CALLBACK_CLASS: once_cell::sync::OnceCell<jni::objects::GlobalRef> =
    once_cell::sync::OnceCell::new();

#[inline(always)]
fn get_nativecallback_class<'a>(env: &jni::JNIEnv<'a>) -> &'static jni::objects::GlobalRef {
    NATIVE_CALLBACK_CLASS.get_or_init(|| {
        let x = unsafe {
            env.new_direct_byte_buffer(PRECOMPILED_DEX as *const _ as *mut _, PRECOMPILED_DEX.len())
        }
        .junwrap();

        let kotlin_interface = env.find_class("kotlin/jvm/functions/Function1").junwrap();
        let native_class_loader = env
            .call_method(
                kotlin_interface,
                "getClassLoader",
                "()Ljava/lang/ClassLoader;",
                &[],
            )
            .junwrap();

        let class_loader = env
            .new_object(
                "dalvik/system/InMemoryDexClassLoader",
                "(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V",
                &[x.into(), native_class_loader],
            )
            .junwrap();

        let native_callback_class = env
            .call_method(
                class_loader,
                "loadClass",
                "(Ljava/lang/String;)Ljava/lang/Class;",
                &[env.new_string("suspendlib.NativeCallback").junwrap().into()],
            )
            .junwrap()
            .l()
            .junwrap();

        env.register_native_methods(
            native_callback_class,
            &[
                NativeMethod {
                    name: "invoke".into(),
                    sig: "(Ljava/lang/Throwable;)Ljava/lang/Object;".into(),
                    fn_ptr: invoke as *mut _,
                },
                NativeMethod {
                    name: "delete_native_side".into(),
                    sig: "()V".into(),
                    fn_ptr: delete_native_side as *mut _,
                },
            ],
        )
        .junwrap();

        env.new_global_ref(native_callback_class).junwrap()
    })
}

pub fn new<'a>(env: &jni::JNIEnv<'a>, native: i64) -> jni::objects::JObject<'a> {
    let ncc = get_nativecallback_class(env);

    env.new_object(ncc, "(J)V", &[native.into()]).junwrap()
}

extern "system" fn invoke(env: jni::JNIEnv, this: JObject, throwable: JThrowable) {
    info!("FAKE_INVOKE");
    crate::util::print_exception(&env, throwable);
    let native = env.get_field(this, "native", "J").junwrap().j().unwrap();
    info!("NATIVE {native}");

    let n = native as *mut tokio::sync::oneshot::Sender<()>;
    let sender = unsafe { Box::from_raw(n) };
    let _ = sender.send(()).map_err(|_| log::warn!("RECIEVER MISSING"));
    env.set_field(this, "native", "J", 0i64.into()).junwrap();
}

extern "system" fn delete_native_side(env: jni::JNIEnv, this: JObject) {
    info!("FAKE_DELETE");
    let native = env.get_field(this, "native", "J").junwrap().j().unwrap();
    info!("NATIVE {native}");
    if native != 0 {
        unsafe { Box::from_raw(native as *mut tokio::sync::oneshot::Sender<()>) };
    }
}
