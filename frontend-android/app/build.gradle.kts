plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

// CI（.github/workflows/android-apk.yml）が -PversionName / -PversionCode で渡す
val appVersionName = (findProperty("versionName") as String?) ?: "0.1.0"
val appVersionCode = ((findProperty("versionCode") as String?) ?: "1").toInt()

android {
    namespace = "com.shiki01.nagi"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.shiki01.nagi"
        minSdk = 26
        targetSdk = 35
        versionCode = appVersionCode
        versionName = appVersionName
        // コア（libnagi_jni.so）は arm64 のみ同梱する
        ndk { abiFilters += listOf("arm64-v8a") }
    }

    // 署名鍵は環境変数で渡す（リポジトリには置かない）。未指定なら debug 鍵で署名する
    // （上書き更新できるのは同じ鍵で署名した APK だけ。配布用は Secrets の鍵を使うこと）
    val keystorePath = System.getenv("NAGI_KEYSTORE_PATH")
    signingConfigs {
        if (!keystorePath.isNullOrEmpty()) {
            create("release") {
                storeFile = file(keystorePath)
                storePassword = System.getenv("NAGI_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("NAGI_KEY_ALIAS")
                keyPassword = System.getenv("NAGI_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig =
                if (!keystorePath.isNullOrEmpty()) signingConfigs.getByName("release")
                else signingConfigs.getByName("debug")
        }
    }

    // キーボードのレイアウト（config/keyboard/presets/*.json）を assets の presets/ として同梱する。
    // src/main/assets（辞書・更新用公開鍵。CI が配置する）はそのまま使い、リポジトリ共通の config/keyboard を足す
    sourceSets {
        getByName("main") {
            assets.srcDir("../../config/keyboard")
        }
    }

    // 辞書は mmap で読むため、assets から filesDir に展開する。圧縮しない（展開を速くする）
    androidResources {
        noCompress += listOf("bin")
        // config/keyboard のうちプリセット以外（JSON Schema・テスト用のレイアウト）は APK に入れない。
        // 既定の除外（.svn / .git / .DS_Store など）も残す。`!` は「除外」、区切りは `:`
        ignoreAssetsPattern = "!.svn:!.git:!.ds_store:!*.scc:.*:<dir>_*:!CVS:!thumbs.db:!picasa.ini:!*~:" +
            "!test-layouts:!test-looks:!layout.schema.json:!README.md"
    }

    packaging {
        jniLibs {
            // 展開された .so を使う（コアのサイズが大きいため）
            useLegacyPackaging = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.15.0")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")
}
