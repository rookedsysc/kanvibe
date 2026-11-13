import 'package:flutter/material.dart';

/// KanVibe 데스크탑의 색 토큰을 그대로 옮긴 값.
///
/// 출처는 `src/styles/globals.css`의 다크 블록이다. 데스크탑과 모바일이 같은 제품으로 읽히려면
/// 여기서 색을 새로 만들지 않고 그쪽 값을 따라야 한다. 데스크탑 토큰이 바뀌면 이 파일도 같이 바꾼다.
///
/// 고정 색은 터미널과 상태 표현에 쓰고, 화면 표면과 글자는 [AppPalette]를 따른다.
abstract final class AppColors {
  /// 강조색. PR 버튼, 선택 상태, 링크처럼 눌러야 하는 것에만 쓴다
  static const brandPrimary = Color(0xFF0064FF);
  static const brandSubtle = Color(0x290064FF);

  /// 눌 수 있어 보이지만 경고는 아닌 표면
  static const buttonNeutral = Color(0xFF202632);

  static const bgPage = Color(0xFF090A0D);
  static const bgSurface = Color(0xFF111216);

  static const textPrimary = Color(0xFFF3F4F7);
  static const textSecondary = Color(0xFFB0B4BD);
  static const textMuted = Color(0xFF717680);

  static const borderDefault = Color(0xFF24262D);
  static const borderStrong = Color(0xFF353842);

  /// 상태색. 강조색과 섞지 않는다 — review만 강조색과 같은 값을 쓴다
  static const statusTodo = Color(0xFF747985);
  static const statusProgress = Color(0xFFF6C35F);
  static const statusPending = Color(0xFF9B8CFF);
  static const statusReview = brandPrimary;
  static const statusDone = Color(0xFF65D08A);

  static const statusError = Color(0xFFFF7D73);

  static const terminalChrome = Color(0xFF0F1014);
  static const terminalBg = Color(0xFF050608);
  static const terminalText = Color(0xFFA0A7B4);

  /// 에이전트 정체성 색. CLI의 브랜드색이라 상태색과 섞지 않는다
  static const agentClaude = Color(0xFFD97757);
  static const agentCodex = Color(0xFF5EC8D8);
  static const agentOpencode = Color(0xFFA78BFA);
  static const agentGemini = Color(0xFF4285F4);
}

@immutable
class AppPalette extends ThemeExtension<AppPalette> {
  const AppPalette({
    required this.bgPage,
    required this.bgSurface,
    required this.textPrimary,
    required this.textSecondary,
    required this.textMuted,
    required this.borderDefault,
    required this.borderStrong,
    required this.buttonNeutral,
  });

  final Color bgPage;
  final Color bgSurface;
  final Color textPrimary;
  final Color textSecondary;
  final Color textMuted;
  final Color borderDefault;
  final Color borderStrong;
  final Color buttonNeutral;

  static const dark = AppPalette(
    bgPage: AppColors.bgPage,
    bgSurface: AppColors.bgSurface,
    textPrimary: AppColors.textPrimary,
    textSecondary: AppColors.textSecondary,
    textMuted: Color(0xFF9298A3),
    borderDefault: AppColors.borderDefault,
    borderStrong: AppColors.borderStrong,
    buttonNeutral: AppColors.buttonNeutral,
  );

  static const light = AppPalette(
    bgPage: Color(0xFFF7F8FA),
    bgSurface: Color(0xFFFFFFFF),
    textPrimary: Color(0xFF17181C),
    textSecondary: Color(0xFF5F636D),
    textMuted: Color(0xFF626975),
    borderDefault: Color(0xFFE2E4E8),
    borderStrong: Color(0xFFC8CCD3),
    buttonNeutral: Color(0xFFEBEEF3),
  );

  @override
  AppPalette copyWith({
    Color? bgPage,
    Color? bgSurface,
    Color? textPrimary,
    Color? textSecondary,
    Color? textMuted,
    Color? borderDefault,
    Color? borderStrong,
    Color? buttonNeutral,
  }) => AppPalette(
    bgPage: bgPage ?? this.bgPage,
    bgSurface: bgSurface ?? this.bgSurface,
    textPrimary: textPrimary ?? this.textPrimary,
    textSecondary: textSecondary ?? this.textSecondary,
    textMuted: textMuted ?? this.textMuted,
    borderDefault: borderDefault ?? this.borderDefault,
    borderStrong: borderStrong ?? this.borderStrong,
    buttonNeutral: buttonNeutral ?? this.buttonNeutral,
  );

  @override
  AppPalette lerp(AppPalette? other, double t) {
    if (other == null) return this;
    return AppPalette(
      bgPage: Color.lerp(bgPage, other.bgPage, t)!,
      bgSurface: Color.lerp(bgSurface, other.bgSurface, t)!,
      textPrimary: Color.lerp(textPrimary, other.textPrimary, t)!,
      textSecondary: Color.lerp(textSecondary, other.textSecondary, t)!,
      textMuted: Color.lerp(textMuted, other.textMuted, t)!,
      borderDefault: Color.lerp(borderDefault, other.borderDefault, t)!,
      borderStrong: Color.lerp(borderStrong, other.borderStrong, t)!,
      buttonNeutral: Color.lerp(buttonNeutral, other.buttonNeutral, t)!,
    );
  }
}

extension AppColorContext on BuildContext {
  AppPalette get colors =>
      Theme.of(this).extension<AppPalette>() ??
      (Theme.of(this).brightness == Brightness.light
          ? AppPalette.light
          : AppPalette.dark);
}
