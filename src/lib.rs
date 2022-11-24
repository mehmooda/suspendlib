use std::{ffi::c_void, ptr::null};

use jni::{objects::{GlobalRef, JObject}, NativeMethod};
use once_cell::sync::OnceCell;

lazy_static::lazy_static!(
    static ref RUNTIME: tokio::runtime::Runtime = tokio::runtime::Runtime::new().unwrap();
    static ref SENDER: tokio::sync::mpsc::UnboundedSender<Continuation> = unsafe { MPSC_SENDER.take() }.unwrap();
);

#[repr(transparent)]
struct Continuation(GlobalRef);

static mut MPSC_SENDER: Option<tokio::sync::mpsc::UnboundedSender<Continuation>> = None;

fn COROUTINE_SUSPENDED(env: jni::JNIEnv) -> jni::sys::jobject {
    static CONTINUATION_SUSPENDED: OnceCell<GlobalRef> = OnceCell::new();

    let r = CONTINUATION_SUSPENDED.get_or_init(|| {
        let c = env.call_static_method(
            "kotlin/coroutines/intrinsics/IntrinsicsKt__IntrinsicsKt",
            "getCOROUTINE_SUSPENDED",
            "()Ljava/lang/Object;",
            &[],
        );
        env.new_global_ref(c.unwrap().l().unwrap()).unwrap()
    });

    r.as_obj().into_inner()
}

struct Job(GlobalRef);

fn get_job(env: &jni::JNIEnv, continuation: &Continuation) -> Option<Job> {

    let context = env.call_method(continuation.0.as_obj(), "getContext", "()Lkotlinx/coroutines/CoroutineContext;", &[]).unwrap().l().unwrap();
    dbg!(context);
    let job_key = env.get_static_field("kotlinx/coroutines/Job", "Key", "Lkotlinx/coroutines/Job$Key;").unwrap();
    dbg!(job_key);
    let job = env.call_method(context, "get", "(Lkotlin/coroutines/CoroutineContext$Key;)Lkotlinx/coroutines/CoroutineContext$Element;", &[job_key]).unwrap();
    dbg!(job);    
    None
}

fn continuation_thread(jvm: jni::JavaVM) {
    println!("Continuation_Thread");
    let ag = jvm.attach_current_thread().unwrap();
    println!("Continuation_Attached");
    let (s, mut r) = tokio::sync::mpsc::unbounded_channel();
    unsafe { MPSC_SENDER = Some(s) }
    loop {
        let continuation = r.blocking_recv().unwrap();
        println!("Continuation_Recived");
        let call = ag.call_method(
            continuation.0.as_obj(),
            "resumeWith",
            "(Ljava/lang/Object;)V",
            &[jni::objects::JObject::null().into()],
        );
        match call {
            Err(jni::errors::Error::JavaException) => {
                ag.exception_describe().unwrap();
                ag.exception_clear().unwrap();
            }
            Err(x) => println!("Continuation Resume Error {}", x),
            Ok(_) => {
                println!("Continuation Resumed")
            }
        }
    }
}

#[no_mangle]
pub extern "system" fn JNI_OnLoad(jvm: jni::JavaVM, reserved: c_void) -> i32 {
    println!("JNI_Onload");

    jvm.get_env().unwrap().register_native_methods("uk/co/dcomp/ui/login/Native", &[NativeMethod{
        name: "Suspend".into(),
        sig: "(Lkotlin/coroutines/Continuation;)Ljava/lang/Object;".into(),
        fn_ptr: Java_uk_co_dcomp_ui_login_Native_Suspend as _
    }]).unwrap();


    std::thread::Builder::new()
        .name("Rust_JNI_Continuation_Thread".into())
        .spawn(|| continuation_thread(jvm))
        .unwrap();
    println!("JNI_Onload_ends");



    jni::JNIVersion::V8.into()
}

#[no_mangle]
extern "system" fn Java_uk_co_dcomp_ui_login_Native_Suspend(
    env: jni::JNIEnv,
    this: JObject,
    continuation: JObject,
) -> jni::sys::jobject {
    println!("JNI_Method");
    let continuation = Continuation(env.new_global_ref(continuation).unwrap());

    get_job(&env, &continuation);

    let jh = RUNTIME.spawn(Statics_Method());
    RUNTIME.spawn(async {
        let res = jh.await;
        SENDER.send(continuation)
    });
    println!("JNI_Method_ends");
    // Return CONTINUATION_SUSPENDED

    COROUTINE_SUSPENDED(env)
}

//jni_suspend_handler! { Java_uk_co_dcomp_ui_login_Native_Suspend, Statics_Method, JObject -> {
//
//},
//JInt, JInt,
//};

async fn Statics_Method() {
    println!("Statics_Method");
    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
    println!("Statics_Running");
    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
    println!("Statics_Running");
    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
    println!("Statics_Method_ends");
}
