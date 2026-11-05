import 'dart:convert';

import 'package:flutter_secure_storage/flutter_secure_storage.dart';

import '../../task_create/domain/session_type.dart';
import '../domain/terminal_shortcut.dart';

class TerminalShortcutStore {
  TerminalShortcutStore({FlutterSecureStorage? storage})
    : _storage = storage ?? const FlutterSecureStorage();

  final FlutterSecureStorage _storage;
  static const _key = 'kanvibe_terminal_shortcuts_v3';
  static const _singleKeyChordKey = 'kanvibe_terminal_shortcuts_v2';
  static const _legacyKey = 'kanvibe_terminal_shortcuts_v1';

  /// 가장 최신 형식부터 찾는다. 앞선 형식은 읽기만 하고 지우지 않으므로, 저장하면 v3로만 쓰인다
  Future<Map<SessionType, TerminalShortcutProfile>> read() async {
    final current =
        await _storage.read(key: _key) ??
        await _storage.read(key: _singleKeyChordKey);
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
        (raw['ctrl'] != null && raw['ctrl'] is! bool) ||
        (raw['alt'] != null && raw['alt'] is! bool) ||
        (raw['shift'] != null && raw['shift'] is! bool)) {
      return null;
    }
    final keys = _readChordKeys(raw);
    if (keys == null) return null;
    final chord = TerminalShortcutChord(
      keys: keys,
      ctrl: raw['ctrl'] == true,
      alt: raw['alt'] == true,
      shift: raw['shift'] == true,
    );
    return chord.isValid ? chord : null;
  }

  /// v3는 `keys` 목록을, v1·v2는 `key` 하나(입력 문자면 `character`)를 담는다
  List<String>? _readChordKeys(Map raw) {
    final keys = raw['keys'];
    if (keys is List) {
      return keys.every((key) => key is String) ? keys.cast<String>() : null;
    }
    final key = raw['key'];
    final character = raw['character'];
    if (key is! String || (character != null && character is! String)) {
      return null;
    }
    if (key != 'character') return [key];
    return character is String && character.runes.length == 1
        ? [character]
        : null;
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
                        'keys': chord.keys,
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
