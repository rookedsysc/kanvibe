package com.kanvibe.kanvibe_mobile

import android.content.ClipboardManager
import android.graphics.BitmapFactory
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors

class MainActivity : FlutterActivity() {
    private var attachmentBridge: PaneChatAttachmentBridge? = null
    private val clipboardReader = Executors.newSingleThreadExecutor()

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        attachmentBridge = PaneChatAttachmentBridge(this, flutterEngine.dartExecutor.binaryMessenger)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "kanvibe/clipboard_image")
            .setMethodCallHandler { call, result ->
                if (call.method != "readImage") {
                    result.notImplemented()
                    return@setMethodCallHandler
                }
                val clipboard = getSystemService(CLIPBOARD_SERVICE) as ClipboardManager
                val clip = clipboard.primaryClip
                val uri = if (clip != null && clip.itemCount > 0) clip.getItemAt(0).uri else null
                if (uri == null || uri.scheme != "content") {
                    result.success(null)
                    return@setMethodCallHandler
                }
                clipboardReader.execute {
                    try {
                        val mimeType = contentResolver.getType(uri)
                        require(mimeType == "image/png" || mimeType == "image/jpeg")
                        val bytes = contentResolver.openInputStream(uri)?.use { stream ->
                            val output = ByteArrayOutputStream()
                            val buffer = ByteArray(8192)
                            while (true) {
                                val length = stream.read(buffer)
                                if (length < 0) break
                                require(output.size() + length <= MAX_IMAGE_BYTES)
                                output.write(buffer, 0, length)
                            }
                            output.toByteArray()
                        } ?: throw IllegalArgumentException("image-unavailable")
                        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
                        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
                        require(bounds.outWidth > 0 && bounds.outHeight > 0)
                        require(bounds.outWidth.toLong() * bounds.outHeight <= MAX_IMAGE_PIXELS)
                        runOnUiThread { result.success(mapOf("mimeType" to mimeType, "bytes" to bytes)) }
                    } catch (_: Exception) {
                        runOnUiThread { result.error("clipboard-image-unavailable", "클립보드의 PNG/JPEG 이미지(최대 2 MiB)를 읽을 수 없습니다.", null) }
                    }
                }
            }
    }

    override fun cleanUpFlutterEngine(flutterEngine: FlutterEngine) {
        attachmentBridge?.dispose()
        attachmentBridge = null
        super.cleanUpFlutterEngine(flutterEngine)
    }

    override fun onDestroy() {
        clipboardReader.shutdownNow()
        super.onDestroy()
    }

    companion object {
        private const val MAX_IMAGE_BYTES = 2 * 1024 * 1024
        private const val MAX_IMAGE_PIXELS = 16_000_000L
    }
}
