import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/board/presentation/board_screen.dart';
import 'package:kanvibe_mobile/src/features/project_registry/data/project_scan_result.dart';
import 'package:kanvibe_mobile/src/features/project_registry/presentation/project_registry_screen.dart';

import '../../../support/fake_desktop.dart';

const connection = DesktopConnection(
  host: '127.0.0.1',
  port: 9736,
  token: 'test-token',
);

Future<void> openRegistry(WidgetTester tester, FakeDesktopClient client) async {
  await tester.binding.setSurfaceSize(phoneSize);
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(
    wrapWithApp(
      const ProjectRegistryScreen(connection: connection),
      client: client,
      size: phoneSize,
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('홈 메뉴에서 등록 화면을 열고 하위 폴더를 탐색한 뒤 등록한다', (tester) async {
    final client = FakeDesktopClient();
    await tester.binding.setSurfaceSize(phoneSize);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(
      wrapWithApp(const BoardScreen(), client: client, size: phoneSize),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('보드 메뉴'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('프로젝트 등록'));
    await tester.pumpAndSettle();
    expect(find.text('127.0.0.1의 폴더'), findsOneWidget);
    await tester.tap(find.text('Documents'));
    await tester.pumpAndSettle();
    expect(
      tester
          .widget<TextField>(find.byKey(const ValueKey('project-path')))
          .controller!
          .text,
      '~/Documents',
    );
    expect(find.textContaining('하위 폴더가 없습니다'), findsOneWidget);
    final boardCalls = client.boardCalls.length;
    await tester.tap(find.byKey(const ValueKey('project-scan')));
    await tester.pumpAndSettle();
    expect(client.scannedPaths, ['~/Documents']);
    expect(find.text('프로젝트 1개 등록'), findsOneWidget);
    expect(find.text('new-project'), findsOneWidget);
    await tester.pageBack();
    await tester.pumpAndSettle();
    expect(client.boardCalls.length, greaterThan(boardCalls));
  });

  testWidgets('상위 폴더와 직접 경로 입력이 선택 경로에 반영된다', (tester) async {
    final client = FakeDesktopClient();
    await openRegistry(tester, client);
    await tester.tap(find.text('Documents'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('상위 폴더'));
    await tester.pumpAndSettle();
    expect(
      tester.widget<TextField>(find.byType(TextField)).controller!.text,
      '~',
    );
    await tester.enterText(find.byType(TextField), '/workspace/');
    await tester.tap(find.text('경로 열기'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('project-scan')));
    await tester.pumpAndSettle();
    expect(client.scannedPaths, ['/workspace']);
  });

  testWidgets('잘못된 경로와 조회 실패는 안내하고 다시 시도할 수 있다', (tester) async {
    final client = FakeDesktopClient()
      ..directoryError = const DesktopRequestException(400);
    await openRegistry(tester, client);
    expect(find.textContaining('폴더를 열 수 없습니다'), findsOneWidget);
    expect(
      tester.widget<FilledButton>(find.byType(FilledButton)).onPressed,
      isNull,
    );
    client.directoryError = null;
    await tester.tap(find.text('다시 시도'));
    await tester.pumpAndSettle();
    expect(find.text('Documents'), findsOneWidget);
    await tester.enterText(find.byType(TextField), 'relative');
    await tester.tap(find.text('경로 열기'));
    await tester.pumpAndSettle();
    expect(find.textContaining('절대 경로 또는 ~/'), findsOneWidget);
  });

  testWidgets('구버전 데스크톱은 업데이트 안내를 보여 준다', (tester) async {
    await openRegistry(
      tester,
      FakeDesktopClient()..directoryError = const DesktopRequestException(404),
    );
    expect(find.textContaining('데스크톱 앱을 업데이트'), findsOneWidget);
  });

  testWidgets('같은 폴더의 끝 슬래시를 정규화해도 등록 버튼이 다시 활성화된다', (tester) async {
    final client = FakeDesktopClient();
    await openRegistry(tester, client);
    await tester.enterText(find.byType(TextField), '~/');
    await tester.pump();
    expect(
      tester.widget<FilledButton>(find.byType(FilledButton)).onPressed,
      isNull,
    );
    await tester.tap(find.text('경로 열기'));
    await tester.pumpAndSettle();
    expect(
      tester.widget<TextField>(find.byType(TextField)).controller!.text,
      '~',
    );
    expect(
      tester.widget<FilledButton>(find.byType(FilledButton)).onPressed,
      isNotNull,
    );
    await tester.tap(find.byKey(const ValueKey('project-scan')));
    await tester.pumpAndSettle();
    expect(client.scannedPaths, ['~']);
  });

  testWidgets('작은 휴대폰에서 키보드를 열어도 경로 입력과 탐색이 넘치지 않는다', (tester) async {
    const size = Size(320, 568);
    final client = FakeDesktopClient();
    await tester.binding.setSurfaceSize(size);
    addTearDown(() => tester.binding.setSurfaceSize(null));
    await tester.pumpWidget(
      wrapWithApp(
        const MediaQuery(
          data: MediaQueryData(
            size: size,
            viewInsets: EdgeInsets.only(bottom: 280),
          ),
          child: ProjectRegistryScreen(connection: connection),
        ),
        client: client,
        size: size,
      ),
    );
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
    await tester.ensureVisible(find.byType(TextField));
    await tester.enterText(find.byType(TextField), '/workspace');
    await tester.ensureVisible(find.text('경로 열기'));
    await tester.tap(find.text('경로 열기'));
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
    expect(
      tester.widget<TextField>(find.byType(TextField)).controller!.text,
      '/workspace',
    );
  });

  testWidgets('등록 중 연타와 경로 변경을 막고 결과에 중복 및 오류를 표시한다', (tester) async {
    final client = _PendingScanClient();
    await openRegistry(tester, client);
    await tester.tap(find.byKey(const ValueKey('project-scan')));
    await tester.pump();
    expect(find.text('프로젝트 등록 중…'), findsOneWidget);
    expect(tester.widget<TextField>(find.byType(TextField)).enabled, isFalse);
    await tester.tap(find.byKey(const ValueKey('project-scan')));
    await tester.pump();
    expect(client.scannedPaths, ['~']);
    client.pending.complete(
      const ProjectScanResult(skipped: ['existing'], errors: ['저장소 등록 실패']),
    );
    await tester.pumpAndSettle();
    expect(find.text('이미 등록된 프로젝트 1개'), findsOneWidget);
    expect(find.text('저장소 등록 실패'), findsOneWidget);
  });

  testWidgets('Git 저장소가 없는 결과와 연결 오류를 구분한다', (tester) async {
    final client = FakeDesktopClient()..scanResult = const ProjectScanResult();
    await openRegistry(tester, client);
    await tester.tap(find.byKey(const ValueKey('project-scan')));
    await tester.pumpAndSettle();
    expect(find.text('이 폴더에서 Git 저장소를 찾지 못했습니다.'), findsOneWidget);
    client.scanError = TimeoutException('offline');
    await tester.tap(find.byKey(const ValueKey('project-scan')));
    await tester.pumpAndSettle();
    expect(find.textContaining('PC에 요청하지 못했습니다'), findsOneWidget);
    expect(find.text('이 폴더에서 Git 저장소를 찾지 못했습니다.'), findsNothing);
  });
}

class _PendingScanClient extends FakeDesktopClient {
  final pending = Completer<ProjectScanResult>();

  @override
  Future<ProjectScanResult> scanProjects(
    DesktopConnection connection,
    String rootPath,
  ) {
    scannedPaths.add(rootPath);
    return pending.future;
  }
}
