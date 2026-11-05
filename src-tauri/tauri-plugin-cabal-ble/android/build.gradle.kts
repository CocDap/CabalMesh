plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.cabalmesh.ble"
    compileSdk = 36

    defaultConfig {
        // L2CAP connection-oriented channels are API 29.
        //
        // `BluetoothAdapter.listenUsingInsecureL2capChannel` and
        // `BluetoothDevice.createInsecureL2capChannel` do not exist before it,
        // and they are what makes the stream in `cabal-ble::framing` a stream.
        // Without them every packet would need the fragmentation layer that
        // design deliberately does not have.
        minSdk = 29
        consumerProguardFiles("consumer-rules.pro")
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("com.fasterxml.jackson.core:jackson-databind:2.15.3")
    implementation(project(":tauri-android"))
}
