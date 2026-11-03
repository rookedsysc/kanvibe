import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_theme.dart';
import '../../../task_create/domain/session_type.dart';
import '../../application/terminal_shortcut_controller.dart';
import '../../domain/terminal_shortcut.dart';
import 'terminal_shortcut_settings_sheet.dart';
import 'terminal_shortcut_panel.dart';

class TerminalShortcutBar extends ConsumerStatefulWidget {
  const TerminalShortcutBar({
    required this.sessionType,
    required this.onShortcut,
    this.onCollapse,
    this.isBusy = false,
    this.maxPanelHeight,
    super.key,
  });

  final SessionType sessionType;
  final ValueChanged<TerminalShortcut> onShortcut;
  final VoidCallback? onCollapse;
  final bool isBusy;
  final double? maxPanelHeight;

  @override
  ConsumerState<TerminalShortcutBar> createState() =>
      _TerminalShortcutBarState();
}

class _TerminalShortcutBarState extends ConsumerState<TerminalShortcutBar> {
  bool _isPanelOpen = false;

  @override
  void didUpdateWidget(TerminalShortcutBar oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.sessionType != widget.sessionType) _isPanelOpen = false;
  }

  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(terminalShortcutControllerProvider);
    final profile =
        settings.asData?.value[widget.sessionType] ??
        TerminalShortcutProfile.defaults();
    final shortcuts = {
      for (final shortcut in profile.combos) shortcut.id: shortcut,
    };
    return Focus(
      canRequestFocus: false,
      descendantsAreFocusable: false,
      child: Container(
        decoration: const BoxDecoration(
          color: AppColors.terminalChrome,
          border: Border(top: BorderSide(color: AppColors.borderDefault)),
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (_isPanelOpen)
              TerminalShortcutPanel(
                key: ValueKey(widget.sessionType),
                sessionType: widget.sessionType,
                profile: profile,
                maxHeight: widget.maxPanelHeight,
                onShortcut: widget.onShortcut,
                isBusy: widget.isBusy,
              ),
            Row(
              children: [
                Semantics(
                  identifier: 'shortcut-panel',
                  selected: _isPanelOpen,
                  child: TextButton(
                    onPressed: () =>
                        setState(() => _isPanelOpen = !_isPanelOpen),
                    style: TextButton.styleFrom(
                      minimumSize: const Size(40, 32),
                      tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                      padding: const EdgeInsets.symmetric(horizontal: 8),
                      foregroundColor: AppColors.brandPrimary,
                      textStyle: const TextStyle(
                        fontSize: 12,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    child: Tooltip(
                      message: _isPanelOpen ? '빠른 단축키 닫기' : '빠른 단축키',
                      child: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Text(
                            widget.sessionType == SessionType.terminal
                                ? '키'
                                : widget.sessionType.name,
                          ),
                          Icon(
                            _isPanelOpen
                                ? Icons.keyboard_arrow_down
                                : Icons.keyboard_arrow_up,
                            size: 14,
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
                Expanded(
                  child: profile.enabled
                      ? SingleChildScrollView(
                          key: ValueKey(widget.sessionType),
                          scrollDirection: Axis.horizontal,
                          padding: const EdgeInsets.symmetric(
                            horizontal: 2,
                            vertical: 2,
                          ),
                          child: profile.ids.isEmpty
                              ? TextButton(
                                  onPressed: _openSettings,
                                  style: TextButton.styleFrom(
                                    minimumSize: const Size(0, 32),
                                    tapTargetSize:
                                        MaterialTapTargetSize.shrinkWrap,
                                    textStyle: const TextStyle(fontSize: 12),
                                  ),
                                  child: const Text('콤보 추가'),
                                )
                              : Row(
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
                            '${widget.sessionType.name} · 단축키 꺼짐',
                            style: const TextStyle(
                              color: AppColors.textSecondary,
                            ),
                          ),
                        ),
                ),
                _control(
                  'shortcut-settings',
                  '${widget.sessionType.name} 단축키 설정',
                  Icons.tune,
                  _openSettings,
                ),
                if (widget.onCollapse case final onCollapse?)
                  _control(
                    'shortcut-collapse',
                    '단축키 바 접기',
                    Icons.keyboard_arrow_down,
                    onCollapse,
                  ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  void _openSettings() => showModalBottomSheet<void>(
    context: context,
    isScrollControlled: true,
    useSafeArea: true,
    builder: (_) =>
        TerminalShortcutSettingsSheet(sessionType: widget.sessionType),
  );

  Widget _control(
    String id,
    String tooltip,
    IconData icon,
    VoidCallback onPressed,
  ) => Semantics(
    identifier: id,
    child: IconButton(
      constraints: const BoxConstraints.tightFor(width: 36, height: 36),
      padding: const EdgeInsets.all(4),
      style: IconButton.styleFrom(
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        visualDensity: VisualDensity.standard,
      ),
      tooltip: tooltip,
      onPressed: onPressed,
      icon: Icon(icon, size: 18),
    ),
  );

  Widget _key(TerminalShortcut shortcut) {
    return Semantics(
      identifier: 'shortcut-${shortcut.id}',
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 2),
        child: Tooltip(
          message: shortcut.description,
          child: TextButton(
            onPressed: () => widget.onShortcut(shortcut),
            style: TextButton.styleFrom(
              minimumSize: const Size(40, 32),
              tapTargetSize: MaterialTapTargetSize.shrinkWrap,
              visualDensity: VisualDensity.standard,
              padding: const EdgeInsets.symmetric(horizontal: 8),
              foregroundColor: AppColors.textPrimary,
              backgroundColor: AppColors.buttonNeutral,
              shape: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(6),
                side: const BorderSide(color: AppColors.borderStrong),
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
