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
  static const _availableHeightFraction = .4;
  static const _keyGridHeightFraction = .25;

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
        maxHeight:
            widget.maxHeight ?? availableHeight * _availableHeightFraction,
      ),
      child: SingleChildScrollView(
        child: Container(
          padding: const EdgeInsets.all(AppSizes.p12),
          decoration: const BoxDecoration(
            color: AppColors.bgSurface,
            border: Border(bottom: BorderSide(color: AppColors.borderDefault)),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            mainAxisSize: MainAxisSize.min,
            children: [
              Wrap(
                spacing: AppSizes.p8,
                children: [
                  for (final group in [
                    if (widget.sessionType != SessionType.terminal) '세션',
                    '키',
                    '내 콤보',
                  ])
                    ChoiceChip(
                      label: Text(
                        group == '세션' ? widget.sessionType.name : group,
                      ),
                      selected: _group == group,
                      selectedColor: AppColors.brandSubtle,
                      side: const BorderSide(color: AppColors.borderStrong),
                      labelStyle: const TextStyle(color: AppColors.textPrimary),
                      showCheckmark: false,
                      onSelected: (_) => setState(() => _group = group),
                    ),
                ],
              ),
              Padding(
                padding: const EdgeInsets.symmetric(vertical: AppSizes.p8),
                child: Text(switch (_group) {
                  '세션' => '패널 제어',
                  '내 콤보' => '저장한 키 순서를 한 번에 실행',
                  _ => '터미널 키',
                }, style: Theme.of(context).textTheme.bodySmall),
              ),
              if (shortcuts.isEmpty)
                const Padding(
                  padding: EdgeInsets.symmetric(vertical: AppSizes.p16),
                  child: Text('아직 콤보가 없습니다. 단축키 설정의 + 버튼으로 추가하세요.'),
                )
              else
                ConstrainedBox(
                  constraints: BoxConstraints(
                    maxHeight:
                        MediaQuery.sizeOf(context).height *
                        _keyGridHeightFraction,
                  ),
                  child: SingleChildScrollView(
                    child: LayoutBuilder(
                      builder: (context, constraints) => Wrap(
                        spacing: AppSizes.p8,
                        runSpacing: AppSizes.p8,
                        children: [
                          for (final shortcut in shortcuts)
                            SizedBox(
                              width: (constraints.maxWidth - AppSizes.p8) / 2,
                              child: Semantics(
                                identifier: 'panel-key-${shortcut.id}',
                                child: OutlinedButton(
                                  key: ValueKey('panel-key-${shortcut.id}'),
                                  onPressed:
                                      widget.isBusy && shortcut.action != null
                                      ? null
                                      : () => widget.onShortcut(shortcut),
                                  style: OutlinedButton.styleFrom(
                                    minimumSize: const Size(
                                      0,
                                      AppSizes.minTouchTarget,
                                    ),
                                    foregroundColor: AppColors.textPrimary,
                                    disabledForegroundColor:
                                        AppColors.textMuted,
                                    alignment: Alignment.centerLeft,
                                    padding: const EdgeInsets.all(AppSizes.p12),
                                    backgroundColor: AppColors.buttonNeutral,
                                    shape: RoundedRectangleBorder(
                                      borderRadius: BorderRadius.circular(
                                        AppSizes.radiusLg,
                                      ),
                                    ),
                                  ),
                                  child: Column(
                                    crossAxisAlignment:
                                        CrossAxisAlignment.start,
                                    children: [
                                      Text(
                                        shortcut.label,
                                        style: const TextStyle(
                                          color: AppColors.textPrimary,
                                        ),
                                      ),
                                      if (_group != '키')
                                        Text(
                                          shortcut.description,
                                          style: Theme.of(context)
                                              .textTheme
                                              .bodySmall,
                                        ),
                                    ],
                                  ),
                                ),
                              ),
                            ),
                        ],
                      ),
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}
