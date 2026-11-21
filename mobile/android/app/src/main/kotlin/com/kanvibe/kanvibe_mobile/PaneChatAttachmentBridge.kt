package com.kanvibe.kanvibe_mobile

import android.app.Activity
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.OpenableColumns
import android.view.DragEvent
import android.view.View
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors

class PaneChatAttachmentBridge(
    private val activity: Activity,
    messenger: BinaryMessenger,
) : MethodChannel.MethodCallHandler {
    private val channel = MethodChannel(messenger, "kanvibe/pane-chat-attachments")
    private val executor = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())
    private var enabled = false
    private var epoch = 0
    private var disposed = false
    private val dragListener = View.OnDragListener { _, event -> onDrag(event) }

    init {
        channel.setMethodCallHandler(this)
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            "setDropEnabled" -> {
                enabled = call.argument<Boolean>("enabled") == true
                epoch = call.argument<Number>("epoch")?.toInt() ?: epoch + 1
                activity.window.decorView.setOnDragListener(if (enabled) dragListener else null)
                result.success(null)
            }
            "readClipboardImage" -> readForCall(call, result) { readClipboardImage() }
            "readContentUri" -> readForCall(call, result) {
                val value = call.argument<String>("uri") ?: throw IllegalArgumentException("파일 주소가 없습니다.")
                readUri(Uri.parse(value), call.argument("mimeType"))
            }
            else -> result.notImplemented()
        }
    }

    private fun readForCall(
        call: MethodCall,
        result: MethodChannel.Result,
        read: () -> Map<String, Any>?,
    ) {
        val requestEpoch = call.argument<Number>("epoch")?.toInt()
        if (!enabled || disposed || requestEpoch != epoch) {
            result.error("inactive", "첨부를 추가할 채팅이 활성화되어 있지 않습니다.", null)
            return
        }
        executor.execute {
            try {
                val file = read()
                main.post {
                    if (!disposed) {
                        if (enabled && epoch == requestEpoch) result.success(file)
                        else result.error("inactive", "첨부 대상이 변경되었습니다.", null)
                    }
                }
            } catch (error: Exception) {
                main.post {
                    if (!disposed) result.error("attachment_read_failed", errorMessage(error), null)
                }
            }
        }
    }

    private fun readClipboardImage(): Map<String, Any>? {
        val clipboard = activity.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
        val clip = clipboard.primaryClip ?: return null
        for (index in 0 until clip.itemCount) {
            val uri = clip.getItemAt(index).uri ?: continue
            val mimeType = activity.contentResolver.getType(uri)
            if (mimeType?.startsWith("image/") == true) return readUri(uri, mimeType)
        }
        return null
    }

    private fun onDrag(event: DragEvent): Boolean {
        if (!enabled || disposed) return false
        return when (event.action) {
            DragEvent.ACTION_DRAG_STARTED -> event.clipDescription?.let { description ->
                description.hasMimeType(ClipDescription.MIMETYPE_TEXT_URILIST) ||
                    description.hasMimeType("image/*") || description.hasMimeType("application/*")
            } ?: false
            DragEvent.ACTION_DROP -> {
                val clip = event.clipData ?: return false
                val requestEpoch = epoch
                val uris = (0 until clip.itemCount).mapNotNull { clip.getItemAt(it).uri }
                if (uris.isEmpty()) return false
                if (uris.size > MAX_FILES) {
                    reportDropFailure(requestEpoch, "첨부는 한 번에 10개까지 추가할 수 있습니다.")
                    return true
                }
                val permission = activity.requestDragAndDropPermissions(event)
                executor.execute {
                    try {
                        val files = uris.map { readUri(it, null) }
                        main.post {
                            if (!disposed && enabled && epoch == requestEpoch) {
                                channel.invokeMethod("filesDropped", mapOf("epoch" to requestEpoch, "files" to files))
                            }
                        }
                    } catch (error: Exception) {
                        main.post { reportDropFailure(requestEpoch, errorMessage(error)) }
                    } finally {
                        permission?.release()
                    }
                }
                true
            }
            else -> true
        }
    }

    private fun reportDropFailure(requestEpoch: Int, message: String) {
        if (!disposed && enabled && epoch == requestEpoch) {
            channel.invokeMethod("dropFailed", mapOf("epoch" to requestEpoch, "message" to message))
        }
    }

    private fun readUri(uri: Uri, suppliedMimeType: String?): Map<String, Any> {
        if (uri.scheme != "content") throw IllegalArgumentException("선택한 파일 주소를 읽을 수 없습니다.")
        val resolver = activity.contentResolver
        var name = uri.lastPathSegment?.substringAfterLast('/') ?: "attachment"
        resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst()) {
                val nameIndex = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                val sizeIndex = cursor.getColumnIndex(OpenableColumns.SIZE)
                if (nameIndex >= 0 && !cursor.isNull(nameIndex)) name = cursor.getString(nameIndex)
                if (sizeIndex >= 0 && !cursor.isNull(sizeIndex) && cursor.getLong(sizeIndex) > MAX_FILE_BYTES) {
                    throw IllegalArgumentException("파일은 10 MiB 이하여야 합니다.")
                }
            }
        }
        val bytes = resolver.openInputStream(uri)?.use { input ->
            val output = ByteArrayOutputStream(READ_BUFFER_BYTES)
            val buffer = ByteArray(READ_BUFFER_BYTES)
            while (true) {
                val count = input.read(buffer)
                if (count < 0) break
                if (output.size() + count > MAX_FILE_BYTES) {
                    throw IllegalArgumentException("파일은 10 MiB 이하여야 합니다.")
                }
                output.write(buffer, 0, count)
            }
            output.toByteArray()
        } ?: throw IllegalArgumentException("선택한 파일을 열지 못했습니다.")
        return mapOf(
            "name" to name,
            "mimeType" to (resolver.getType(uri) ?: suppliedMimeType ?: "application/octet-stream"),
            "bytes" to bytes,
        )
    }

    private fun errorMessage(error: Exception): String =
        if (error is IllegalArgumentException) error.message ?: "파일 정보가 올바르지 않습니다."
        else "선택한 파일을 읽지 못했습니다. 다시 시도해 주세요."

    fun dispose() {
        disposed = true
        enabled = false
        epoch++
        activity.window.decorView.setOnDragListener(null)
        channel.setMethodCallHandler(null)
        executor.shutdownNow()
    }

    companion object {
        private const val MAX_FILE_BYTES = 10 * 1024 * 1024
        private const val MAX_FILES = 10
        private const val READ_BUFFER_BYTES = 8 * 1024
    }
}
