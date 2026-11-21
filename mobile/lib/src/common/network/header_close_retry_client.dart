import 'package:http/http.dart' as http;

/// 응답 헤더 전에 연결이 닫힌 빈 본문 조회만 새 요청으로 한 번 복구한다.
/// 응답 스트림을 받은 뒤의 실패와 변경 요청은 재전송하지 않는다.
class HeaderCloseRetryClient extends http.BaseClient {
  HeaderCloseRetryClient(this._inner);

  final http.Client _inner;
  static const _closedBeforeHeaders =
      'Connection closed before full header was received';

  @override
  Future<http.StreamedResponse> send(http.BaseRequest request) async {
    final retry = _copyEmptyRead(request);
    try {
      return await _inner.send(request);
    } on http.ClientException catch (error) {
      if (retry == null || error.message != _closedBeforeHeaders) rethrow;
      return _inner.send(retry);
    }
  }

  http.Request? _copyEmptyRead(http.BaseRequest request) {
    if (request is! http.Request ||
        (request.method != 'GET' && request.method != 'HEAD') ||
        request.bodyBytes.isNotEmpty) {
      return null;
    }
    final copy = request is http.AbortableRequest
        ? http.AbortableRequest(
            request.method,
            request.url,
            abortTrigger: request.abortTrigger,
          )
        : http.Request(request.method, request.url);
    return copy
      ..headers.addAll(request.headers)
      ..followRedirects = request.followRedirects
      ..maxRedirects = request.maxRedirects
      ..persistentConnection = request.persistentConnection;
  }

  @override
  void close() => _inner.close();
}
