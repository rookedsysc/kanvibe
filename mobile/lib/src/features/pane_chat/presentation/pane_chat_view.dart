import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/constants/app_theme.dart';
import '../application/pane_chat_controller.dart';
import '../domain/pane_chat.dart';
import 'pane_chat_composer.dart';
import 'pane_chat_transcript_list.dart';

class PaneChatView extends ConsumerStatefulWidget {
  const PaneChatView({
    required this.target,
    required this.onReturnToTerminal,
    this.isActive = true,
    super.key,
  });
  final PaneChatTarget target;
  final VoidCallback onReturnToTerminal;
  final bool isActive;

  @override
  ConsumerState<PaneChatView> createState() => _PaneChatViewState();
}

class _PaneChatViewState extends ConsumerState<PaneChatView> {
  bool _showCli = false;

  @override
  void initState() {
    super.initState();
    _synchronizeActivity();
  }

  @override
  void didUpdateWidget(PaneChatView oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.isActive != widget.isActive) _synchronizeActivity();
  }

  void _synchronizeActivity() {
    final active = widget.isActive;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && widget.isActive == active) {
        ref.read(paneChatProvider(widget.target).notifier).setActive(active);
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final provider = paneChatProvider(widget.target);
    final chat = ref.watch(provider);
    return chat.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (_, _) => Center(
        child: Padding(
          padding: const EdgeInsets.all(AppSizes.p24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Text('현재 pane의 CLI를 확인하지 못했습니다.'),
              TextButton(
                onPressed: () => ref.invalidate(provider),
                child: const Text('다시 확인'),
              ),
              TextButton(
                onPressed: widget.onReturnToTerminal,
                child: const Text('터미널로 돌아가기'),
              ),
            ],
          ),
        ),
      ),
      data: (state) {
        final snapshot = state.snapshot;
        final notifier = ref.read(provider.notifier);
        final showScreen =
            _showCli ||
            !snapshot.transcript.available ||
            snapshot.inputState != PaneChatInputState.ready;
        final draftKey = '${state.connectionId}:${snapshot.targetToken}';
        return LayoutBuilder(
          builder: (context, constraints) {
            final compact = constraints.maxHeight < 360;
            final body = Column(
              mainAxisSize: compact ? MainAxisSize.min : MainAxisSize.max,
              children: [
                if (state.readError != null)
                  _PaneChatNotice(
                    text: state.readError!,
                    action: '다시 확인',
                    onAction: state.isRefreshing ? null : notifier.refresh,
                  ),
                if (state.actionError != null)
                  _PaneChatNotice(
                    text: state.actionError!,
                    action: state.canRetryAction ? '다시 시도' : null,
                    onAction: state.canControl ? notifier.retryAction : null,
                  ),
                if (!snapshot.canSend)
                  _PaneChatNotice(
                    text: switch (snapshot.reason) {
                      'no-foreground-ai-cli' => '이 pane에서 실행 중인 AI CLI가 없습니다. 터미널에서 Claude 또는 Codex를 실행하세요.',
                      'process-inspection-unavailable' =>
                        '실행 중인 CLI를 확인할 수 없습니다. 다시 확인해 주세요.',
                      _ => '현재 pane에 연결할 수 없습니다. 터미널 상태를 확인해 주세요.',
                    },
                    action: '터미널로 돌아가기',
                    onAction: widget.onReturnToTerminal,
                  )
                else if (snapshot.inputState != PaneChatInputState.ready)
                  _PaneChatNotice(
                    text: snapshot.inputState == PaneChatInputState.occupied
                        ? 'CLI 입력 중 · 아래 방향키와 확인 버튼으로 응답하세요.'
                        : 'CLI 입력 대기 상태를 확인하고 있습니다. 현재 화면에서 상태를 확인하세요.',
                  ),
                if (!compact)
                  Expanded(
                    child: !snapshot.transcript.available
                        ? const Center(
                            child: _PaneChatNotice(
                              text: '현재 CLI의 대화 기록을 아직 연결하지 못했습니다. 현재 CLI 화면을 표시합니다.',
                            ),
                          )
                        : PaneChatTranscriptList(
                            target: widget.target,
                            snapshot: snapshot,
                            storageKey: draftKey,
                          ),
                  ),
                if (snapshot.provider != null && snapshot.targetToken != null)
                  PaneChatComposer(
                    key: ValueKey(draftKey),
                    target: (
                      connectionId: state.connectionId,
                      taskId: widget.target.taskId,
                      paneId: widget.target.paneId,
                      targetToken: snapshot.targetToken!,
                    ),
                    isActive: widget.isActive,
                    showPrompt: snapshot.inputState == PaneChatInputState.ready,
                    toolbar: Wrap(
                      crossAxisAlignment: WrapCrossAlignment.center,
                      children: [
                        Semantics(
                          identifier: 'pane-chat-model',
                          // 파일 참조 버튼과 한 줄에 놓이도록 아이콘 없이 글자만 둔다
                          child: TextButton(
                            style: TextButton.styleFrom(
                              minimumSize: const Size(
                                AppSizes.minTouchTarget,
                                AppSizes.minTouchTarget,
                              ),
                              padding: const EdgeInsets.symmetric(
                                horizontal: AppSizes.p8,
                              ),
                            ),
                            onPressed:
                                state.canPrompt && snapshot.modelPickerSupported
                                ? () async {
                                    if (await notifier.openModelPicker() &&
                                        mounted) {
                                      setState(() => _showCli = true);
                                    }
                                  }
                                : null,
                            child: const Text('모델'),
                          ),
                        ),
                        TextButton(
                          style: TextButton.styleFrom(
                            minimumSize: const Size(
                              AppSizes.minTouchTarget,
                              AppSizes.minTouchTarget,
                            ),
                            padding: const EdgeInsets.symmetric(
                              horizontal: AppSizes.p8,
                            ),
                          ),
                          onPressed:
                              snapshot.transcript.available &&
                                  snapshot.inputState ==
                                      PaneChatInputState.ready
                              ? () => setState(() => _showCli = !_showCli)
                              : null,
                          child: Text(
                            !snapshot.transcript.available ||
                                    snapshot.inputState !=
                                        PaneChatInputState.ready
                                ? '현재 CLI 화면'
                                : showScreen
                                ? '대화 보기'
                                : 'CLI',
                          ),
                        ),
                        Semantics(
                          identifier: 'pane-chat-interrupt',
                          child: IconButton(
                            tooltip: '중단',
                            onPressed: state.canControl
                                ? notifier.interrupt
                                : null,
                            icon: const Icon(
                              Icons.stop_circle_outlined,
                              size: AppSizes.p24,
                            ),
                            constraints: const BoxConstraints(
                              minWidth: AppSizes.minTouchTarget,
                              minHeight: AppSizes.minTouchTarget,
                            ),
                          ),
                        ),
                      ],
                    ),
                    cliScreen: showScreen
                        ? Column(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              Container(
                                width: double.infinity,
                                padding: const EdgeInsets.all(AppSizes.p12),
                                decoration: BoxDecoration(
                                  color: AppColors.terminalBg,
                                  borderRadius: BorderRadius.circular(
                                    AppSizes.radiusLg,
                                  ),
                                ),
                                child: ConstrainedBox(
                                  constraints: BoxConstraints(
                                    maxHeight: constraints.maxHeight * 0.2,
                                  ),
                                  child: SingleChildScrollView(
                                    child: Semantics(
                                      identifier: 'pane-chat-screen',
                                      child: SelectableText(
                                        snapshot.screen.isEmpty
                                            ? '현재 CLI 화면이 비어 있습니다.'
                                            : snapshot.screen,
                                        style: monoStyle(),
                                      ),
                                    ),
                                  ),
                                ),
                              ),
                              Wrap(
                                alignment: WrapAlignment.center,
                                children: [
                                  for (final control in const [
                                    (PaneChatKey.up, Icons.arrow_upward, '위'),
                                    (
                                      PaneChatKey.down,
                                      Icons.arrow_downward,
                                      '아래',
                                    ),
                                    (PaneChatKey.left, Icons.arrow_back, '왼쪽'),
                                    (
                                      PaneChatKey.right,
                                      Icons.arrow_forward,
                                      '오른쪽',
                                    ),
                                    (
                                      PaneChatKey.enter,
                                      Icons.keyboard_return,
                                      '확인',
                                    ),
                                    (PaneChatKey.escape, Icons.close, '취소'),
                                  ])
                                    Semantics(
                                      identifier:
                                          'pane-chat-${control.$1.name}',
                                      child: IconButton(
                                        tooltip: control.$3,
                                        onPressed: state.canControl
                                            ? () => notifier.sendKey(control.$1)
                                            : null,
                                        icon: Icon(
                                          control.$2,
                                          size: AppSizes.p24,
                                        ),
                                        constraints: const BoxConstraints(
                                          minWidth: AppSizes.minTouchTarget,
                                          minHeight: AppSizes.minTouchTarget,
                                        ),
                                      ),
                                    ),
                                ],
                              ),
                            ],
                          )
                        : null,
                  ),
              ],
            );
            return SingleChildScrollView(
              reverse: true,
              child: SizedBox(
                height: compact ? null : constraints.maxHeight,
                child: body,
              ),
            );
          },
        );
      },
    );
  }
}

class _PaneChatNotice extends StatelessWidget {
  const _PaneChatNotice({required this.text, this.action, this.onAction});
  final String text;
  final String? action;
  final VoidCallback? onAction;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.symmetric(
      horizontal: AppSizes.p16,
      vertical: AppSizes.p8,
    ),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(text, style: Theme.of(context).textTheme.bodySmall),
        if (action != null)
          TextButton(onPressed: onAction, child: Text(action!)),
      ],
    ),
  );
}
