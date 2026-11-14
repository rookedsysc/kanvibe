import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../chat_history/domain/chat_history.dart';
import '../../chat_history/presentation/chat_message_tile.dart';
import '../application/pane_chat_controller.dart';
import '../domain/pane_chat.dart';

/// 현재 CLI의 최근 대화. 최신 메시지가 아래에 오고, 읽던 위치는 새 응답이 와도 움직이지 않는다
class PaneChatTranscriptList extends ConsumerStatefulWidget {
  const PaneChatTranscriptList({
    required this.target,
    required this.snapshot,
    required this.storageKey,
    super.key,
  });

  final PaneChatTarget target;
  final PaneChatSnapshot snapshot;

  /// 같은 CLI의 스크롤 위치만 이어 가도록 연결과 대상 토큰으로 만든 키
  final String storageKey;

  @override
  ConsumerState<PaneChatTranscriptList> createState() =>
      _PaneChatTranscriptListState();
}

class _PaneChatTranscriptListState
    extends ConsumerState<PaneChatTranscriptList> {
  /// 떠 있는 채팅 버튼이 가장 오래된 메시지를 가리지 않도록 비워 두는 높이
  static const _floatingToggleInset = AppSizes.minTouchTarget + AppSizes.p24;

  final _scroll = ScrollController();

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  /// 최신 메시지에서 한 화면 넘게 올라가 있는지.
  ///
  /// CLI 대상이 바뀌는 프레임에는 이전 목록과 새 목록이 함께 붙어 있고, 새 목록은 아직 배치 전이라
  /// 그동안에는 판단하지 않는다.
  bool get _isAwayFromLatest =>
      _scroll.positions.length == 1 &&
      _scroll.position.hasViewportDimension &&
      _scroll.offset > _scroll.position.viewportDimension;

  void _jumpToLatest() {
    if (MediaQuery.disableAnimationsOf(context)) {
      _scroll.jumpTo(0);
    } else {
      _scroll.animateTo(0, duration: Durations.short4, curve: Curves.easeOut);
    }
  }

  void _keepReadingPosition() {
    if (!_scroll.hasClients) return;
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
  }

  @override
  void didUpdateWidget(PaneChatTranscriptList oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.snapshot != widget.snapshot) _keepReadingPosition();
  }

  @override
  Widget build(BuildContext context) {
    final snapshot = widget.snapshot;
    final messages = snapshot.transcript.messages;
    final pendingPrompt = ref.watch(
      paneChatPendingPromptProvider(widget.target),
    );
    if (messages.isEmpty && pendingPrompt == null) {
      return const Center(child: Text('현재 CLI에 첫 메시지를 보내세요.'));
    }
    final entries = _buildEntries(messages, pendingPrompt);
    final entryIndexes = {
      for (var index = 0; index < entries.length; index++)
        entries[index].key: index,
    };
    final provider = snapshot.provider ?? 'AI';
    return Stack(
      children: [
        Align(
          alignment: Alignment.topCenter,
          child: ConstrainedBox(
            constraints: const BoxConstraints(
              maxWidth: AppSizes.tabletBreakpoint,
            ),
            child: ListView.builder(
              key: PageStorageKey('pane-chat-transcript:${widget.storageKey}'),
              findChildIndexCallback: (key) => entryIndexes[key],
              controller: _scroll,
              reverse: true,
              padding: const EdgeInsets.fromLTRB(
                AppSizes.p16,
                _floatingToggleInset,
                AppSizes.p16,
                AppSizes.p16,
              ),
              itemCount:
                  entries.length +
                  (snapshot.transcript.nextCursor != null ? 1 : 0),
              itemBuilder: (context, index) {
                if (index == entries.length) {
                  return const Padding(
                    padding: EdgeInsets.all(AppSizes.p16),
                    child: Text('최근 대화를 표시합니다. 이전 대화는 채팅 History에서 볼 수 있습니다.'),
                  );
                }
                final entry = entries[index];
                if (entry.isPending) {
                  return Semantics(
                    key: entry.key,
                    identifier: 'pane-chat-pending',
                    child: Opacity(
                      opacity: 0.6,
                      child: ChatMessageTile(
                        message: entry.messages.single,
                        provider: provider,
                      ),
                    ),
                  );
                }
                if (entry.messages.length > 1) {
                  return _ToolRunCard(
                    key: entry.key,
                    messages: entry.messages,
                    messageKeys: entry.messageKeys,
                    provider: provider,
                    taskId: widget.target.taskId,
                    sessionId: snapshot.transcript.sessionId,
                  );
                }
                return ChatMessageTile(
                  key: entry.key,
                  message: entry.messages.single,
                  provider: provider,
                  taskId: widget.target.taskId,
                  sessionId: snapshot.transcript.sessionId,
                );
              },
            ),
          ),
        ),
        Positioned(
          bottom: AppSizes.p16,
          right: AppSizes.p16,
          // 스크롤할 때마다 목록 전체를 다시 그리지 않도록 버튼만 스크롤을 듣는다
          child: ListenableBuilder(
            listenable: _scroll,
            builder: (context, _) => _isAwayFromLatest
                ? IconButton.filledTonal(
                    tooltip: '최신 메시지로 이동',
                    style: IconButton.styleFrom(
                      backgroundColor: context.colors.bgSurface,
                      foregroundColor: context.colors.textPrimary,
                    ),
                    onPressed: _jumpToLatest,
                    icon: const Icon(Icons.arrow_downward),
                  )
                : const SizedBox.shrink(),
          ),
        ),
      ],
    );
  }

  /// 최신순 메시지를 목록 항목으로 바꾼다. 연달아 나온 도구 실행과 추론은 한 묶음으로 접는다
  List<_TranscriptEntry> _buildEntries(
    List<ChatMessage> messages,
    String? pendingPrompt,
  ) {
    final occurrences = <(String, DateTime?, String), int>{};
    final messageKeys = <LocalKey>[
      for (final message in messages)
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
    final entries = <_TranscriptEntry>[
      if (pendingPrompt != null)
        _TranscriptEntry(
          key: const ValueKey('pane-chat-pending'),
          messages: [ChatMessage(role: 'user', text: pendingPrompt)],
          messageKeys: const [],
          isPending: true,
        ),
    ];
    var start = 0;
    while (start < messages.length) {
      var end = start + 1;
      if (_isToolRunMessage(messages[start])) {
        while (end < messages.length && _isToolRunMessage(messages[end])) {
          end++;
        }
      }
      final isRun = end - start > 1;
      entries.add(
        _TranscriptEntry(
          // 묶음에 새 실행이 덧붙어도 펼친 상태가 유지되도록 가장 오래된 메시지의 키를 쓴다
          key: isRun
              ? ValueKey(('pane-chat-tool-run', messageKeys[end - 1]))
              : messageKeys[start],
          messages: messages.sublist(start, end),
          messageKeys: messageKeys.sublist(start, end),
        ),
      );
      start = end;
    }
    return entries;
  }

  bool _isToolRunMessage(ChatMessage message) =>
      message.role == 'tool' || message.role == 'reasoning';
}

