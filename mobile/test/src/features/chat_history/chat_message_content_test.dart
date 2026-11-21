import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:kanvibe_mobile/src/features/chat_history/presentation/widgets/chat_message_content.dart';

void main() {
  testWidgets('대화 Markdown은 제목·표·코드로 표시하고 선택을 허용한다', (tester) async {
    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(
          body: ChatMessageContent(
            text: '# 결과\n\n| 파일 | 상태 |\n| --- | --- |\n| main.dart | 수정 |\n\n`git status`',
          ),
        ),
      ),
    );
    expect(find.byType(MarkdownBody), findsOneWidget);
    expect(find.textContaining('결과', findRichText: true), findsWidgets);
    expect(find.textContaining('main.dart', findRichText: true), findsWidgets);
    expect(
      tester.widget<MarkdownBody>(find.byType(MarkdownBody)).selectable,
      isTrue,
    );
  });
}
