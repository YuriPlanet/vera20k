// Gradle root for the Android client. The Rust side, Cargo.toml and src/ next
// to this file, is built separately by cargo-ndk (see docs/android.md) and
// dropped into app/src/main/jniLibs; Gradle only packages it.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "vera20k"
include(":app")
