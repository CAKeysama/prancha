plugins {
    `kotlin-dsl`
}

gradlePlugin {
    plugins {
        create("pluginsForCoolKids") {
            id = "rust"
            implementationClass = "RustPlugin"
        }
    }
}

repositories {
    google()
    mavenCentral()
}

dependencies {
    compileOnly(gradleApi())
    implementation("com.android.tools.build:gradle:9.3.1")
    // O AGP traz o plugin do Kotlin 2.2, que não lê as bibliotecas das extensões do Mihon; o buildSrc vence o
    // buildscript da raiz, então a versão nova precisa estar aqui também.
    implementation("org.jetbrains.kotlin:kotlin-gradle-plugin:2.4.10")
}

