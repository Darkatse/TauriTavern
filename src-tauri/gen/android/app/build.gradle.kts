import groovy.json.JsonSlurper
import java.io.File
import java.util.Properties
import java.io.FileInputStream

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

val rustlsPlatformVerifierAndroidPackage = run {
    val cargoProjectDir = File(rootProject.projectDir, "../..")
    val cargoMetadata = providers.exec {
        workingDir = cargoProjectDir
        commandLine(
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--filter-platform",
            "aarch64-linux-android",
            "--manifest-path",
            File(cargoProjectDir, "Cargo.toml").path,
        )
    }.standardOutput.asText.get()

    val dependencyJson = JsonSlurper().parseText(cargoMetadata) as Map<*, *>
    val packages = dependencyJson["packages"] as List<*>
    packages
        .map { it as Map<*, *> }
        .first { it["name"] == "rustls-platform-verifier-android" }
}

val rustlsPlatformVerifierAndroidVersion =
    rustlsPlatformVerifierAndroidPackage["version"] as String

val rustlsPlatformVerifierMavenDir = run {
    val manifestPath = rustlsPlatformVerifierAndroidPackage["manifest_path"] as String
    val manifestFile = file(manifestPath)
    File(manifestFile.parentFile, "maven")
}

repositories {
    maven {
        url = uri(rustlsPlatformVerifierMavenDir)
        metadataSources { mavenPom(); artifact() }
    }
}

android {
    compileSdk = 36
    namespace = "com.tauritavern.client"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "com.tauritavern.client"
        minSdk = 24
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    signingConfigs {
        create("release") {
            val keystorePropertiesFile = rootProject.file("keystore.properties")
            val keystoreProperties = Properties()
            if (keystorePropertiesFile.exists()) {
                keystoreProperties.load(FileInputStream(keystorePropertiesFile))
            }

            keyAlias = keystoreProperties["keyAlias"] as String
            keyPassword = keystoreProperties["password"] as String
            storeFile = file(keystoreProperties["storeFile"] as String)
            storePassword = keystoreProperties["password"] as String
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            isMinifyEnabled = true
            isShrinkResources = true
            signingConfig = signingConfigs.getByName("release")
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.core:core:1.18.0-rc01")
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("rustls:rustls-platform-verifier:$rustlsPlatformVerifierAndroidVersion")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")
