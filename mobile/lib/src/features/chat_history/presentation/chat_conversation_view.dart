import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../application/chat_history_controller.dart';
import '../domain/chat_history.dart';
import 'chat_message_tile.dart';

class ChatConversationView extends ConsumerStatefulWidget {
  const ChatConversationView({
    required this.taskId,
    required this.session,
    super.key,
  });
  final String taskId;
  final ChatSession session;

  @override
  ConsumerState<ChatConversationView> createState() =>
      _ChatConversationViewState();
}

class _ChatConversationViewState extends ConsumerState<ChatConversationView> {
  final _scroll = ScrollController();
  bool _showLatest = false;

  @override
  void initState() {
    super.initState();
    _scroll.addListener(_onScroll);
  }

  void _onScroll() {
    final showLatest = _scroll.offset > _scroll.position.viewportDimension;
    if (showLatest != _showLatest) setState(() => _showLatest = showLatest);
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final provider = chatMessagesProvider((
      taskId: widget.taskId,
      session: widget.session,
    ));
    final messages = ref.watch(provider);
    return Column(
      children: [
        Expanded(
          child: messages.when(
            loading: () => const Center(child: CircularProgressIndicator()),
            error: (_, _) => Center(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const Text('대화를 불러오지 못했습니다.'),
                  TextButton(
                    onPressed: () => ref.invalidate(provider),
                    child: const Text('다시 시도'),
                  ),
                ],
              ),
            ),
            data: (state) => Stack(
              children: [
                Align(
                  alignment: Alignment.topCenter,
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(
                      maxWidth: AppSizes.tabletBreakpoint,
                    ),
                    child: ListView.builder(
                      key: PageStorageKey(
                        'chat-messages:${widget.taskId}:${widget.session.key}',
                      ),
                      controller: _scroll,
                      reverse: true,
                      padding: const EdgeInsets.all(AppSizes.p16),
                      itemCount: state.page.messages.length + 1,
                      itemBuilder: (context, index) {
                        if (index == state.page.messages.length) {
                          return state.page.nextCursor != null
                              ? TextButton(
                                  onPressed: state.isLoadingMore
                                      ? null
                                      : () => ref
                                            .read(provider.notifier)
                                            .loadMore(),
                                  child: Text(
                                    state.isLoadingMore
                                        ? '불러오는 중…'
                                        : state.loadMoreFailed
                                        ? '이전 메시지 다시 불러오기'
                                        : '이전 메시지 보기',
                                  ),
                                )
                              : state.page.messages.isEmpty
                              ? const Padding(
                                  padding: EdgeInsets.all(AppSizes.p32),
                                  child: Center(child: Text('표시할 메시지가 없습니다.')),
                                )
                              : const SizedBox.shrink();
                        }
                        return ChatMessageTile(
                          key: ValueKey('message:$index'),
                          message: state.page.messages[index],
                          provider: widget.session.provider,
                        );
                      },
                    ),
                  ),
                ),
                if (_showLatest)
                  Positioned(
                    bottom: AppSizes.p16,
                    right: AppSizes.p16,
                    child: IconButton.filledTonal(
                      tooltip: '최신 메시지로 이동',
                      style: IconButton.styleFrom(
                        backgroundColor: AppColors.bgSurface,
                        foregroundColor: AppColors.textPrimary,
                      ),
                      onPressed: () {
                        final reduceMotion = MediaQuery.disableAnimationsOf(
                          context,
                        );
                        if (reduceMotion) {
                          _scroll.jumpTo(0);
                        } else {
                          _scroll.animateTo(
                            0,
                            duration: Durations.short4,
                            curve: Curves.easeOut,
                          );
                        }
                      },
                      icon: const Icon(Icons.arrow_downward),
                    ),
                  ),
              ],
            ),
          ),
        ),
        Container(
          width: double.infinity,
          padding: const EdgeInsets.symmetric(
            horizontal: AppSizes.p16,
            vertical: AppSizes.p12,
          ),
          decoration: const BoxDecoration(
            border: Border(top: BorderSide(color: AppColors.borderDefault)),
          ),
          child: Text(
            '저장된 대화 · 읽기 전용',
            textAlign: TextAlign.center,
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ),
      ],
    );
  }
}
