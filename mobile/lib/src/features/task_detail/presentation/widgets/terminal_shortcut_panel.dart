import 'package:flutter/material.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../task_create/domain/session_type.dart';
import '../../domain/terminal_shortcut.dart';

class TerminalShortcutPanel extends StatefulWidget {
  const TerminalShortcutPanel({
    required this.sessionType,
    required this.profile,
    required this.onShortcut,
    required this.isBusy,
    this.maxHeight,
    super.key,
  });

  final SessionType sessionType;
  final TerminalShortcutProfile profile;
  final ValueChanged<TerminalShortcut> onShortcut;
  final bool isBusy;
  final double? maxHeight;

  @override
  State<TerminalShortcutPanel> createState() => _TerminalShortcutPanelState();
}

class _TerminalShortcutPanelState extends State<TerminalShortcutPanel> {
  late String _group = widget.sessionType == SessionType.terminal ? '키' : '세션';

  @override
  Widget build(BuildContext context) {
    final sessionKeys = terminalShortcuts(widget.sessionType);
    final shortcuts = switch (_group) {
      '세션' => sessionKeys.where((key) => key.action != null).toList(),
      '내 콤보' => widget.profile.combos,
      _ => sessionKeys.where((key) => key.action == null).toList(),
    };
    final availableHeight =
        MediaQuery.sizeOf(context).height -
        MediaQuery.viewInsetsOf(context).bottom;
    return ConstrainedBox(
      constraints: BoxConstraints(
        maxHeight: widget.maxHeight ?? availableHeight * .4,
      ),
      child: SingleChildScrollView(
        child: Container(
          padding: const EdgeInsets.all(AppSizes.p8),
          decoration: const BoxDecoration(
            color: AppColors.bgSurface,
            border: Border(bottom: BorderSide(color: AppColors.borderDefault)),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            mainAxisSize: MainAxisSize.min,
            children: [
              SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: Row(
                  children: [
                    for (final group in [
                      if (widget.sessionType != SessionType.terminal) '세션',
                      '키',
                      '내 콤보',
                    ])
                      Padding(
                        padding: const EdgeInsets.only(right: AppSizes.p8),
                        child: ChoiceChip(
                          label: Text(
                            group == '세션' ? widget.sessionType.name : group,
                          ),
                          selected: _group == group,
                          selectedColor: AppColors.brandSubtle,
                          side: const BorderSide(color: AppColors.borderStrong),
                          labelStyle: const TextStyle(
                            color: AppColors.textPrimary,
                            fontSize: 12,
                          ),
                          visualDensity: VisualDensity.compact,
                          materialTapTargetSize:
                              MaterialTapTargetSize.shrinkWrap,
                          showCheckmark: false,
                          onSelected: (_) => setState(() => _group = group),
                        ),
                      ),
                  ],
                ),
              ),
              const SizedBox(height: AppSizes.p8),
              if (shortcuts.isEmpty)
                const Padding(
                  padding: EdgeInsets.symmetric(vertical: AppSizes.p8),
                  child: Text('아직 콤보가 없습니다. 단축키 설정의 + 버튼으로 추가하세요.'),
                )
              else
                SingleChildScrollView(
                  key: ValueKey(_group),
                  scrollDirection: Axis.horizontal,
                  child: Row(
                    children: [
                      for (final shortcut in shortcuts)
                        Padding(
                          padding: const EdgeInsets.only(right: AppSizes.p8),
                          child: Semantics(
                            identifier: 'panel-key-${shortcut.id}',
                            child: Tooltip(
                              message: shortcut.description,
                              child: OutlinedButton(
                                key: ValueKey('panel-key-${shortcut.id}'),
                                onPressed:
                                    widget.isBusy && shortcut.action != null
                                    ? null
                                    : () => widget.onShortcut(shortcut),
                                style: OutlinedButton.styleFrom(
                                  minimumSize: const Size(0, 36),
                                  tapTargetSize:
                                      MaterialTapTargetSize.shrinkWrap,
                                  foregroundColor: AppColors.textPrimary,
                                  disabledForegroundColor: AppColors.textMuted,
                                  padding: const EdgeInsets.symmetric(
                                    horizontal: AppSizes.p12,
                                    vertical: AppSizes.p8,
                                  ),
                                  textStyle: Theme.of(context)
                                      .textTheme
                                      .labelLarge
                                      ?.copyWith(fontSize: 12),
                                  backgroundColor: AppColors.buttonNeutral,
                                  shape: RoundedRectangleBorder(
                                    borderRadius: BorderRadius.circular(
                                      AppSizes.radiusMd,
                                    ),
                                  ),
                                ),
                                child: Text(shortcut.label),
                              ),
                            ),
                          ),
                        ),
                    ],
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}
