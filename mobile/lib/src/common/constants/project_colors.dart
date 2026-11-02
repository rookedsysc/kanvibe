import 'dart:math' as math;

import 'package:flutter/material.dart';

/// 데스크탑 projectColor.ts와 같은 UTF-16 해시와 팔레트를 사용한다.
Color projectColor(String name, String? configured) {
  if (configured != null &&
      RegExp(r'^#[0-9a-fA-F]{6}$').hasMatch(configured.trim())) {
    return Color(
      0xFF000000 | int.parse(configured.trim().substring(1), radix: 16),
    );
  }
  const palette = [
    0xFFF9A8D4,
    0xFF93C5FD,
    0xFF86EFAC,
    0xFFC4B5FD,
    0xFFFDBA74,
    0xFFFDE047,
    0xFF5EEAD4,
    0xFFA5B4FC,
  ];
  var hash = 0;
  for (final unit in name.codeUnits) {
    hash = (hash * 31 + unit).toSigned(32);
  }
  return Color(palette[hash % palette.length]);
}

/// 배경과 대비가 높은 데스크탑 글자색을 선택한다.
Color projectTextColor(Color background) {
  double linear(double channel) => channel <= 0.03928
      ? channel / 12.92
      : math.pow((channel + 0.055) / 1.055, 2.4).toDouble();
  final luminance =
      linear(background.r) * 0.2126 +
      linear(background.g) * 0.7152 +
      linear(background.b) * 0.0722;
  final darkContrast =
      (math.max(luminance, 0.0092) + 0.05) /
      (math.min(luminance, 0.0092) + 0.05);
  return darkContrast >= 1.05 / (luminance + 0.05)
      ? const Color(0xFF111827)
      : Colors.white;
}
