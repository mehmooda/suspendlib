pub fn print_exception(env: &jni::JNIEnv, thrown: jni::objects::JThrowable) {
    let y = env
        .call_method(thrown, "toString", "()Ljava/lang/String;", &[])
        .unwrap();
    let s = jni::objects::JString::from(y.l().unwrap());
    let o = env.get_string(s).unwrap();
    let v = o.to_str().unwrap();
    log::error!("{v}")
}
