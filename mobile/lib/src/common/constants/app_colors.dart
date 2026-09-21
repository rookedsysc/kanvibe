import 'package:flutter/widgets.dart';

/// KanVibe 데스크탑의 색 토큰을 그대로 옮긴 값.
///
/// 출처는 `src/styles/globals.css`의 다크 블록이다. 데스크탑과 모바일이 같은 제품으로 읽히려면
/// 여기서 색을 새로 만들지 않고 그쪽 값을 따라야 한다. 데스크탑 토큰이 바뀌면 이 파일도 같이 바꾼다.
///
/// 데스크탑은 다크가 기본 테마라 모바일도 다크만 그린다.
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
