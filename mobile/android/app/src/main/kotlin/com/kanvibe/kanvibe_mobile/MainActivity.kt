package com.kanvibe.kanvibe_mobile

import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine

class MainActivity : FlutterActivity() {
    private var attachmentBridge: PaneChatAttachmentBridge? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        attachmentBridge = PaneChatAttachmentBridge(this, flutterEngine.dartExecutor.binaryMessenger)
    }

    override fun cleanUpFlutterEngine(flutterEngine: FlutterEngine) {
        attachmentBridge?.dispose()
        attachmentBridge = null
        super.cleanUpFlutterEngine(flutterEngine)
    }
}
