import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/features/chat_history/domain/chat_history.dart';
import 'package:kanvibe_mobile/src/features/chat_history/domain/task_file.dart';
import 'package:kanvibe_mobile/src/features/chat_history/presentation/chat_code_block.dart';
import 'package:kanvibe_mobile/src/features/chat_history/presentation/chat_file_preview_screen.dart';
import 'package:kanvibe_mobile/src/features/chat_history/presentation/chat_message_tile.dart';

import '../../../support/fake_desktop.dart';

const planPath = '/repo/docs/plan.md';
const sourcePath = '/repo/src/app.ts';
const planContent = '# 배포 계획\n\n**금요일**에 배포합니다.';
const networkBlockingPolicy =
    '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; script-src \'unsafe-inline\'; style-src \'unsafe-inline\'; img-src data: blob:; font-src data:; form-action \'none\'">';

Future<void> pumpMessage(
  WidgetTester tester,
  FakeDesktopClient client,
  ChatMessage message, {
  String? taskId = 'task-1',
}) async {
  tester.view.physicalSize = phoneSize;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(
    wrapWithApp(
      Scaffold(
        body: SingleChildScrollView(
          padding: const EdgeInsets.all(16),
          child: ChatMessageTile(
            provider: 'claude',
            message: message,
            taskId: taskId,
          ),
        ),
      ),
      client: client,
      size: phoneSize,
    ),
  );
  await tester.pumpAndSettle();
}

const changedFilesMessage = ChatMessage(
  role: 'assistant',
  text: '파일 두 개를 바꿨습니다.',
  files: [
    ChatMessageFile(path: planPath, change: ChatMessageFileChange.created),
    ChatMessageFile(path: sourcePath, change: ChatMessageFileChange.modified),
  ],
);

