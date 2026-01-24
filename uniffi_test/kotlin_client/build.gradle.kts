plugins {
    kotlin("jvm") version "2.0.21"
    application
}

group = "com.example"
version = "0.1.0"

repositories {
    mavenCentral()
}

dependencies {
    // JNA for UniFFI native library loading
    implementation("net.java.dev.jna:jna:5.14.0")

    // Kotlin coroutines (optional, for async WebSocket handling)
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.8.1")

    // OkHttp for WebSocket client
    implementation("com.squareup.okhttp3:okhttp:4.12.0")

    // Java-WebSocket for server
    implementation("org.java-websocket:Java-WebSocket:1.5.7")

    // Logging
    implementation("org.slf4j:slf4j-simple:2.0.13")
}

application {
    mainClass.set("MainKt")
}

kotlin {
    jvmToolchain(21)
}

tasks.named<JavaExec>("run") {
    // Set library path to find the native .so file
    // The Rust target directory is at the workspace root (two levels up from kotlin_client)
    val libPath = file("${projectDir}/../../target/debug").absolutePath
    jvmArgs("-Djna.library.path=$libPath")

    // Also set LD_LIBRARY_PATH for the native library
    environment("LD_LIBRARY_PATH", libPath)

    // Connect stdin for interactive input
    standardInput = System.`in`
}
