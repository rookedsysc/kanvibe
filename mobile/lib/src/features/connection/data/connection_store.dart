import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import '../../../common/network/desktop_connection.dart';

/// 연결 정보를 기기에 보관한다.
///
/// 토큰은 데스크탑 터미널에 닿는 열쇠라 일반 설정 저장소가 아니라 보안 저장소에 둔다.
/// 만료시키지 않기로 했으므로, 기기에서 지워지기 전까지는 계속 유효하다.
class ConnectionStore {
  ConnectionStore({FlutterSecureStorage? storage})
    : _storage = storage ?? const FlutterSecureStorage();

  final FlutterSecureStorage _storage;

  static const _connectionKey = 'kanvibe_desktop_connection';

  Future<DesktopConnection?> read() async {
    final raw = await _storage.read(key: _connectionKey);
    if (raw == null) {
      return null;
    }

    try {
      return DesktopConnection.fromJson(
        jsonDecode(raw) as Map<String, dynamic>,
      );
    } on FormatException {
      /// 저장값이 깨졌으면 다시 페어링하면 되는 손실이다
      return null;
    }
  }

  Future<void> write(DesktopConnection connection) => _storage.write(
    key: _connectionKey,
    value: jsonEncode(connection.toJson()),
  );

  Future<void> clear() => _storage.delete(key: _connectionKey);
}
