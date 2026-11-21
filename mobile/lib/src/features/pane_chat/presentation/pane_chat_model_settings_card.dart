import 'package:flutter/material.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../domain/pane_chat.dart';

/// 현재 모델의 effort를 슬라이더로 고르고, 모델 이름을 눌러 다른 모델로 바꾼다.
///
/// 고를 수 없는 것은 숨긴다. effort 단계가 없는 모델에는 슬라이더와 되돌리기가 없고,
/// 빠른 모드를 지원하지 않는 모델에는 번개 버튼이 없다.
class PaneChatModelSettingsCard extends StatefulWidget {
  const PaneChatModelSettingsCard({
    required this.settings,
    required this.isEnabled,
    required this.onChange,
    super.key,
  });

  final PaneChatModelSettings settings;

  /// CLI가 입력을 받을 수 있을 때만 바꿀 수 있다
  final bool isEnabled;

  /// 변경 한 건을 CLI에 보낸다. 보내지 못했으면 false다
  final Future<bool> Function(Map<String, Object?> change) onChange;

  @override
  State<PaneChatModelSettingsCard> createState() =>
      _PaneChatModelSettingsCardState();
}

class _PaneChatModelSettingsCardState extends State<PaneChatModelSettingsCard> {
  bool _showsModels = false;

  /// 방금 고른 값. CLI 기록이 따라올 때까지 화면이 예전 값으로 되돌아가 보이지 않게 한다
  String? _chosenEffort;
  bool? _chosenFast;

  @override
  void didUpdateWidget(PaneChatModelSettingsCard oldWidget) {
    super.didUpdateWidget(oldWidget);
    final before = oldWidget.settings;
    final after = widget.settings;
    if (before.model != after.model) {
      _chosenEffort = null;
      _chosenFast = null;
      return;
    }
    if (before.effort != after.effort) _chosenEffort = null;
    if (before.isFast != after.isFast) _chosenFast = null;
  }

  Future<void> _change(
    Map<String, Object?> change, {
    required VoidCallback onRejected,
  }) async {
    final accepted = await widget.onChange(change);
    if (!accepted && mounted) setState(onRejected);
  }

  void _chooseEffort(String effort) {
    setState(() => _chosenEffort = effort);
    _change({
      'kind': 'effort',
      'effort': effort,
    }, onRejected: () => _chosenEffort = null);
  }

  void _toggleFast(bool isFast) {
    setState(() => _chosenFast = isFast);
    _change({
      'kind': 'fast',
      'isFast': isFast,
    }, onRejected: () => _chosenFast = null);
  }

  void _reset() {
    setState(() => _chosenEffort = null);
    _change(const {'kind': 'reset'}, onRejected: () {});
  }

  void _chooseModel(String model) {
    setState(() => _showsModels = false);
    if (model == widget.settings.model) return;
    _change({'kind': 'model', 'model': model}, onRejected: () {});
  }

  @override
  Widget build(BuildContext context) {
    final settings = widget.settings;
    final model = settings.currentModel;
    if (model == null) return const SizedBox.shrink();
    final levels = model.effortLevels;
    final effort = _chosenEffort ?? settings.effort;
    final effortIndex = levels.indexWhere((level) => level.id == effort);
    final isFast = _chosenFast ?? settings.isFast ?? false;
    final textTheme = Theme.of(context).textTheme;
    return Semantics(
      identifier: 'pane-chat-model-settings',
      child: Padding(
        padding: const EdgeInsets.only(bottom: AppSizes.p8),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Row(
              children: [
                // 가운데 제목이 좌우 버튼 유무와 상관없이 가운데에 오도록 빈자리도 같은 폭을 차지한다
                SizedBox.square(
                  dimension: AppSizes.minTouchTarget,
                  child: model.supportsFast
                      ? Semantics(
                          identifier: 'pane-chat-fast-mode',
                          toggled: isFast,
                          child: IconButton(
                            tooltip: isFast ? '빠른 모드 끄기' : '빠른 모드 켜기',
                            onPressed: widget.isEnabled
                                ? () => _toggleFast(!isFast)
                                : null,
                            color: isFast
                                ? AppColors.brandPrimary
                                : context.colors.textSecondary,
                            icon: Icon(
                              isFast ? Icons.bolt : Icons.bolt_outlined,
                            ),
                          ),
                        )
                      : null,
                ),
                Expanded(
                  child: Semantics(
                    identifier: 'pane-chat-model-name',
                    button: true,
                    child: InkWell(
                      borderRadius: BorderRadius.circular(AppSizes.radiusLg),
                      onTap: widget.isEnabled
                          ? () => setState(() => _showsModels = !_showsModels)
                          : null,
                      child: Padding(
                        padding: const EdgeInsets.symmetric(
                          vertical: AppSizes.p4,
                        ),
                        child: Column(
                          children: [
                            if (effortIndex >= 0)
                              Text(
                                levels[effortIndex].label,
                                style: textTheme.titleMedium,
                              ),
                            Row(
                              mainAxisAlignment: MainAxisAlignment.center,
                              children: [
                                Flexible(
                                  child: Text(
                                    model.label,
                                    overflow: TextOverflow.ellipsis,
                                    style: textTheme.bodySmall?.copyWith(
                                      color: context.colors.textSecondary,
                                    ),
                                  ),
                                ),
                                Icon(
                                  _showsModels
                                      ? Icons.expand_more
                                      : Icons.chevron_right,
                                  size: AppSizes.p16,
                                  color: context.colors.textSecondary,
                                ),
                              ],
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
                SizedBox.square(
                  dimension: AppSizes.minTouchTarget,
                  child: levels.isNotEmpty
                      ? Semantics(
                          identifier: 'pane-chat-effort-reset',
                          child: IconButton(
                            tooltip: 'effort 기본값으로 되돌리기',
                            onPressed: widget.isEnabled ? _reset : null,
                            color: context.colors.textSecondary,
                            icon: const Icon(Icons.restart_alt),
                          ),
                        )
                      : null,
                ),
              ],
            ),
            if (levels.length > 1)
              Semantics(
                identifier: 'pane-chat-effort',
                child: Slider(
                  min: 0,
                  max: (levels.length - 1).toDouble(),
                  divisions: levels.length - 1,
                  // 현재 단계를 모르면 가장 낮은 칸에 두되, 고르기 전에는 명령을 보내지 않는다
                  value: (effortIndex < 0 ? 0 : effortIndex).toDouble(),
                  label: effortIndex < 0 ? null : levels[effortIndex].label,
                  onChanged: widget.isEnabled
                      ? (value) => setState(
                          () => _chosenEffort = levels[value.round()].id,
                        )
                      : null,
                  onChangeEnd: widget.isEnabled
                      ? (value) => _chooseEffort(levels[value.round()].id)
                      : null,
                ),
              ),
            if (_showsModels)
              // 입력창의 배경 상자 안에서도 눌림 효과가 보이도록 목록에 자기 Material을 준다
              Material(
                type: MaterialType.transparency,
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    for (final option in settings.models)
                      ListTile(
                        dense: true,
                        enabled: widget.isEnabled,
                        title: Text(option.label),
                        trailing: option.id == settings.model
                            ? const Icon(
                                Icons.check,
                                color: AppColors.brandPrimary,
                              )
                            : null,
                        onTap: () => _chooseModel(option.id),
                      ),
                  ],
                ),
              ),
          ],
        ),
      ),
    );
  }
}
