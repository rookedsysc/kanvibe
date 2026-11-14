import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/features/chat_history/domain/chat_history.dart';
import 'package:kanvibe_mobile/src/features/chat_history/presentation/chat_code_block.dart';
import 'package:kanvibe_mobile/src/features/chat_history/presentation/chat_message_tile.dart';

Future<void> pumpAssistantMessage(
  WidgetTester tester,
  String markdown, {
  double textScale = 1,
}) async {
  tester.view.physicalSize = const Size(390, 844);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(
    MaterialApp(
      key: ValueKey((markdown, textScale)),
      theme: buildAppTheme(),
      builder: (context, child) => MediaQuery(
        data: MediaQuery.of(context)
            .copyWith(textScaler: TextScaler.linear(textScale)),
        child: child!,
      ),
      home: Scaffold(
        body: SingleChildScrollView(
          padding: const EdgeInsets.all(16),
          child: ChatMessageTile(
            provider: 'codex',
            message: ChatMessage(role: 'assistant', text: markdown),
          ),
        ),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

/// 본문 안에서 가로로 밀 수 있는 첫 영역이 실제로 밀리는 거리
double horizontalScrollExtent(WidgetTester tester, Finder within) {
  final scrollable = tester.state<ScrollableState>(
    find.descendant(of: within, matching: find.byType(Scrollable)).first,
  );
  expect(scrollable.position.axis, Axis.horizontal);
  return scrollable.position.maxScrollExtent;
}

/// 코드 본문에 쓰인 글자색의 가짓수. 기본색 하나에 문법 색이 더해지면 둘 이상이다
int codeColorCount(WidgetTester tester, String code) {
  final colors = <Color>{};
  void collectColors(InlineSpan span) {
    final color = span.style?.color;
    if (color != null) colors.add(color);
    if (span is TextSpan) span.children?.forEach(collectColors);
  }

  collectColors(
    tester
        .widget<SelectableText>(
          find.ancestor(
            of: find.text(code),
            matching: find.byType(SelectableText),
          ),
        )
        .textSpan!,
  );
  return colors.length;
}

void main() {
  testWidgets('코드 블록은 언어 이름을 보여 주고 복사하면 코드만 클립보드에 담는다', (tester) async {
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

    await pumpAssistantMessage(
      tester,
      '설명\n\n```dart\nfinal greeting = "안녕";\n\nprint(greeting);\n```\n\n끝',
    );

    expect(find.text('dart'), findsOneWidget);
    expect(
      find.text('final greeting = "안녕";\n\nprint(greeting);'),
      findsOneWidget,
    );
    await tester.tap(find.byTooltip('코드 복사'));
    await tester.pumpAndSettle();
    expect(copiedText, 'final greeting = "안녕";\n\nprint(greeting);');
    expect(find.text('코드를 복사했습니다.'), findsOneWidget);
    expect(
      codeColorCount(tester, 'final greeting = "안녕";\n\nprint(greeting);'),
      greaterThan(1),
      reason: '아는 언어는 키워드와 문자열에 서로 다른 색을 입힌다',
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('언어가 없으면 text로, 모르는 언어는 적힌 이름 그대로 코드 블록으로 보여 준다', (tester) async {
    await pumpAssistantMessage(
      tester,
      '```\n이름 없는 코드\n```\n\n'
      '```kanvibe-lang\n모르는 <언어> {코드}\n```',
    );

    expect(find.byType(ChatCodeBlock), findsNWidgets(2));
    expect(find.text('text'), findsOneWidget);
    expect(find.text('이름 없는 코드'), findsOneWidget);
    expect(find.text('kanvibe-lang'), findsOneWidget);
    expect(find.text('모르는 <언어> {코드}'), findsOneWidget);
    expect(codeColorCount(tester, '이름 없는 코드'), 1);
    expect(codeColorCount(tester, '모르는 <언어> {코드}'), 1);
    expect(tester.takeException(), isNull);
  });

  testWidgets('인라인 코드는 코드 블록 머리 없이 문장 안에 남는다', (tester) async {
    await pumpAssistantMessage(tester, '`pnpm test` 명령을 실행하세요.');

    expect(find.byType(ChatCodeBlock), findsNothing);
    expect(find.byTooltip('코드 복사'), findsNothing);
    expect(find.text('pnpm test 명령을 실행하세요.'), findsOneWidget);
  });

  for (final textScale in [1.0, 2.0]) {
    testWidgets('아주 긴 코드 한 줄은 글자 배율 $textScale에서도 넘치지 않고 가로로 밀어 읽는다', (
      tester,
    ) async {
      final longLine = 'const veryLongLine = "${'가나다라마바사 ' * 40}";';

      await pumpAssistantMessage(
        tester,
        '```ts\n$longLine\n```',
        textScale: textScale,
      );

      expect(tester.takeException(), isNull);
      expect(find.text(longLine), findsOneWidget);
      expect(
        horizontalScrollExtent(tester, find.byType(ChatCodeBlock)),
        greaterThan(0),
      );
    });
  }

  testWidgets('8열 표는 폰 너비에서 넘치지 않고 가로로 밀어 끝 열까지 읽는다', (tester) async {
    final header = List.generate(8, (index) => '항목 ${index + 1}').join(' | ');
    final divider = List.filled(8, '---').join(' | ');
    final row = List.generate(8, (index) => '값 ${index + 1}번 내용').join(' | ');

    await pumpAssistantMessage(tester, '| $header |\n| $divider |\n| $row |');

    expect(tester.takeException(), isNull);
    expect(find.text('항목 8'), findsOneWidget);
    expect(find.text('값 8번 내용'), findsOneWidget);
    expect(
      horizontalScrollExtent(tester, find.byType(MarkdownBody)),
      greaterThan(0),
    );
  });

  testWidgets('표의 긴 문장 셀은 한 줄로 늘어나지 않고 열 안에서 줄을 바꾼다', (tester) async {
    final sentence = '아주 긴 설명 문장입니다 ' * 12;

    await pumpAssistantMessage(
      tester,
      '| 이름 | 설명 |\n| --- | --- |\n| 가 | $sentence |',
    );

    expect(tester.takeException(), isNull);
    final cell = tester.getSize(find.text(sentence.trim()));
    expect(cell.width, lessThanOrEqualTo(240));
    expect(cell.height, greaterThan(40));
  });

  testWidgets('https 이미지는 누르기 전에는 불러오지 않고, 누르면 높이를 제한해 불러온다', (tester) async {
    await pumpAssistantMessage(
      tester,
      '![구성도](https://example.com/diagram.png)',
    );

    expect(find.byType(Image), findsNothing);

    await tester.tap(find.text('이미지 불러오기: 구성도'));
    await tester.pump();

    // 테스트 환경의 네트워크는 항상 실패로 답하므로 이미지 위젯이 만들어진 뒤 대체 글로 바뀐다.
    expect(find.byType(Image), findsOneWidget);
    expect(tester.getSize(find.byType(Image)).height, lessThanOrEqualTo(280));
    await tester.pumpAndSettle();
    expect(find.text('이미지: 구성도'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('http·file·data 이미지는 불러오지 않고 대체 글만 보여 준다', (tester) async {
    await pumpAssistantMessage(
      tester,
      '![평문 이미지](http://example.com/a.png)\n\n'
      '![기기 파일](file:///private/secret.png)\n\n'
      '![데이터 이미지](data:image/png;base64,AAAA)',
    );

    expect(find.byType(Image), findsNothing);
    expect(find.text('이미지: 평문 이미지'), findsOneWidget);
    expect(find.text('이미지: 기기 파일'), findsOneWidget);
    expect(find.text('이미지: 데이터 이미지'), findsOneWidget);
  });
}
