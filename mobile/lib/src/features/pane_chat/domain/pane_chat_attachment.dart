import 'dart:typed_data';

class PickedPaneChatAttachment {
  const PickedPaneChatAttachment({
    required this.name,
    required this.mimeType,
    required this.bytes,
  });

  final String name;
  final String mimeType;
  final Uint8List bytes;

  bool get isImage => mimeType.startsWith('image/');
}
