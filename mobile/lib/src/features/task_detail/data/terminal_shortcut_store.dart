import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import '../../task_create/domain/session_type.dart';
import '../domain/terminal_shortcut.dart';

class TerminalShortcutStore {
  TerminalShortcutStore({FlutterSecureStorage? storage})
    : _storage = storage ?? const FlutterSecureStorage();

  final FlutterSecureStorage _storage;
  static const _key = 'kanvibe_terminal_shortcuts_v2';
  static const _legacyKey = 'kanvibe_terminal_shortcuts_v1';

  Future<Map<SessionType, TerminalShortcutProfile>> read() async {
    final current = await _storage.read(key: _key);
    final raw = current ?? await _storage.read(key: _legacyKey);
    Object? decoded;
    try {
      decoded = raw == null ? null : jsonDecode(raw);
    } on FormatException {
      decoded = null;
    }
    return {
      for (final type in SessionType.values)
        type: _readProfile(decoded, type, isLegacy: current == null),
    };
  }

  TerminalShortcutProfile _readProfile(
    Object? decoded,
    SessionType type, {
    required bool isLegacy,
  }) {
    final profile = decoded is Map ? decoded[type.name] : null;
    if (profile is! Map || profile['ids'] is! List) {
      return TerminalShortcutProfile.defaults();
    }
    return TerminalShortcutProfile(
      enabled: profile['enabled'] != false,
      combos: _readCombos(profile['combos'], isLegacy: isLegacy),
      ids: (profile['ids'] as List).whereType<String>().toList(),
    );
  }

  List<TerminalShortcut> _readCombos(Object? raw, {required bool isLegacy}) {
    if (raw is! List) return [];
    final combos = <TerminalShortcut>[];
    final seen = <String>{};
    for (final entry in raw.take(maxTerminalCombos)) {
      if (entry is! Map) continue;
      final id = entry['id'];
      final label = entry['label'];
      if (id is! String ||
          !id.startsWith('combo-') ||
          id.length > 80 ||
          seen.contains(id) ||
          label is! String ||
          label.trim().isEmpty ||
          label.length > maxTerminalComboLabelLength) {
        continue;
      }
      Object? rawChord = entry['chord'];
      if (isLegacy) {
        final strokes = entry['strokes'];
        if (strokes is! List || strokes.length != 1) continue;
        rawChord = strokes.single;
      }
      final chord = _readChord(rawChord);
      if (chord == null) continue;
      seen.add(id);
      combos.add(TerminalShortcut.combo(id, label.trim(), chord));
    }
    return combos;
  }

  TerminalShortcutChord? _readChord(Object? raw) {
    if (raw is! Map ||
        raw['key'] is! String ||
        (raw['character'] != null && raw['character'] is! String) ||
        (raw['ctrl'] != null && raw['ctrl'] is! bool) ||
        (raw['alt'] != null && raw['alt'] is! bool) ||
        (raw['shift'] != null && raw['shift'] is! bool)) {
      return null;
    }
    final chord = TerminalShortcutChord(
      key: raw['key'] as String,
      character: raw['character'] as String? ?? '',
      ctrl: raw['ctrl'] == true,
      alt: raw['alt'] == true,
      shift: raw['shift'] == true,
    );
    return chord.isValid ? chord : null;
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
                  if (combo.chord case final chord?)
                    {
                      'id': combo.id,
                      'label': combo.label,
                      'chord': {
                        'key': chord.key,
                        'character': chord.character,
                        'ctrl': chord.ctrl,
                        'alt': chord.alt,
                        'shift': chord.shift,
                      },
                    },
              ],
            },
        }),
      );
}
