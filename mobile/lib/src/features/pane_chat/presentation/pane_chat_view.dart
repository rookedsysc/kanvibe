import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/constants/app_theme.dart';
import '../../chat_history/presentation/chat_message_tile.dart';
import '../application/pane_chat_controller.dart';
import '../domain/pane_chat.dart';

class PaneChatView extends ConsumerStatefulWidget {
  const PaneChatView({
    required this.target,
    required this.onReturnToTerminal,
    this.paneLabel,
    this.isActive = true,
    super.key,
  });
  final PaneChatTarget target;
  final String? paneLabel;
  final VoidCallback onReturnToTerminal;
  final bool isActive;

  @override
  ConsumerState<PaneChatView> createState() => _PaneChatViewState();
}

class _PaneChatViewState extends ConsumerState<PaneChatView> {
  final _scroll = ScrollController();
  final _drafts = <String, String>{};
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
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final provider = paneChatProvider(widget.target);
    ref.listen(provider, (previous, next) {
      if (!_scroll.hasClients ||
          previous?.value?.snapshot == next.value?.snapshot) {
        return;
      }
      final offset = _scroll.offset;
      final extent = _scroll.position.maxScrollExtent;
      if (offset <= AppSizes.minTouchTarget) return;
      // Reverse lists grow beneath the reader when a live response arrives.
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted || !_scroll.hasClients || _scroll.offset != offset) return;
        _scroll.jumpTo(
          (offset + _scroll.position.maxScrollExtent - extent).clamp(
            0.0,
            _scroll.position.maxScrollExtent,
          ),
        );
      });
    });
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
        final occurrences = <(String, DateTime?, String), int>{};
        final messageKeys = <LocalKey>[
          for (final message in snapshot.transcript.messages)
            ValueKey((
              message.role,
              message.timestamp,
              message.text,
              occurrences.update(
                (message.role, message.timestamp, message.text),
                (count) => count + 1,
                ifAbsent: () => 0,
              ),
            )),
        ];
        final messageIndexes = {
          for (var index = 0; index < messageKeys.length; index++)
            messageKeys[index]: index,
        };
        return LayoutBuilder(
          builder: (context, constraints) {
            final compact = constraints.maxHeight < 360;
            final body = Column(
              mainAxisSize: compact ? MainAxisSize.min : MainAxisSize.max,
              children: [
                Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: AppSizes.p16,
                    vertical: AppSizes.p4,
                  ),
                  child: Row(
                    children: [
                      Expanded(
                        child: Tooltip(
                          message: widget.target.paneId,
                          child: Text(
                            widget.paneLabel == null
                                ? '${snapshot.provider ?? '현재 pane'} · ${widget.target.paneId.length > 16 ? '${widget.target.paneId.substring(0, 8)}…${widget.target.paneId.substring(widget.target.paneId.length - 4)}' : widget.target.paneId}'
                                : '${widget.paneLabel} · ${snapshot.provider ?? '현재 pane'}',
                            semanticsLabel:
                                '${snapshot.provider ?? '현재 pane'} · ${widget.target.paneId}',
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: Theme.of(context).textTheme.bodySmall,
                          ),
                        ),
                      ),
                      IconButton(
                        tooltip: '현재 CLI 다시 확인',
                        onPressed: state.isActing || state.isRefreshing
                            ? null
                            : notifier.refresh,
                        icon: const Icon(Icons.refresh, size: AppSizes.p24),
                      ),
                    ],
                  ),
                ),
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
                        : snapshot.transcript.messages.isEmpty
                        ? const Center(child: Text('현재 CLI에 첫 메시지를 보내세요.'))
                        : Align(
                            alignment: Alignment.topCenter,
                            child: ConstrainedBox(
                              constraints: const BoxConstraints(
                                maxWidth: AppSizes.tabletBreakpoint,
                              ),
                              child: ListView.builder(
                                key: PageStorageKey(
                                  'pane-chat-transcript:$draftKey',
                                ),
                                findChildIndexCallback: (key) =>
                                    messageIndexes[key],
                                controller: _scroll,
                                reverse: true,
                                padding: const EdgeInsets.all(AppSizes.p16),
                                itemCount:
                                    snapshot.transcript.messages.length +
                                    (snapshot.transcript.nextCursor != null
                                        ? 1
                                        : 0),
                                itemBuilder: (context, index) {
                                  if (index ==
                                      snapshot.transcript.messages.length) {
                                    return const Padding(
                                      padding: EdgeInsets.all(AppSizes.p16),
                                      child: Text(
                                        '최근 대화를 표시합니다. 이전 대화는 채팅 History에서 볼 수 있습니다.',
                                      ),
                                    );
                                  }
                                  final message =
                                      snapshot.transcript.messages[index];
                                  return ChatMessageTile(
                                    key: messageKeys[index],
                                    message: message,
                                    provider: snapshot.provider ?? 'AI',
                                  );
                                },
                              ),
                            ),
                          ),
                  ),
                if (snapshot.provider != null)
                  _PaneChatComposer(
                    key: ValueKey(draftKey),
                    target: widget.target,
                    initialDraft: _drafts[draftKey] ?? '',
                    onDraftChanged: (text) => _drafts[draftKey] = text,
                    showPrompt: snapshot.inputState == PaneChatInputState.ready,
                    toolbar: Wrap(
                      crossAxisAlignment: WrapCrossAlignment.center,
                      children: [
                        Semantics(
                          identifier: 'pane-chat-model',
                          child: TextButton.icon(
                            style: TextButton.styleFrom(
                              minimumSize: const Size(
                                AppSizes.minTouchTarget,
                                AppSizes.minTouchTarget,
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
                            icon: const Icon(Icons.tune, size: AppSizes.p16),
                            label: const Text('모델'),
                          ),
                        ),
                        TextButton.icon(
                          style: TextButton.styleFrom(
                            minimumSize: const Size(
                              AppSizes.minTouchTarget,
                              AppSizes.minTouchTarget,
                            ),
                          ),
                          onPressed:
                              snapshot.transcript.available &&
                                  snapshot.inputState ==
                                      PaneChatInputState.ready
                              ? () => setState(() => _showCli = !_showCli)
                              : null,
                          icon: Icon(
                            showScreen
                                ? Icons.chat_bubble_outline
                                : Icons.terminal,
                            size: AppSizes.p16,
                          ),
                          label: Text(
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

class _PaneChatComposer extends ConsumerStatefulWidget {
  const _PaneChatComposer({
    required this.target,
    required this.initialDraft,
    required this.onDraftChanged,
    required this.toolbar,
    required this.showPrompt,
    this.cliScreen,
    super.key,
  });
  final PaneChatTarget target;
  final String initialDraft;
  final ValueChanged<String> onDraftChanged;
  final Widget toolbar;
  final Widget? cliScreen;
  final bool showPrompt;

  @override
  ConsumerState<_PaneChatComposer> createState() => _PaneChatComposerState();
}

class _PaneChatComposerState extends ConsumerState<_PaneChatComposer> {
  late final _input = TextEditingController(text: widget.initialDraft);

  @override
  void dispose() {
    _input.dispose();
    super.dispose();
  }

  Future<void> _send() async {
    final text = _input.text;
    if (!await ref.read(paneChatProvider(widget.target).notifier).send(text)) {
      return;
    }
    if (!mounted || _input.text != text) return;
    _input.clear();
    widget.onDraftChanged('');
    setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(paneChatProvider(widget.target)).value;
    return Container(
      margin: const EdgeInsets.fromLTRB(
        AppSizes.p12,
        AppSizes.p4,
        AppSizes.p12,
        AppSizes.p12,
      ),
      padding: const EdgeInsets.all(AppSizes.p8),
      decoration: BoxDecoration(
        color: AppColors.bgSurface,
        border: Border.all(color: AppColors.borderStrong),
        borderRadius: BorderRadius.circular(AppSizes.radiusCard),
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (widget.cliScreen != null) widget.cliScreen!,
          if (widget.showPrompt)
            Semantics(
              identifier: 'pane-chat-input',
              child: TextField(
                controller: _input,
                minLines: 1,
                maxLines: 4,
                keyboardType: TextInputType.multiline,
                textInputAction: TextInputAction.newline,
                onChanged: (text) {
                  widget.onDraftChanged(text);
                  setState(() {});
                },
                decoration: const InputDecoration(
                  labelText: '현재 CLI에 메시지',
                  hintText: '메시지를 입력하세요',
                  border: InputBorder.none,
                  enabledBorder: InputBorder.none,
                  focusedBorder: InputBorder.none,
                ),
              ),
            ),
          Row(
            children: [
              Expanded(child: widget.toolbar),
              if (widget.showPrompt)
                Semantics(
                  identifier: 'pane-chat-send',
                  child: IconButton.filled(
                    tooltip: '메시지 전송',
                    onPressed:
                        state?.canPrompt == true &&
                            _input.text.trim().isNotEmpty
                        ? _send
                        : null,
                    icon: const Icon(Icons.arrow_upward),
                    constraints: const BoxConstraints(
                      minWidth: AppSizes.minTouchTarget,
                      minHeight: AppSizes.minTouchTarget,
                    ),
                  ),
                ),
            ],
          ),
        ],
      ),
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
