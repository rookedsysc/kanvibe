import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/chat_history/domain/chat_history.dart';

void main() {
  test('도구 이름과 바뀐 파일 목록을 메시지에서 읽는다', () {
    final message = ChatMessage.fromJson({
      'role': 'tool',
      'text': '파일을 썼습니다',
      'toolName': 'Write',
      'files': [
        {'path': '/repo/docs/plan.md', 'change': 'created'},
        {'path': '/repo/src/app.ts', 'change': 'modified'},
      ],
    });

    expect(message.toolName, 'Write');
    expect(message.files.map((file) => file.path), [
      '/repo/docs/plan.md',
      '/repo/src/app.ts',
    ]);
    expect(message.files.map((file) => file.change), [
      ChatMessageFileChange.created,
      ChatMessageFileChange.modified,
    ]);
    expect(message.files.map((file) => file.name), ['plan.md', 'app.ts']);
  });

  test('오래된 데스크탑이 두 필드를 보내지 않으면 도구 이름과 파일 없이 읽는다', () {
    final message = ChatMessage.fromJson({'role': 'assistant', 'text': '답변'});

    expect(message.text, '답변');
    expect(message.toolName, isNull);
    expect(message.files, isEmpty);
  });

  test('모양이 맞지 않는 파일 항목과 모르는 변경 종류는 그 항목만 건너뛴다', () {
    final message = ChatMessage.fromJson({
      'role': 'tool',
      'text': '',
      'toolName': 42,
      'files': [
        'not-a-map',
        null,
        {'path': 7, 'change': 'created'},
        {'change': 'created'},
        {'path': '/repo/removed.ts', 'change': 'deleted'},
        {'path': '/repo/kept.ts', 'change': 'modified'},
      ],
    });

    expect(message.toolName, isNull);
    expect(message.files, hasLength(1));
    expect(message.files.single.path, '/repo/kept.ts');
    expect(message.files.single.change, ChatMessageFileChange.modified);
  });

  test('파일 목록이 배열이 아니면 파일 없이 읽는다', () {
    final message = ChatMessage.fromJson({
      'role': 'tool',
      'text': '',
      'files': {'path': '/repo/a.ts', 'change': 'created'},
    });

    expect(message.files, isEmpty);
  });
}
