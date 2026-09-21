import 'package:flutter/widgets.dart';

/// 간격과 크기.
///
/// 데스크탑 도구를 옮긴 화면이라 밀도를 높게 잡되, 4/8 배수 리듬은 지킨다.
/// 손가락이 닿는 것은 밀도와 무관하게 최소 크기를 지켜야 하므로 [minTouchTarget]만 별도로 둔다.
abstract final class AppSizes {
  static const p4 = 4.0;
  static const p8 = 8.0;
  static const p12 = 12.0;
  static const p16 = 16.0;
  static const p24 = 24.0;
  static const p32 = 32.0;

  /// iOS 44pt / Android 48dp 중 큰 쪽을 따른다
  static const minTouchTarget = 48.0;

  static const radiusSm = 3.0;
  static const radiusMd = 6.0;
  static const radiusLg = 8.0;

  /// 이 너비부터 칸반과 window 전체 보기로 바뀐다.
  /// 세로로 든 태블릿도 칸반이어야 해서 흔한 태블릿 세로 너비(768) 아래에 두지 않는다.
  static const tabletBreakpoint = 768.0;
}

/// 지금 화면이 태블릿 배치인지. 폰과 태블릿의 갈림길은 이 함수 하나뿐이다
bool isTabletLayout(BoxConstraints constraints) =>
    constraints.maxWidth >= AppSizes.tabletBreakpoint;
