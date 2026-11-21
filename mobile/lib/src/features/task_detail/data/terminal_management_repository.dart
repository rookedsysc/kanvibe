import 'dart:convert';

import 'package:flutter/services.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';

class ClipboardImage {
  const ClipboardImage({required this.mimeType, required this.bytes});
  final String mimeType;
  final Uint8List bytes;
}

class ClipboardImageReader {
  const ClipboardImageReader();
  static const channel = MethodChannel('kanvibe/clipboard_image');
  Future<ClipboardImage?> read() async {
    final value = await channel.invokeMapMethod<String, dynamic>('readImage');
    if (value == null) return null;
    final mimeType = value['mimeType'];
    final bytes = value['bytes'];
    if (mimeType is! String ||
        !const ['image/png', 'image/jpeg'].contains(mimeType) ||
        bytes is! Uint8List ||
        bytes.isEmpty ||
        bytes.length > 2 * 1024 * 1024) {
      throw const FormatException('클립보드 이미지 형식이나 크기가 올바르지 않습니다.');
    }
    return ClipboardImage(mimeType: mimeType, bytes: bytes);
  }
}

class TerminalManagementRepository {
  const TerminalManagementRepository(this.client);
  final DesktopClient client;
  Future<void> rename(
    DesktopConnection connection,
    String taskId,
    String tabId,
    String name,
  ) async {
    await client.requestJson(
      connection,
      'PATCH',
      ['tasks', taskId, 'tabs', tabId],
      body: {'name': name},
    );
  }

  Future<void> move(
    DesktopConnection connection,
    String taskId,
    String tabId,
    int index,
  ) async {
    await client.requestJson(
      connection,
      'POST',
      ['tasks', taskId, 'tabs', tabId, 'move'],
      body: {'index': index},
    );
  }

  Future<void> pasteImage(
    DesktopConnection connection,
    String taskId,
    String paneId,
    ClipboardImage image,
  ) async {
    await client.requestJson(
      connection,
      'POST',
      ['tasks', taskId, 'panes', paneId, 'image'],
      body: {'mimeType': image.mimeType, 'base64': base64Encode(image.bytes)},
      timeout: const Duration(seconds: 60),
    );
  }
}
