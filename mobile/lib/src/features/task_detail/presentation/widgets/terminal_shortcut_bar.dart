import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_theme.dart';
import '../../../task_create/domain/session_type.dart';
import '../../application/terminal_shortcut_controller.dart';
import '../../domain/terminal_shortcut.dart';
import 'terminal_shortcut_settings_sheet.dart';

class TerminalShortcutBar extends ConsumerWidget {
  const TerminalShortcutBar({
    required this.sessionType,
    required this.onShortcut,
    this.ctrlPressed = false,
    this.altPressed = false,
    this.isBusy = false,
    super.key,
  });

  final SessionType sessionType;
  final ValueChanged<TerminalShortcut> onShortcut;
  final bool ctrlPressed;
  final bool altPressed;
  final bool isBusy;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final settings = ref.watch(terminalShortcutControllerProvider);
    final profile =
        settings.asData?.value[sessionType] ??
        TerminalShortcutProfile.defaults(sessionType);
    final shortcuts = {
      for (final shortcut in profile.shortcuts(sessionType))
        shortcut.id: shortcut,
    };
    return Focus(
      canRequestFocus: false,
      descendantsAreFocusable: false,
      child: Container(
        decoration: const BoxDecoration(
          color: AppColors.terminalChrome,
          border: Border(top: BorderSide(color: AppColors.borderDefault)),
        ),
        child: Row(
          children: [
            Expanded(
              child: profile.enabled
                  ? SingleChildScrollView(
                      key: ValueKey(sessionType),
                      scrollDirection: Axis.horizontal,
                      padding: const EdgeInsets.symmetric(
                        horizontal: 2,
                        vertical: 2,
                      ),
                      child: Row(
                        children: [
                          for (final id in profile.ids)
                            if (shortcuts[id] case final shortcut?)
                              _key(shortcut),
                        ],
                      ),
                    )
                  : Padding(
                      padding: const EdgeInsets.only(left: 12),
                      child: Text(
                        '${sessionType.name} · 단축키 꺼짐',
                        style: const TextStyle(color: AppColors.textSecondary),
                      ),
                    ),
            ),
            Semantics(
              identifier: 'shortcut-settings',
              child: IconButton(
                constraints: const BoxConstraints.tightFor(
                  width: 36,
                  height: 36,
                ),
                padding: const EdgeInsets.all(4),
                style: IconButton.styleFrom(
                  tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                  visualDensity: VisualDensity.standard,
                ),
                tooltip: '${sessionType.name} 단축키 설정',
                onPressed: () => showModalBottomSheet<void>(
                  context: context,
                  isScrollControlled: true,
                  useSafeArea: true,
                  builder: (_) =>
                      TerminalShortcutSettingsSheet(sessionType: sessionType),
                ),
                icon: const Icon(Icons.tune, size: 18),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _key(TerminalShortcut shortcut) {
    final pressed =
        (shortcut.id == 'ctrl' && ctrlPressed) ||
        (shortcut.id == 'alt' && altPressed);
    return Semantics(
      identifier: 'shortcut-${shortcut.id}',
      selected: pressed,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 2),
        child: Tooltip(
          message: shortcut.description,
          child: TextButton(
            onPressed: isBusy && shortcut.action != null
                ? null
                : () => onShortcut(shortcut),
            style: TextButton.styleFrom(
              minimumSize: const Size(40, 32),
              tapTargetSize: MaterialTapTargetSize.shrinkWrap,
              visualDensity: VisualDensity.standard,
              padding: const EdgeInsets.symmetric(horizontal: 8),
              foregroundColor: AppColors.textPrimary,
              backgroundColor: AppColors.buttonNeutral,
              shape: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(6),
                side: BorderSide(
                  color: pressed
                      ? AppColors.brandPrimary
                      : AppColors.borderStrong,
                ),
              ),
              textStyle: const TextStyle(
                fontFamily: monoFontFamily,
                fontSize: 12,
              ),
            ),
            child: Text(shortcut.label),
          ),
        ),
      ),
    );
  }
}
