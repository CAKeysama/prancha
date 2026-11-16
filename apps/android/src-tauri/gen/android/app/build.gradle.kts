import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.serialization")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

android {
    compileSdk = 37
    namespace = "app.prancha"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "app.prancha"
        minSdk = 24
        targetSdk = 37
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    // A chave do APK publicado vem do workflow de release (.github/workflows/release.yml, ver docs/RELEASING.md),
    // nunca do repositório. Sem ela, o APK de release sai sem assinatura; o de depuração usa a chave de depuração.
    val releaseKeystore = System.getenv("PRANCHA_KEYSTORE")
    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword = System.getenv("PRANCHA_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("PRANCHA_KEY_ALIAS")
                keyPassword = System.getenv("PRANCHA_KEY_PASSWORD")
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            if (releaseKeystore != null) {
                signingConfig = signingConfigs.getByName("release")
            }
            optimization {
               enable = true
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
        // O jsoup e o OkHttp das extensões do Mihon usam APIs do Java que o Android 7 não tem.
        isCoreLibraryDesugaringEnabled = true
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    // Verificação de capítulos novos com o app fechado (FollowWorker).
    implementation("androidx.work:work-runtime-ktx:2.10.0")
    // Componente Android do verificador de certificados do núcleo (rustls-platform-verifier-android no Cargo.lock).
    implementation("org.rustls:rustls-platform-verifier:0.2.0")
    // O que as extensões do Mihon esperam encontrar no app (MihonPlugin, código em java/eu/kanade). As versões
    // acompanham as do Mihon e as do repositório de extensões do Keiyoushi.
    implementation("com.squareup.okhttp3:okhttp:5.5.1")
    implementation("com.squareup.okhttp3:okhttp-brotli:5.5.1")
    implementation("com.squareup.okhttp3:okhttp-zstd:5.5.1")
    implementation("org.jsoup:jsoup:1.24.1")
    implementation("io.reactivex:rxjava:1.3.8")
    implementation("uy.kohesive.injekt:injekt-core:1.16.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.13.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json-okio:1.13.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-protobuf:1.13.0")
    implementation("androidx.preference:preference-ktx:1.2.1")
    implementation("app.cash.quickjs:quickjs-android:0.9.2")
    coreLibraryDesugaring("com.android.tools:desugar_jdk_libs:2.1.5")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = file("tauri.build.gradle.kts"))
