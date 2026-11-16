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
    this.naturalWidth,
  }) : bytes = bytes.asUnmodifiableView();

  final Uint8List bytes;
  final int width;
  final int height;

  /// PC가 칸 너비에 맞춰 줄이기 전 다이어그램의 원래 너비
  final int? naturalWidth;
}
