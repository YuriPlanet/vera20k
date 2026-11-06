// Gradle root for the Android client. The Rust side is built separately by
// cargo-ndk (see README.md) and dropped into app/src/main/jniLibs; Gradle only
// packages it.
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
