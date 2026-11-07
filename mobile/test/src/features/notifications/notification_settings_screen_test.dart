import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/notifications/presentation/notification_settings_screen.dart';

import '../../../support/fake_desktop.dart';

Future<void> openSettings(
  WidgetTester tester,
  FakeDesktopClient client, {
  Size size = phoneSize,
  double textScale = 1,
}) async {
  await tester.binding.setSurfaceSize(size);
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(
    wrapWithApp(
      MediaQuery(
        data: MediaQueryData(
          size: size,
          textScaler: TextScaler.linear(textScale),
        ),
        child: const NotificationSettingsScreen(),
      ),
      client: client,
      size: size,
    ),
  );
  await tester.pumpAndSettle();
}

SwitchListTile statusSwitch(WidgetTester tester, String label) =>
    tester.widget<SwitchListTile>(
      find.ancestor(
        of: find.text(label),
        matching: find.byType(SwitchListTile),
      ),
    );

void main() {
  testWidgets('PC 공유 범위와 기존 상태 선택을 표시하고 변경을 저장한다', (tester) async {
    final client = FakeDesktopClient();
    await openSettings(tester, client);
    expect(find.text('이 PC의 데스크탑과 모바일에 함께 적용됩니다.'), findsOneWidget);
    expect(statusSwitch(tester, 'Review').value, isTrue);
    expect(statusSwitch(tester, 'Todo').value, isFalse);
    await tester.tap(find.text('Todo'));
    await tester.pumpAndSettle();
    expect(
      client.savedNotificationSettings.single.enabledStatuses,
      contains('todo'),
    );
    expect(statusSwitch(tester, 'Todo').value, isTrue);
  });

  testWidgets('전체 수신을 꺼도 선택을 기억하고 다시 켜면 복원한다', (tester) async {
    final client = FakeDesktopClient();
    await openSettings(tester, client);
    await tester.tap(find.text('상태 변경 알림'));
    await tester.pumpAndSettle();
    expect(client.notificationSettings.isEnabled, isFalse);
    expect(statusSwitch(tester, 'Review').onChanged, isNull);
    expect(statusSwitch(tester, 'Review').value, isTrue);
    await tester.tap(find.text('상태 변경 알림'));
    await tester.pumpAndSettle();
    expect(client.notificationSettings.isEnabled, isTrue);
    expect(statusSwitch(tester, 'Review').onChanged, isNotNull);
  });

  testWidgets('저장 실패 시 이전 선택과 재시도 가능한 화면을 유지한다', (tester) async {
    final client = FakeDesktopClient();
    await openSettings(tester, client);
    client.notificationSettingsError = StateError('offline');
    await tester.tap(find.text('Todo'));
    await tester.pumpAndSettle();
    expect(statusSwitch(tester, 'Todo').value, isFalse);
    expect(find.text('설정을 저장하지 못했습니다. 다시 시도해 주세요.'), findsOneWidget);
    client.notificationSettingsError = null;
    await tester.tap(find.text('Todo'));
    await tester.pumpAndSettle();
    expect(statusSwitch(tester, 'Todo').value, isTrue);
  });

  testWidgets('설정 조회 실패 후 재시도한다', (tester) async {
    final client = FakeDesktopClient()
      ..notificationSettingsError = StateError('offline');
    await openSettings(tester, client);
    expect(find.text('알림 설정을 불러오지 못했습니다.'), findsOneWidget);
    client.notificationSettingsError = null;
    await tester.tap(find.text('다시 시도'));
    await tester.pumpAndSettle();
    expect(find.text('알림을 받을 상태'), findsOneWidget);
  });

  for (final size in [phoneSize, tabletSize, const Size(320, 640)]) {
    testWidgets('설정 화면 너비 ${size.width}에서 상태 목록을 스크롤한다', (tester) async {
      await openSettings(tester, FakeDesktopClient(), size: size);
      await tester.ensureVisible(find.text('Done'));
      await tester.pumpAndSettle();
      expect(find.text('작업 완료'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }
  testWidgets('큰 글자에서도 스위치와 설명을 읽고 변경할 수 있다', (tester) async {
    final client = FakeDesktopClient();
    await openSettings(
      tester,
      client,
      size: const Size(320, 640),
      textScale: 2,
    );
    await tester.scrollUntilVisible(find.text('Review'), 200);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Review'));
    await tester.pumpAndSettle();
    expect(
      client.notificationSettings.enabledStatuses,
      isNot(contains('review')),
    );
    expect(tester.takeException(), isNull);
  });
}
