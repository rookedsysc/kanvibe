import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import '../../../common/network/desktop_connection.dart';
import '../domain/saved_connections.dart';

/// 연결 정보를 기기에 보관한다.
///
/// 토큰은 데스크탑 터미널에 닿는 열쇠라 일반 설정 저장소가 아니라 보안 저장소에 둔다.
/// 만료시키지 않기로 했으므로, 기기에서 지워지기 전까지는 계속 유효하다.
class ConnectionStore {
  ConnectionStore({FlutterSecureStorage? storage})
    : _storage = storage ?? const FlutterSecureStorage();

  final FlutterSecureStorage _storage;

  static const _connectionKey = 'kanvibe_desktop_connection';
  static const _connectionsKey = 'kanvibe_desktop_connections';

  Future<SavedConnections> readRegistry() async {
    final registryRaw = await _storage.read(key: _connectionsKey);
    if (registryRaw != null) {
      try {
        return SavedConnections.fromJson(jsonDecode(registryRaw)) ??
            const SavedConnections();
      } on FormatException {
        return const SavedConnections();
      }
    }

    final raw = await _storage.read(key: _connectionKey);
    if (raw == null) return const SavedConnections();

    try {
      final connection = DesktopConnection.fromJson(
        jsonDecode(raw) as Map<String, dynamic>,
      );
      if (connection == null) return const SavedConnections();
      final registry = SavedConnections(
        connections: [connection],
        activeId: connection.id,
      );
      await writeRegistry(registry);
      return registry;
    } on FormatException {
      return const SavedConnections();
    } on TypeError {
      return const SavedConnections();
    }
  }

  /// 새 값의 저장이 성공한 뒤에만 이전 단일 연결을 지운다.
  Future<void> writeRegistry(SavedConnections registry) async {
    await _storage.write(
      key: _connectionsKey,
      value: jsonEncode(registry.toJson()),
    );
    await _storage.delete(key: _connectionKey);
  }
}
