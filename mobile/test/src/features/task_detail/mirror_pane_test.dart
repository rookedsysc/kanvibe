import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/mirror_pane.dart';

void main() {
  group('pane 응답 해석', () {
    test('좌표를 그대로 읽는다', () {
      final pane = MirrorPane.fromJson({
        'id': '%20',
        'tabId': '@10',
        'tabName': 'kanvibe',
        'command': 'claude',
        'left': 51,
        'top': 0,
        'width': 49,
        'height': 30,
      });

      expect(pane?.left, 51);
      expect(pane?.width, 49);
    });

    test('식별자가 없는 항목은 버린다', () {
      expect(MirrorPane.fromJson({'tabId': '@10'}), isNull);
    });
  });

  group('폰이 쓰는 pane 목록', () {
    test('모든 window의 pane을 순서대로 펼친다', () {
      final surfaces = TaskSurfaces.fromJson({
        'tabs': [
          {
            'id': '@10',
            'name': 'first',
            'panes': [
              {'id': '%19', 'width': 50, 'height': 30},
              {'id': '%20', 'width': 49, 'height': 30},
            ],
          },
          {
            'id': '@11',
            'name': 'second',
            'panes': [
              {'id': '%21', 'width': 100, 'height': 30},
            ],
          },
        ],
      });

      expect(surfaces.allPanes.map((pane) => pane.id), ['%19', '%20', '%21']);
    });

    test('탭이 하나도 없으면 pane도 없다', () {
      expect(TaskSurfaces.fromJson(const {}).allPanes, isEmpty);
    });
  });

  group('세션을 비출 수 없는 사유', () {
    test('데스크탑이 보낸 사유를 그대로 알아본다', () {
      expect(
        SurfaceUnavailableReason.fromWire('session-not-running'),
        SurfaceUnavailableReason.sessionNotRunning,
      );
      expect(
        SurfaceUnavailableReason.fromWire('zellij-too-old'),
        SurfaceUnavailableReason.zellijTooOld,
      );
    });

    test('모르는 사유도 안내 문구를 가진다', () {
      expect(SurfaceUnavailableReason.fromWire('처음 보는 값').message, isNotEmpty);
    });
  });
}
