import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';

class TaskProjectIcon extends StatelessWidget {
  const TaskProjectIcon({required this.dataUrl, super.key});
  final String? dataUrl;
  static const _maxIconCharacters = 1024 * 1024;
  static const _iconSize = 20.0;

  @override
  Widget build(BuildContext context) {
    final source = dataUrl;
    Widget fallback() => const Icon(Icons.folder_outlined, size: _iconSize);
    if (source == null ||
        source.length > _maxIconCharacters ||
        !source.startsWith('data:image/')) {
      return fallback();
    }
    try {
      final data = UriData.parse(source);
      final bytes = data.contentAsBytes();
      if (data.mimeType == 'image/svg+xml') {
        return SvgPicture.memory(
          bytes,
          width: _iconSize,
          height: _iconSize,
          excludeFromSemantics: true,
          errorBuilder: (_, _, _) => fallback(),
        );
      }
      if (!const {
        'image/png',
        'image/jpeg',
        'image/gif',
        'image/webp',
      }.contains(data.mimeType)) {
        return fallback();
      }
      return Image.memory(
        bytes,
        width: _iconSize,
        height: _iconSize,
        excludeFromSemantics: true,
        errorBuilder: (_, _, _) => fallback(),
      );
    } on FormatException {
      return fallback();
    }
  }
}