void main() {
  testWidgets('바뀐 파일마다 파일 이름과 변경 종류를 적은 카드를 보여 준다', (tester) async {
    await pumpMessage(tester, FakeDesktopClient(), changedFilesMessage);

    final createdCard = find.byTooltip(planPath);
    final modifiedCard = find.byTooltip(sourcePath);
    expect(
      find.descendant(of: createdCard, matching: find.text('plan.md')),
      findsOneWidget,
    );
    expect(
      find.descendant(of: createdCard, matching: find.text('새로 만듦')),
      findsOneWidget,
    );
    expect(
      find.descendant(of: modifiedCard, matching: find.text('app.ts')),
      findsOneWidget,
    );
    expect(
      find.descendant(of: modifiedCard, matching: find.text('수정함')),
      findsOneWidget,
    );
    for (final path in [planPath, sourcePath]) {
      expect(
        tester.getSize(find.byTooltip(path)).height,
        greaterThanOrEqualTo(48),
      );
    }
    expect(tester.takeException(), isNull);
  });

  testWidgets('태스크 없이 그린 메시지는 열 수 없는 파일 카드를 만들지 않는다', (tester) async {
    await pumpMessage(
      tester,
      FakeDesktopClient(),
      changedFilesMessage,
      taskId: null,
    );

    expect(find.text('파일 두 개를 바꿨습니다.'), findsOneWidget);
    expect(find.text('plan.md'), findsNothing);
    expect(find.text('app.ts'), findsNothing);
  });

  testWidgets('도구 실행 줄은 도구 이름과 첫 줄 요약을 보여 주고 접힌 채로도 파일 카드를 연다', (tester) async {
    await pumpMessage(
      tester,
      FakeDesktopClient(),
      const ChatMessage(
        role: 'tool',
        text: '계획 문서를 썼습니다\n\n도구 결과 전문',
        toolName: 'Write',
        files: [
          ChatMessageFile(
            path: planPath,
            change: ChatMessageFileChange.created,
          ),
        ],
      ),
    );

    expect(find.text('Write'), findsOneWidget);
    expect(find.text('도구 실행'), findsNothing);
    expect(find.text('계획 문서를 썼습니다'), findsOneWidget);
    expect(find.text('도구 결과 전문'), findsNothing);
    expect(find.text('plan.md'), findsOneWidget);
  });

  testWidgets('Markdown 파일 카드를 누르면 그린 본문을 열고 원문으로 바꿔 읽고 전체를 복사한다', (
    tester,
  ) async {
    String? copiedText;
    tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
      SystemChannels.platform,
      (call) async {
        if (call.method == 'Clipboard.setData') {
          copiedText = (call.arguments as Map)['text'] as String;
        }
        return null;
      },
    );
    addTearDown(
      () => tester.binding.defaultBinaryMessenger.setMockMethodCallHandler(
        SystemChannels.platform,
        null,
      ),
    );
    final client = FakeDesktopClient()..taskFilesByPath[planPath] = planContent;
    await pumpMessage(tester, client, changedFilesMessage);

    await tester.tap(find.text('plan.md'));
    await tester.pumpAndSettle();

    expect(client.requestedTaskFilePaths, [planPath]);
    expect(find.widgetWithText(AppBar, 'plan.md'), findsOneWidget);
    expect(find.text('배포 계획'), findsOneWidget);
    expect(find.text('금요일에 배포합니다.'), findsOneWidget);
    expect(find.text(planContent), findsNothing);

    await tester.tap(find.text('원문'));
    await tester.pumpAndSettle();
    expect(find.text(planContent), findsOneWidget);
    expect(find.text('배포 계획'), findsNothing);

    await tester.tap(find.byTooltip('파일 내용 복사'));
    await tester.pumpAndSettle();
    expect(copiedText, planContent);
    expect(find.text('파일 내용을 복사했습니다.'), findsOneWidget);

    await tester.tap(find.text('미리보기'));
    await tester.pumpAndSettle();
    expect(find.text('배포 계획'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('긴 Markdown 파일은 화면에 보이는 블록만 만들고 끝까지 밀면 마지막 블록을 보여 준다', (
    tester,
  ) async {
    final paragraphs = [
      for (var index = 0; index < 400; index++) '문단 $index 의 본문입니다.',
    ];
    final client = FakeDesktopClient()
      ..taskFilesByPath[planPath] =
          '```dart\nvoid main() {}\n```\n\n${paragraphs.join('\n\n')}';
    await pumpMessage(tester, client, changedFilesMessage);

    await tester.tap(find.text('plan.md'));
    await tester.pumpAndSettle();

    expect(find.byType(ChatCodeBlock), findsOneWidget);
    expect(find.text(paragraphs.first), findsOneWidget);
    expect(find.text(paragraphs.last), findsNothing);

    await tester.scrollUntilVisible(
      find.text(paragraphs.last),
      2000,
      scrollable: find
          .descendant(
            of: find.byType(ChatFilePreviewScreen),
            matching: find.byWidgetPredicate(
              (widget) =>
                  widget is Scrollable &&
                  widget.axisDirection == AxisDirection.down,
            ),
          )
          .first,
      maxScrolls: 200,
    );
    expect(find.text(paragraphs.last), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('그려 볼 수 없는 형식은 전환 버튼 없이 원문만 보여 준다', (tester) async {
    const source = 'export const answer: number = 42;';
    final client = FakeDesktopClient()..taskFilesByPath[sourcePath] = source;
    await pumpMessage(tester, client, changedFilesMessage);

    await tester.tap(find.text('app.ts'));
    await tester.pumpAndSettle();

    expect(find.widgetWithText(AppBar, 'app.ts'), findsOneWidget);
    expect(find.text(source), findsOneWidget);
    expect(find.text('미리보기'), findsNothing);
    expect(find.text('원문'), findsNothing);
    expect(tester.takeException(), isNull);
  });

  for (final (name, failure, notice) in <(String, Object, String)>[
    (
      '태스크 폴더 밖',
      const TaskFileException(TaskFileFailure.outsideTaskFolder),
      '이 태스크 폴더 밖에 있는 파일은 열 수 없습니다.',
    ),
    (
      '없는 파일',
      const TaskFileException(TaskFileFailure.notFound),
      '파일을 찾을 수 없습니다. 파일이 지워졌거나 데스크탑 업데이트가 필요할 수 있습니다.',
    ),
    (
      '너무 큰 파일',
      const TaskFileException(TaskFileFailure.tooLarge),
      '파일이 1MB보다 커서 열 수 없습니다.',
    ),
    (
      '그 밖의 읽기 실패',
      const DesktopRequestException(500),
      '파일을 읽지 못했습니다. 다시 시도해 주세요.',
    ),
  ]) {
    testWidgets('파일 열기 실패($name)는 사유를 안내하고 다시 시도하면 새로 요청해 본문을 보여 준다', (
      tester,
    ) async {
      final client = FakeDesktopClient()..taskFilesByPath[planPath] = failure;
      await pumpMessage(tester, client, changedFilesMessage);

      await tester.tap(find.text('plan.md'));
      await tester.pumpAndSettle();
      expect(find.text(notice), findsOneWidget);
      expect(find.text('배포 계획'), findsNothing);

      client.taskFilesByPath[planPath] = planContent;
      await tester.tap(find.text('다시 시도'));
      await tester.pumpAndSettle();

      expect(client.requestedTaskFilePaths, [planPath, planPath]);
      expect(find.text(notice), findsNothing);
      expect(find.text('배포 계획'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }

  group('네트워크를 막은 HTML 문서 만들기', () {
    test('doctype이 없으면 정책 meta를 문서 맨 앞에 붙인다', () {
      expect(
        buildSandboxedHtmlDocument('<html><body>본문</body></html>'),
        '$networkBlockingPolicy<html><body>본문</body></html>',
      );
    });

    test('doctype이 있으면 정책 meta를 doctype 바로 뒤에 넣는다', () {
      expect(
        buildSandboxedHtmlDocument(
          '<!DOCTYPE html><html><body>본문</body></html>',
        ),
        '<!DOCTYPE html>$networkBlockingPolicy<html><body>본문</body></html>',
      );
    });

    test('doctype 앞의 공백과 소문자 doctype도 doctype으로 알아본다', () {
      expect(
        buildSandboxedHtmlDocument('\n  <!doctype html>\n<svg></svg>'),
        '\n  <!doctype html>$networkBlockingPolicy\n<svg></svg>',
      );
    });

    test('본문 중간에 나오는 doctype 글자는 문서 시작으로 보지 않는다', () {
      expect(
        buildSandboxedHtmlDocument('<p><!doctype html></p>'),
        '$networkBlockingPolicy<p><!doctype html></p>',
      );
    });
  });

  test('처음 넣은 문서의 로드만 허용하고 다른 주소로의 이동은 모두 막는다', () {
    expect(isSandboxedDocumentLoad('about:blank'), isTrue);
    for (final url in [
      'https://example.com/',
      'http://192.168.0.2/',
      'file:///etc/passwd',
      'about:blank#top',
      'javascript:alert(1)',
      'data:text/html,<p>x</p>',
    ]) {
      expect(isSandboxedDocumentLoad(url), isFalse, reason: url);
    }
  });
}
