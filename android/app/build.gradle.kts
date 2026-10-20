plugins { id("com.android.application") }

android {
    namespace = "com.wannafire.vera20k"
    compileSdk = 36
    buildToolsVersion = "36.0.0"
    defaultConfig {
        applicationId = "com.wannafire.vera20k"
        minSdk = 29
        targetSdk = 35
        versionCode = providers.gradleProperty("packageVersionCode").orElse("1").get().toInt()
        versionName = providers.gradleProperty("packageVersionName").orElse("1.0").get()
        ndk { abiFilters += "arm64-v8a" }
    }
    signingConfigs {
        create("development") {
            val location = System.getenv("VERA_ANDROID_KEYSTORE")
            if (location != null) {
                storeFile = file(location)
                storePassword = System.getenv("VERA_ANDROID_STORE_PASSWORD")
                keyAlias = "vera20k-development"
                keyPassword = System.getenv("VERA_ANDROID_STORE_PASSWORD")
            }
        }
    }
    buildTypes {
        getByName("release") {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("development")
        }
    }
    sourceSets["main"].jniLibs.srcDir(rootProject.file("out/jniLibs"))
    sourceSets["main"].assets.srcDir(rootProject.file("out/assets"))
    sourceSets["main"].res.srcDir(rootProject.file("out/res"))
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    packaging { jniLibs { useLegacyPackaging = false } }
}

tasks.named("preBuild") {
    doFirst {
        check(rootProject.file("out/jniLibs/arm64-v8a/libmain.so").isFile) {
            "Build the native Rust Android library before packaging"
        }
        check(rootProject.file("out/assets/retail-manifest.json").isFile) {
            "Stage the local retail assets with tools/android/package.py"
        }
        check(System.getenv("VERA_ANDROID_KEYSTORE") != null) {
            "Use tools/android/package.py to select the persistent development signer"
        }
    }
}