class _TranscriptEntry {
  const _TranscriptEntry({
    required this.key,
    required this.messages,
    required this.messageKeys,
    this.isPending = false,
  });

  final LocalKey key;

  /// 최신순. 둘 이상이면 접어서 보여 주는 도구 실행 묶음이다
  final List<ChatMessage> messages;
  final List<LocalKey> messageKeys;
  final bool isPending;
}

/// 연달아 실행한 도구와 추론을 한 줄로 접어 답변 본문이 먼저 읽히게 한다
class _ToolRunCard extends StatefulWidget {
  const _ToolRunCard({
    required this.messages,
    required this.messageKeys,
    required this.provider,
    required this.taskId,
    required this.sessionId,
    super.key,
  });

  final List<ChatMessage> messages;
  final List<LocalKey> messageKeys;
  final String provider;
  final String taskId;
  final String? sessionId;

  @override
  State<_ToolRunCard> createState() => _ToolRunCardState();
}

class _ToolRunCardState extends State<_ToolRunCard> {
  bool _expanded = false;

  @override
  Widget build(BuildContext context) {
    final toolCount = widget.messages
        .where((message) => message.role == 'tool')
        .length;
    final reasoningCount = widget.messages.length - toolCount;
    final summary = [
      if (toolCount > 0) '도구 실행 $toolCount개',
      if (reasoningCount > 0) '추론 $reasoningCount개',
    ].join(' · ');
    return Container(
      margin: const EdgeInsets.only(bottom: AppSizes.p16),
      decoration: BoxDecoration(
        color: context.colors.bgSurface,
        border: Border.all(color: context.colors.borderDefault),
        borderRadius: BorderRadius.circular(AppSizes.radiusLg),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Semantics(
            expanded: _expanded,
            child: InkWell(
              onTap: () => setState(() => _expanded = !_expanded),
              child: ConstrainedBox(
                constraints: const BoxConstraints(
                  minHeight: AppSizes.minTouchTarget,
                ),
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: AppSizes.p12),
                  child: Row(
                    children: [
                      const Icon(
                        Icons.check,
                        size: AppSizes.p16,
                        color: AppColors.statusDone,
                      ),
                      const SizedBox(width: AppSizes.p8),
                      Expanded(
                        child: Text(
                          summary,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.bodySmall
                              ?.copyWith(color: context.colors.textSecondary),
                        ),
                      ),
                      Icon(
                        _expanded ? Icons.expand_more : Icons.chevron_right,
                        size: AppSizes.p16,
                        color: context.colors.textSecondary,
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ),
          if (_expanded)
            // 실행한 순서대로 읽히도록 오래된 것부터 놓는다
            for (var index = widget.messages.length - 1; index >= 0; index--)
              ChatMessageTile(
                key: widget.messageKeys[index],
                message: widget.messages[index],
                provider: widget.provider,
                taskId: widget.taskId,
                sessionId: widget.sessionId,
              ),
        ],
      ),
    );
  }
}
