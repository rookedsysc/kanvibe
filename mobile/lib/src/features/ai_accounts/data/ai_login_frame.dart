import 'dart:convert';

enum AiLoginFrameType { ready, output, exit, error }

class AiLoginFrame {
  const AiLoginFrame({
    required this.type,
    this.data,
    this.error,
    this.exitCode,
  });
  final AiLoginFrameType type;
  final String? data;
  final String? error;
  final int? exitCode;
  factory AiLoginFrame.parse(String message) {
    final raw = jsonDecode(message);
    if (raw is! Map) throw const FormatException('로그인 터미널 응답을 읽지 못했습니다.');
    return switch (raw['type']) {
      'ready' => const AiLoginFrame(type: AiLoginFrameType.ready),
      'output' when raw['data'] is String => AiLoginFrame(
        type: AiLoginFrameType.output,
        data: raw['data'] as String,
      ),
      'exit' when raw['exitCode'] is int => AiLoginFrame(
        type: AiLoginFrameType.exit,
        exitCode: raw['exitCode'] as int,
      ),
      'error' => AiLoginFrame(
        type: AiLoginFrameType.error,
        error: raw['error'] is String
            ? raw['error'] as String
            : '로그인 터미널에 연결하지 못했습니다.',
      ),
      _ => throw const FormatException('지원하지 않는 로그인 터미널 응답입니다.'),
    };
  }
}
