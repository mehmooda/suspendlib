mod android;

use std::ffi::c_void;

use jni::objects::{GlobalRef, JObject, JValue};
use log::info;
use once_cell::sync::OnceCell;

lazy_static::lazy_static!(
    static ref RUNTIME: tokio::runtime::Runtime = tokio::runtime::Runtime::new().unwrap();
    static ref SENDER: tokio::sync::mpsc::UnboundedSender<Continuation> = unsafe { MPSC_SENDER.take() }.unwrap();
);

#[repr(transparent)]
struct Continuation(GlobalRef);

static mut MPSC_SENDER: Option<tokio::sync::mpsc::UnboundedSender<Continuation>> = None;

fn coroutine_suspended(env: &jni::JNIEnv) -> jni::sys::jobject {
    static CONTINUATION_SUSPENDED: OnceCell<GlobalRef> = OnceCell::new();

    let r = CONTINUATION_SUSPENDED.get_or_init(|| {
        let c = env.call_static_method(
            "kotlin/coroutines/intrinsics/IntrinsicsKt__IntrinsicsKt",
            "getCOROUTINE_SUSPENDED",
            "()Ljava/lang/Object;",
            &[],
        );
        env.new_global_ref(c.junwrap().l().junwrap()).junwrap()
    });

    r.as_obj().into_raw()
}

struct Job(GlobalRef);

trait JUnwrapJerror<T> {
    fn junwrap(self) -> T;
}

#[inline(never)]
#[cold]
#[track_caller]
fn junwrap_failed(msg: &str, error: &dyn std::fmt::Debug) -> ! {
    panic!("{msg}: {error:?}")
}
mod util;

#[inline(never)]
#[cold]
#[track_caller]
fn log_exception() {
    JAVA_VM.get_env().ok().and_then(|x| {
        let exc = x.exception_occurred().junwrap();
        x.exception_clear().junwrap();
        util::print_exception(&x, exc.clone());

        // todo Cause and StackTrace
        None::<()>
    });
}

impl<T> JUnwrapJerror<T> for jni::errors::Result<T> {
    #[inline(always)]
    #[track_caller]
    fn junwrap(self) -> T {
        match self {
            Ok(t) => t,
            Err(e) => {
                if let jni::errors::Error::JavaException = e {
                    log_exception()
                }
                junwrap_failed("called `Result::junwrap()` on an `Err` value", &e)
            }
        }
    }
}

static mut JAVA_VM_IMP: std::mem::MaybeUninit<jni::JavaVM> = std::mem::MaybeUninit::uninit();
static JAVA_VM: &jni::JavaVM = unsafe { &JAVA_VM_IMP.assume_init_ref() };

fn init_java_vm(with: jni::JavaVM) {
    unsafe { std::ptr::write(JAVA_VM_IMP.as_mut_ptr(), with) };
}

fn get_job(env: &jni::JNIEnv, continuation: &Continuation) -> Option<Job> {
    let context = env
        .call_method(
            continuation.0.as_obj(),
            "getContext",
            "()Lkotlin/coroutines/CoroutineContext;",
            &[],
        )
        .junwrap()
        .l()
        .junwrap();
    let job_key = env
        .get_static_field(
            "kotlinx/coroutines/Job",
            "Key",
            "Lkotlinx/coroutines/Job$Key;",
        )
        .junwrap();
    let job = env.call_method(context, "get", "(Lkotlin/coroutines/CoroutineContext$Key;)Lkotlin/coroutines/CoroutineContext$Element;", &[job_key]).junwrap().l().junwrap();
    Some(Job(env.new_global_ref(job).junwrap()))
}

mod native_callback;

