import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import '../../task_create/domain/session_type.dart';
import '../domain/terminal_shortcut.dart';

class TerminalShortcutStore {
  TerminalShortcutStore({FlutterSecureStorage? storage})
    : _storage = storage ?? const FlutterSecureStorage();

  final FlutterSecureStorage _storage;
  static const _key = 'kanvibe_terminal_shortcuts_v1';

  Future<Map<SessionType, TerminalShortcutProfile>> read() async {
    final raw = await _storage.read(key: _key);
    Object? decoded;
    try {
      decoded = raw == null ? null : jsonDecode(raw);
    } on FormatException {
      decoded = null;
    }
    return {
      for (final type in SessionType.values) type: _readProfile(decoded, type),
    };
  }

  TerminalShortcutProfile _readProfile(Object? decoded, SessionType type) {
    final profile = decoded is Map ? decoded[type.name] : null;
    if (profile is! Map || profile['ids'] is! List) {
      return TerminalShortcutProfile.defaults(type);
    }
    final allowed = terminalShortcuts(type)
        .map((shortcut) => shortcut.id)
        .toSet();
    final ids = (profile['ids'] as List)
        .whereType<String>()
        .where(allowed.contains)
        .toSet()
        .toList();
    return TerminalShortcutProfile(
      enabled: profile['enabled'] != false,
      ids: ids,
    );
  }

  Future<void> write(Map<SessionType, TerminalShortcutProfile> profiles) =>
      _storage.write(
        key: _key,
        value: jsonEncode({
          for (final entry in profiles.entries)
            entry.key.name: {
              'enabled': entry.value.enabled,
              'ids': entry.value.ids,
            },
        }),
      );
}
