import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

class BoardProjectFilterStore {
  BoardProjectFilterStore({FlutterSecureStorage? storage})
    : _storage = storage ?? const FlutterSecureStorage();

  final FlutterSecureStorage _storage;

  String _key(String connectionId) =>
      'kanvibe_board_project_filter_v1:$connectionId';

  Future<Set<String>> read(String connectionId) async {
    final raw = await _storage.read(key: _key(connectionId));
    if (raw == null) return const {};
    try {
      final decoded = jsonDecode(raw);
      if (decoded is! List || decoded.any((id) => id is! String)) {
        return const {};
      }
      return Set.unmodifiable(decoded.cast<String>());
    } on FormatException {
      return const {};
    }
  }

  Future<void> write(String connectionId, Set<String> projectIds) => _storage
      .write(key: _key(connectionId), value: jsonEncode(projectIds.toList()));
}
