import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/ai_accounts/data/ai_login_frame.dart';

void main() {
  test('로그인 준비·출력·종료 이벤트를 독립적으로 읽는다', () {
    expect(AiLoginFrame.parse('{"type":"ready"}').type, AiLoginFrameType.ready);
    expect(
      AiLoginFrame.parse('{"type":"output","data":"OAuth code"}').data,
      'OAuth code',
    );
    expect(AiLoginFrame.parse('{"type":"exit","exitCode":1}').exitCode, 1);
  });
  test('서버 오류 문구가 없는 경우 안전한 안내를 사용한다', () {
    expect(
      AiLoginFrame.parse('{"type":"error","error":123}').error,
      '로그인 터미널에 연결하지 못했습니다.',
    );
  });
  for (final message in [
    '{bad',
    '[]',
    '{"type":"unknown"}',
    '{"type":"output","data":123}',
  ]) {
    test('잘못된 로그인 프레임을 거절한다: $message', () {
      expect(() => AiLoginFrame.parse(message), throwsFormatException);
    });
  }
}
