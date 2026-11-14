import '../../../../common/l10n/app_localizations.dart';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_theme.dart';
import '../../../task_create/domain/session_type.dart';
import '../../application/terminal_shortcut_controller.dart';
import '../../domain/terminal_shortcut.dart';
import 'terminal_shortcut_settings_page.dart';

class TerminalShortcutBar extends ConsumerStatefulWidget {
  const TerminalShortcutBar({
    required this.sessionType,
    required this.onShortcut,
    this.onCollapse,
    this.isBusy = false,
    super.key,
  });

  final SessionType sessionType;
  final ValueChanged<TerminalShortcut> onShortcut;
  final VoidCallback? onCollapse;
  final bool isBusy;

  @override
  ConsumerState<TerminalShortcutBar> createState() =>
      _TerminalShortcutBarState();
}

class _TerminalShortcutBarState extends ConsumerState<TerminalShortcutBar> {
  @override
  Widget build(BuildContext context) {
    final settings = ref.watch(terminalShortcutControllerProvider);
    final profile =
        settings.asData?.value[widget.sessionType] ??
        TerminalShortcutProfile.defaults();
    final visibleShortcuts = profile.visibleShortcuts(widget.sessionType);
    return Focus(
      canRequestFocus: false,
      descendantsAreFocusable: false,
      child: Container(
        decoration: BoxDecoration(
          color: AppColors.terminalChrome,
          border: Border(top: BorderSide(color: AppColors.borderDefault)),
        ),
        child: Row(
          children: [
            // 바가 꺼졌을 때 옆의 상태 문구와 한 노드로 합쳐져 함께 읽히지 않도록 경계를 둔다
            Semantics(
              container: true,
              child: Padding(
                padding: EdgeInsets.symmetric(horizontal: 8),
                child: Text(
                  widget.sessionType == SessionType.terminal
                      ? '키'
                      : widget.sessionType.name,
                  style: TextStyle(
                    color: AppColors.textSecondary,
                    fontSize: 12,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ),
            Expanded(
              child: profile.enabled
                  ? SingleChildScrollView(
                      key: ValueKey(widget.sessionType),
                      scrollDirection: Axis.horizontal,
                      padding: EdgeInsets.symmetric(horizontal: 2, vertical: 2),
                      child: visibleShortcuts.isEmpty
                          ? TextButton(
                              onPressed: _openSettings,
                              style: TextButton.styleFrom(
                                minimumSize: Size(0, 32),
                                tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                                textStyle: TextStyle(fontSize: 12),
                              ),
                              child: Text(context.tr('단축키 추가')),
                            )
                          : Row(
                              children: [
                                for (final shortcut in visibleShortcuts)
                                  _key(shortcut),
                              ],
                            ),
                    )
                  : Padding(
                      padding: EdgeInsets.only(left: 12),
                      child: Text(
                        context.tr('{type} · 단축키 꺼짐', {
                          'type': widget.sessionType.name,
                        }),
                        style: TextStyle(color: AppColors.textSecondary),
                      ),
                    ),
            ),
            _control(
              'shortcut-settings',
              context.tr('{type} 단축키 설정', {'type': widget.sessionType.name}),
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
      ),
    );
  }

  void _openSettings() => Navigator.of(context).push(
    MaterialPageRoute<void>(
      fullscreenDialog: true,
      builder: (_) =>
          TerminalShortcutSettingsPage(sessionType: widget.sessionType),
    ),
  );

  Widget _control(
    String id,
    String tooltip,
    IconData icon,
    VoidCallback onPressed,
  ) => Semantics(
    identifier: id,
    child: IconButton(
      constraints: BoxConstraints.tightFor(width: 36, height: 36),
      padding: EdgeInsets.all(4),
      style: IconButton.styleFrom(
        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
        visualDensity: VisualDensity.standard,
      ),
      tooltip: context.tr(tooltip),
      onPressed: onPressed,
      icon: Icon(icon, size: 18),
    ),
  );

  Widget _key(TerminalShortcut shortcut) {
    return Semantics(
      identifier: 'shortcut-${shortcut.id}',
      child: Padding(
        padding: EdgeInsets.symmetric(horizontal: 2),
        child: Tooltip(
          message: shortcut.description,
          child: TextButton(
            // 세션 동작은 다른 pane 제어가 끝날 때까지 막고, 키 입력은 그대로 보낸다
            onPressed: widget.isBusy && shortcut.action != null
                ? null
                : () => widget.onShortcut(shortcut),
            style: TextButton.styleFrom(
              minimumSize: Size(40, 32),
              tapTargetSize: MaterialTapTargetSize.shrinkWrap,
              visualDensity: VisualDensity.standard,
              padding: EdgeInsets.symmetric(horizontal: 8),
              foregroundColor: AppColors.textPrimary,
              backgroundColor: AppColors.buttonNeutral,
              shape: RoundedRectangleBorder(
                borderRadius: BorderRadius.circular(6),
                side: BorderSide(color: AppColors.borderStrong),
              ),
              textStyle: TextStyle(fontFamily: monoFontFamily, fontSize: 12),
            ),
            child: Text(shortcut.label),
          ),
        ),
      ),
    );
  }
}
