// Plugins are declared here and applied in :app so the version lives in one
// place. AGP 8.11 is the first line that accepts compileSdk 36; bump it with
// the SDK, and keep Gradle (gradle/wrapper/gradle-wrapper.properties) at the
// minimum AGP requires — 8.13 for this version.
plugins {
    id("com.android.application") version "8.11.1" apply false
}
