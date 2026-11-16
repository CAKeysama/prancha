buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:9.3.1")
        // A versão que compila as bibliotecas das extensões do Mihon (kotlinx.serialization e coroutines).
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.4.10")
        classpath("org.jetbrains.kotlin:kotlin-serialization:2.4.10")
    }
}

allprojects {
    repositories {
        google()
        mavenCentral()
        // Onde o rustls-platform-verifier publica o componente Android (não está no Maven Central).
        maven { url = uri("https://github.com/rustls/rustls-platform-verifier/raw/maven-archive/android-release-support/maven/") }
    }
}

tasks.register("clean").configure {
    delete("build")
}

