import Flutter
import UIKit
import UserNotifications

@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate {
  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    UNUserNotificationCenter.current().delegate = self
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    GeneratedPluginRegistrant.register(with: engineBridge.pluginRegistry)
    if let registrar = engineBridge.pluginRegistry.registrar(forPlugin: "PaneChatAttachmentPlugin") {
      PaneChatAttachmentPlugin.register(with: registrar)
    }
    let clipboardChannel = FlutterMethodChannel(
      name: "kanvibe/clipboard_image",
      binaryMessenger: engineBridge.applicationRegistrar.messenger()
    )
    clipboardChannel.setMethodCallHandler { call, result in
      guard call.method == "readImage" else {
        result(FlutterMethodNotImplemented)
        return
      }
      guard let image = UIPasteboard.general.image else {
        result(nil)
        return
      }
      let width = image.size.width * image.scale
      let height = image.size.height * image.scale
      guard width > 0, height > 0, width * height <= 16_000_000,
            let bytes = image.pngData(), bytes.count <= 2 * 1024 * 1024 else {
        result(FlutterError(code: "clipboard-image-unavailable", message: "클립보드 이미지가 너무 크거나 읽을 수 없습니다.", details: nil))
        return
      }
      result(["mimeType": "image/png", "bytes": FlutterStandardTypedData(bytes: bytes)])
    }
  }
}