fn make_cancellable(
    env: &jni::JNIEnv,
    continuation: JObject,
) -> (Continuation, tokio::sync::oneshot::Receiver<()>) {
    let (s, r) = tokio::sync::oneshot::channel::<()>();

    let s = Box::into_raw(Box::new(s)) as i64;

    let native_callback = native_callback::new(env, s);

    let cancellable = env
        .new_object(
            "kotlinx/coroutines/CancellableContinuationImpl",
            "(Lkotlin/coroutines/Continuation;I)V",
            &[continuation.into(), JValue::Int(1)],
        )
        .junwrap();
    env.call_method(cancellable, "initCancellability", "()V", &[])
        .junwrap();
    env.call_method(
        cancellable,
        "invokeOnCancellation",
        "(Lkotlin/jvm/functions/Function1;)V",
        &[native_callback.into()],
    )
    .junwrap();
    let ob = env
        .call_method(cancellable, "getResult", "()Ljava/lang/Object;", &[])
        .junwrap()
        .l()
        .junwrap();

    let x = env
        .is_same_object(ob, unsafe { JObject::from_raw(coroutine_suspended(&env)) })
        .junwrap();
    info!("getResult returned Suspended {x}");

    (Continuation(env.new_global_ref(cancellable).junwrap()), r)
}

fn continuation_thread(
    jvm: &jni::JavaVM,
    mut r: tokio::sync::mpsc::UnboundedReceiver<Continuation>,
) {
    info!("Continuation_Thread");
    let ag = jvm.attach_current_thread().junwrap();
    info!("Continuation_Attached");

    loop {
        let continuation = r.blocking_recv().unwrap();
        info!("Continuation_Recived");
        let call = ag.call_method(
            continuation.0.as_obj(),
            "resumeWith",
            "(Ljava/lang/Object;)V",
            &[jni::objects::JObject::null().into()],
        );
        match call {
            Err(jni::errors::Error::JavaException) => {
                ag.exception_describe().junwrap();
                ag.exception_clear().junwrap();
            }
            Err(x) => info!("Continuation Resume Error {}", x),
            Ok(_) => {
                info!("Continuation Resumed")
            }
        }
    }
}

#[no_mangle]
pub extern "system" fn JNI_OnLoad(jvm: jni::JavaVM, _reserved: c_void) -> i32 {
    android::init();

    init_java_vm(jvm);
    info!("JNI_Onload");

    //    jvm.get_env().junwrap().register_native_methods("uk/co/dcomp/ui/login/Native", &[NativeMethod{
    //        name: "Suspend".into(),
    //        sig: "(Lkotlin/coroutines/Continuation;)Ljava/lang/Object;".into(),
    //        fn_ptr: Java_uk_co_dcomp_ui_login_Native_Suspend as _
    //    }]).junwrap();

    let (s, r) = tokio::sync::mpsc::unbounded_channel();
    unsafe { MPSC_SENDER = Some(s) }

    std::thread::Builder::new()
        .name("Rust_JNI_Continuation_Thread".into())
        .spawn(|| continuation_thread(JAVA_VM, r))
        .unwrap();
    info!("JNI_Onload_ends");

    jni::JNIVersion::V6.into()
}

#[no_mangle]
pub extern "system" fn Java_uk_co_dcomp_ui_login_Native_Suspend(
    env: jni::JNIEnv,
    _this: JObject,
    continuation: JObject,
) -> jni::sys::jobject {
    info!("JNI_Method");
    let continuation = env
        .call_method(
            continuation,
            "intercepted",
            "()Lkotlin/coroutines/Continuation;",
            &[],
        )
        .junwrap()
        .l()
        .unwrap();
    let (continuation, r) = make_cancellable(&env, continuation);

    get_job(&env, &continuation);

    let jh = RUNTIME.spawn(statics_method(r));
    RUNTIME.spawn(async {
        jh.await.unwrap();
        SENDER.send(continuation)
    });
    info!("JNI_Method_ends");
    // Return CONTINUATION_SUSPENDED

    coroutine_suspended(&env)
}

async fn statics_method(cancel: tokio::sync::oneshot::Receiver<()>) {
    info!("Statics_Method");

    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
    info!("Statics_Running");

    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
    info!("Statics_Running");
    tokio::select! {
        _ = cancel => log::error!("STATICS METHOD CANCELLED"),
        _ = tokio::time::sleep(std::time::Duration::from_millis(2000)) => {},
    }
    info!("Statics_Method_ends");
}
