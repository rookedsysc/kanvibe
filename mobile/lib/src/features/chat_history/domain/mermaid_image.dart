import 'dart:typed_data';

enum MermaidTheme { light, dark }

typedef MermaidRenderTarget = ({
  String definition,
  MermaidTheme theme,
  int width,
});

class MermaidImage {
  MermaidImage({
    required Uint8List bytes,
    required this.width,
    required this.height,
  }) : bytes = bytes.asUnmodifiableView();

  final Uint8List bytes;
  final int width;
  final int height;
}
