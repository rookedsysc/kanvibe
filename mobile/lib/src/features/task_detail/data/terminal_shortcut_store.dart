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
    final combos = _readCombos(profile['combos']);
    final allowed = [
      ...terminalShortcuts(type),
      ...combos,
    ].map((shortcut) => shortcut.id).toSet();
    final ids = (profile['ids'] as List)
        .whereType<String>()
        .where(allowed.contains)
        .toSet()
        .toList();
    return TerminalShortcutProfile(
      enabled: profile['enabled'] != false,
      combos: combos,
      ids: ids,
    );
  }

  List<TerminalShortcut> _readCombos(Object? raw) {
    if (raw is! List) return [];
    final combos = <TerminalShortcut>[];
    final seen = <String>{};
    for (final entry in raw.take(maxTerminalCombos)) {
      if (entry is! Map) continue;
      final id = entry['id'];
      final label = entry['label'];
      final rawStrokes = entry['strokes'];
      if (id is! String ||
          !id.startsWith('combo-') ||
          id.length > 80 ||
          seen.contains(id) ||
          label is! String ||
          label.trim().isEmpty ||
          label.length > maxTerminalComboLabelLength ||
          rawStrokes is! List ||
          rawStrokes.isEmpty ||
          rawStrokes.length > maxTerminalComboStrokes) {
        continue;
      }
      final strokes = <TerminalShortcutStroke>[];
      for (final rawStroke in rawStrokes) {
        if (rawStroke is! Map ||
            rawStroke['key'] is! String ||
            (rawStroke['character'] != null &&
                rawStroke['character'] is! String) ||
            (rawStroke['ctrl'] != null && rawStroke['ctrl'] is! bool) ||
            (rawStroke['alt'] != null && rawStroke['alt'] is! bool)) {
          break;
        }
        final stroke = TerminalShortcutStroke(
          key: rawStroke['key'] as String,
          character: rawStroke['character'] as String? ?? '',
          ctrl: rawStroke['ctrl'] == true,
          alt: rawStroke['alt'] == true,
        );
        if (!stroke.isValid) break;
        strokes.add(stroke);
      }
      if (strokes.length != rawStrokes.length) continue;
      seen.add(id);
      combos.add(TerminalShortcut.combo(id, label.trim(), strokes));
    }
    return combos;
  }

  Future<void> write(Map<SessionType, TerminalShortcutProfile> profiles) =>
      _storage.write(
        key: _key,
        value: jsonEncode({
          for (final entry in profiles.entries)
            entry.key.name: {
              'enabled': entry.value.enabled,
              'ids': entry.value.ids,
              'combos': [
                for (final combo in entry.value.combos)
                  {
                    'id': combo.id,
                    'label': combo.label,
                    'strokes': [
                      for (final stroke in combo.strokes)
                        {
                          'key': stroke.key,
                          'character': stroke.character,
                          'ctrl': stroke.ctrl,
                          'alt': stroke.alt,
                        },
                    ],
                  },
              ],
            },
        }),
      );
}
