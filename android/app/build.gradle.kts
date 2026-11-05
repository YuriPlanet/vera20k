plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
}

// versionName reads the Cargo package version, so the APK and the binary inside
// it cannot drift apart (they did once: a 0.2.7 bump changed Cargo.toml while
// this file still said 0.2.6). versionCode is a monotonically rising integer
// that Play and sideload both compare, so it cannot be derived from the version
// string and has to be bumped by hand on every upload.
val veraVersionName: String = rootProject.file("../Cargo.toml")
    .readLines()
    .first { it.trimStart().startsWith("version = ") }
    .substringAfter('"')
    .substringBefore('"')
val veraVersionCode = 207

android {
    namespace = "org.vera20k"
    compileSdk = 36
    // Pinned to the installed version: AGP otherwise downloads the build-tools
    // release it prefers, which fails on a machine without network access to
    // dl.google.com in the middle of a build.
    buildToolsVersion = "36.1.0"
    // NDK r28 aligns LOAD segments for Android 15's 16 KB pages by default;
    // r27 and older need -Wl,-z,max-page-size=16384 instead (see P9).
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "org.vera20k"
        // cpal 0.17's Android backend is AAudio, which needs API 26.
        minSdk = 26
        targetSdk = 36
        versionCode = veraVersionCode
        versionName = veraVersionName

        // arm64 is the Play requirement and the only ABI worth a real device
        // build; x86_64 exists for the emulator. armeabi-v7a is deliberately
        // absent — 32-bit address space cannot hold a 20k-unit game.
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    // The debug keystore AGP generates lives in the checkout, not in
    // %USERPROFILE%\.android. That folder is not writable in a sandboxed shell
    // or most CI containers, and validateSigningDebug then fails on
    // `debug.keystore.lock` after every other task has already succeeded.
    signingConfigs {
        getByName("debug") {
            storeFile = rootProject.file(".android/debug.keystore")
        }
    }

    buildTypes {
        debug {
            isMinifyEnabled = false
        }
        release {
            // Keep JVM entry points named for the native JNI callbacks.
            // The shipping Cargo command strips native symbols separately.
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    // cargo-ndk writes libvera20k_android.so here; the directory is generated
    // and git-ignored.
    sourceSets.getByName("main").jniLibs.srcDir("src/main/jniLibs")

    packaging {
        // Keep the .so uncompressed and mapped from the APK
        // (extractNativeLibs="false" in the manifest).
        jniLibs.useLegacyPackaging = false
    }
}
