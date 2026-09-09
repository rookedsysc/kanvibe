import java.util.Properties

plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

/**
 * 업로드 키는 저장소에 둘 수 없어서 `android/key.properties`로 받는다(`.gitignore` 대상).
 * 그 안의 `storeFile` 경로는 이 파일이 있는 `android/`를 기준으로 읽는다.
 */
val keystoreProperties = Properties().apply {
    val file = rootProject.file("key.properties")
    if (file.exists()) file.inputStream().use { load(it) }
}

/** 반쯤 채워진 key.properties는 그 자리에서 세운다. 조용히 debug로 떨어지면 Play가 거절할 때까지 모른다. */
fun keystoreProperty(name: String): String =
    keystoreProperties.getProperty(name)
        ?: throw GradleException("android/key.properties에 $name 이(가) 없습니다.")

android {
    namespace = "com.kanvibe.kanvibe_mobile"
    /**
     * flutter_secure_storage 11이 API 37 이상을 요구해서 Flutter 기본값(36)보다 한 단계 올린다.
     * 토큰을 안드로이드 키스토어에 두려면 이 의존성이 필요하고, 낮추면 그 저장소를 못 쓴다.
     */
    compileSdk = 37
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        // TODO: Specify your own unique Application ID (https://developer.android.com/studio/build/application-id.html).
        applicationId = "com.kanvibe.kanvibe_mobile"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        // Uses the version code from pubspec.yaml. When using split APKs, 1000 * ABI_VERSION
        // is added automatically by Flutter. (https://developer.android.com/studio/build/configure-apk-splits#configure-APK-versions)
        // You can force using the value of versionCode by specifying the `-P force-version-code-ignoring-abi=true`
        // flag during build.
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    signingConfigs {
        if (!keystoreProperties.isEmpty) {
            create("release") {
                storeFile = rootProject.file(keystoreProperty("storeFile"))
                storePassword = keystoreProperty("storePassword")
                keyAlias = keystoreProperty("keyAlias")
                keyPassword = keystoreProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            /** keystore를 받지 않은 기기에서도 `flutter run --release`는 돌아가야 한다. */
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}
