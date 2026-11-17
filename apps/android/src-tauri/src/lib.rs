//! O Prancha no Android: os mesmos comandos do desktop (crates/prancha-app), com a configuração deste app.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    prancha_app::run(tauri::generate_context!())
}

/// O `FollowWorker` (WorkManager) verifica as obras acompanhadas com o app fechado. Devolve os avisos em JSON.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_app_prancha_PranchaNative_checkFollowing<'caller>(
    mut env: jni::EnvUnowned<'caller>,
    _class: jni::objects::JClass<'caller>,
    data_dir: jni::objects::JString<'caller>,
    context: jni::objects::JObject<'caller>,
) -> jni::objects::JString<'caller> {
    env.with_env(|env| -> Result<_, jni::errors::Error> {
        let dir = data_dir.try_to_string(env)?;
        let json = prancha_app::android::check_following(env, context, dir.into()).unwrap_or_else(|e| {
            log::warn!("verificação em segundo plano: {e}");
            "[]".into()
        });
        jni::objects::JString::from_str(env, json)
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}
