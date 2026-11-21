import 'package:flutter/material.dart';

import 'app_colors.dart';
import 'app_sizes.dart';

/// 화면 전체의 기본값.
///
/// 서체는 두 갈래다. 식별자(브랜치, 세션, pane 명령)와 터미널은 고정폭이라야 글자 폭이 흔들리지 않고,
/// 제목과 설명은 시스템 서체가 읽기 좋다. 전부 고정폭으로 쓰면 목록이 코드 덤프처럼 보인다.
const monoFontFamily = 'JetBrainsMono';

ThemeData buildAppTheme([Brightness brightness = Brightness.dark]) {
  final palette = brightness == Brightness.dark
      ? AppPalette.dark
      : AppPalette.light;
  final colorScheme = ColorScheme.fromSeed(
    seedColor: AppColors.brandPrimary,
    brightness: brightness,
    primary: AppColors.brandPrimary,
    onPrimary: Colors.white,
    surface: palette.bgSurface,
    onSurface: palette.textPrimary,
    error: brightness == Brightness.dark
        ? AppColors.statusError
        : const Color(0xFFB3261E),
    outline: palette.borderDefault,
  );

  return ThemeData(
    useMaterial3: true,
    brightness: brightness,
    extensions: [palette],
    colorScheme: colorScheme,
    scaffoldBackgroundColor: palette.bgPage,
    dividerColor: palette.borderDefault,
    appBarTheme: AppBarTheme(
      backgroundColor: palette.bgPage,
      surfaceTintColor: Colors.transparent,
      foregroundColor: palette.textPrimary,
      elevation: 0,
      centerTitle: false,
    ),
    textTheme: TextTheme(
      titleMedium: const TextStyle(
        fontSize: 16,
        height: 1.4,
        fontWeight: FontWeight.w600,
      ),
      bodyMedium: TextStyle(
        fontSize: 14,
        height: 1.5,
        color: palette.textPrimary,
      ),
      bodySmall: TextStyle(
        fontSize: 12,
        height: 1.5,
        color: palette.textSecondary,
      ),
      labelSmall: TextStyle(
        fontSize: 11,
        height: 1.4,
        color: palette.textMuted,
      ),
    ),
    inputDecorationTheme: InputDecorationTheme(
      filled: true,
      fillColor: palette.bgSurface,
      border: OutlineInputBorder(
        borderRadius: BorderRadius.circular(AppSizes.radiusMd),
        borderSide: BorderSide(color: palette.borderDefault),
      ),
      enabledBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(AppSizes.radiusMd),
        borderSide: BorderSide(color: palette.borderDefault),
      ),
      focusedBorder: OutlineInputBorder(
        borderRadius: BorderRadius.circular(AppSizes.radiusMd),
        borderSide: const BorderSide(color: AppColors.brandPrimary, width: 2),
      ),
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(
        minimumSize: const Size.fromHeight(AppSizes.minTouchTarget),
        backgroundColor: AppColors.brandPrimary,
        foregroundColor: Colors.white,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(AppSizes.radiusMd),
        ),
      ),
    ),
  );
}

/// 식별자와 터미널에 쓰는 고정폭 스타일
TextStyle monoStyle({
  double fontSize = 12,
  Color? color,
  FontWeight? fontWeight,
}) => TextStyle(
  fontFamily: monoFontFamily,
  fontSize: fontSize,
  height: 1.4,
  color: color ?? AppColors.textSecondary,
  fontWeight: fontWeight,
);
