import '../domain/mirror_pane.dart';

/// 세션을 비출 수 없는 사유를 화면 문구로 옮긴다.
///
/// 사유 자체는 데스크탑이 보내는 값이라 domain의 것이지만 문구는 아니다.
/// 문구가 domain에 있으면 나중에 번역을 넣을 때 도메인 계층부터 다시 갈라야 한다.
String surfaceUnavailableMessage(SurfaceUnavailableReason reason) =>
    switch (reason) {
      SurfaceUnavailableReason.noSession => '이 태스크에는 터미널 세션이 없습니다.',
      SurfaceUnavailableReason.sessionNotRunning =>
        '세션이 실행 중이 아닙니다. 데스크탑에서 태스크를 열면 세션이 시작됩니다.',
      SurfaceUnavailableReason.zellijTooOld => 'zellij 0.44 이상이 필요합니다.',
      SurfaceUnavailableReason.unknown => '터미널을 불러오지 못했습니다.',
    };
