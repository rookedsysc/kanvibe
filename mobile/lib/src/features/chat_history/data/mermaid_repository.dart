import 'dart:convert';
import 'dart:typed_data';

import 'package:http/http.dart' as http;

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../domain/mermaid_image.dart';

class MermaidRepository {
  const MermaidRepository(this.client);
  final DesktopClient client;

  static const maxDefinitionBytes = 65536;
  static const maxImageBytes = 4 * 1024 * 1024;
  static const renderTimeout = Duration(seconds: 70);
  static const _maxEncodedLength = ((maxImageBytes + 2) ~/ 3) * 4;
  static const _closedBeforeHeaders =
      'Connection closed before full header was received';
  static const _invalidImage = 'PC에서 받은 다이어그램 이미지가 올바르지 않습니다.';

  Future<MermaidImage> render(
    DesktopConnection connection,
    MermaidRenderTarget target,
  ) {
    if (target.definition.trim().isEmpty ||
        utf8.encode(target.definition).length > maxDefinitionBytes ||
        target.width < 320 ||
        target.width > 1200) {
      throw ArgumentError('다이어그램 원문이나 크기를 확인해 주세요.');
    }
    return _renderWithRecovery(connection, target).timeout(renderTimeout);
  }

  Future<MermaidImage> _renderWithRecovery(
    DesktopConnection connection,
    MermaidRenderTarget target,
  ) async {
    Map<String, dynamic> response;
    try {
      response = await _request(connection, target);
    } on http.ClientException catch (error) {
      if (error.message != _closedBeforeHeaders) rethrow;
      // 이 POST는 서버의 hash 기반 읽기 전용 렌더링이다. 변경 API에는 적용하지 않는다.
      response = await _request(connection, target);
    }
    return _parse(response, target.width);
  }

  Future<Map<String, dynamic>> _request(
    DesktopConnection connection,
    MermaidRenderTarget target,
  ) => client.requestJson(
    connection,
    'POST',
    ['diagrams', 'render'],
    body: {
      'definition': target.definition,
      'theme': target.theme.name,
      'width': target.width,
    },
    timeout: renderTimeout,
  );

  MermaidImage _parse(Map<String, dynamic> response, int requestedWidth) {
    final encoded = response['imageBase64'];
    final width = response['width'];
    final height = response['height'];
    final mime = response.containsKey('mimeType')
        ? response['mimeType']
        : 'image/png';
    if ((mime != 'image/png' && mime != 'image/jpeg') ||
        encoded is! String ||
        encoded.isEmpty ||
        encoded.length > _maxEncodedLength ||
        width is! int ||
        height is! int ||
        width != requestedWidth ||
        width < 320 ||
        width > 1200 ||
        height < 1 ||
        height > 2400 ||
        width * height > 2880000) {
      throw const FormatException(_invalidImage);
    }
    final Uint8List bytes;
    try {
      bytes = base64Decode(encoded);
    } on FormatException {
      throw const FormatException(_invalidImage);
    }
    if (bytes.length < 4 || bytes.length > maxImageBytes) {
      throw const FormatException(_invalidImage);
    }
    if (mime == 'image/jpeg') {
      _validateJpeg(bytes, width, height);
    } else {
      _validatePng(bytes, width, height);
    }
    return MermaidImage(bytes: bytes, width: width, height: height);
  }

  void _validatePng(Uint8List bytes, int width, int height) {
    if (bytes.length < 45) throw const FormatException(_invalidImage);
    const signature = [137, 80, 78, 71, 13, 10, 26, 10];
    const end = [0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130];
    for (var i = 0; i < signature.length; i++) {
      if (bytes[i] != signature[i]) throw const FormatException(_invalidImage);
    }
    for (var i = 0; i < end.length; i++) {
      if (bytes[bytes.length - end.length + i] != end[i]) {
        throw const FormatException(_invalidImage);
      }
    }
    final header = ByteData.sublistView(bytes);
    if (header.getUint32(8) != 13 ||
        ascii.decode(bytes.sublist(12, 16), allowInvalid: true) != 'IHDR' ||
        header.getUint32(16) != width ||
        header.getUint32(20) != height) {
      throw const FormatException(_invalidImage);
    }
  }

  void _validateJpeg(Uint8List bytes, int width, int height) {
    if (bytes[0] != 0xff ||
        bytes[1] != 0xd8 ||
        bytes[bytes.length - 2] != 0xff ||
        bytes.last != 0xd9) {
      throw const FormatException(_invalidImage);
    }
    final data = ByteData.sublistView(bytes);
    var offset = 2;
    var foundFrame = false;
    while (offset < bytes.length - 2) {
      if (bytes[offset++] != 0xff) throw const FormatException(_invalidImage);
      while (offset < bytes.length - 2 && bytes[offset] == 0xff) {
        offset++;
      }
      if (offset >= bytes.length - 2) {
        throw const FormatException(_invalidImage);
      }
      final marker = bytes[offset++];
      if (marker == 0 ||
          marker == 1 ||
          marker >= 0xd0 && marker <= 0xd9 ||
          offset + 2 > bytes.length - 2) {
        throw const FormatException(_invalidImage);
      }
      final length = data.getUint16(offset);
      if (length < 2 || offset + length > bytes.length - 2) {
        throw const FormatException(_invalidImage);
      }
      final isFrame =
          marker >= 0xc0 &&
          marker <= 0xcf &&
          marker != 0xc4 &&
          marker != 0xc8 &&
          marker != 0xcc;
      if (isFrame) {
        if (foundFrame ||
            !const {0xc0, 0xc1, 0xc2}.contains(marker) ||
            length < 8) {
          throw const FormatException(_invalidImage);
        }
        final components = bytes[offset + 7];
        if (bytes[offset + 2] != 8 ||
            components != 1 && components != 3 ||
            length != 8 + components * 3 ||
            data.getUint16(offset + 3) != height ||
            data.getUint16(offset + 5) != width) {
          throw const FormatException(_invalidImage);
        }
        foundFrame = true;
      }
      if (marker == 0xda) {
        if (!foundFrame || length < 6 || offset + length >= bytes.length - 2) {
          throw const FormatException(_invalidImage);
        }
        final components = bytes[offset + 2];
        if (components < 1 || components > 3 || length != 6 + components * 2) {
          throw const FormatException(_invalidImage);
        }
        return;
      }
      offset += length;
    }
    throw const FormatException(_invalidImage);
  }
}
