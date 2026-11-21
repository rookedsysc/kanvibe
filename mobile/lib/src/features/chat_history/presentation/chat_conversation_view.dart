import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/l10n/app_localizations.dart';
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
    final target = (taskId: widget.taskId, session: widget.session);
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.all(AppSizes.p12),
          child: _ChatMessageSearchField(
            key: ValueKey((widget.taskId, widget.session.key)),
            target: target,
          ),
        ),
        SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          padding: const EdgeInsets.symmetric(horizontal: AppSizes.p12),
          child: Row(
            children: [
              for (final role in [
                'user',
                'assistant',
                'tool',
                'system',
                'developer',
                'reasoning',
                'unknown',
              ])
                Padding(
                  padding: const EdgeInsets.only(right: AppSizes.p8),
                  child: FilterChip(
                    label: Text(role),
                    selected: ref
                        .watch(chatMessageRolesProvider(target))
                        .contains(role),
                    onSelected: (_) => ref
                        .read(chatMessageRolesProvider(target).notifier)
                        .toggle(role),
                  ),
                ),
            ],
          ),
        ),
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
                          taskId: widget.taskId,
                          sessionId: widget.session.id,
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

class _ChatMessageSearchField extends ConsumerStatefulWidget {
  const _ChatMessageSearchField({required this.target, super.key});

  final ({String taskId, ChatSession session}) target;

  @override
  ConsumerState<_ChatMessageSearchField> createState() =>
      _ChatMessageSearchFieldState();
}

class _ChatMessageSearchFieldState
    extends ConsumerState<_ChatMessageSearchField> {
  late final TextEditingController _search;
  late String _committedQuery;

  @override
  void initState() {
    super.initState();
    _committedQuery = ref.read(chatMessageQueryProvider(widget.target));
    _search = TextEditingController(text: _committedQuery);
  }

  void _syncCommittedQuery(String query) {
    if (query == _committedQuery) return;
    _committedQuery = query;
    if (_search.text != query) {
      _search.value = TextEditingValue(
        text: query,
        selection: TextSelection.collapsed(offset: query.length),
      );
    }
  }

  @override
  void didUpdateWidget(covariant _ChatMessageSearchField oldWidget) {
    super.didUpdateWidget(oldWidget);
    _syncCommittedQuery(ref.read(chatMessageQueryProvider(widget.target)));
  }

  @override
  void dispose() {
    _search.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final query = chatMessageQueryProvider(widget.target);
    ref.listen(query, (_, next) => _syncCommittedQuery(next));
    return TextFormField(
      controller: _search,
      textInputAction: TextInputAction.search,
      onFieldSubmitted: (value) => ref.read(query.notifier).search(value),
      decoration: InputDecoration(
        labelText: context.tr('메시지 본문 검색'),
        prefixIcon: Icon(Icons.search),
        suffixIcon: IconButton(
          tooltip: context.tr('검색 지우기'),
          icon: Icon(Icons.clear),
          onPressed: () {
            _search.clear();
            ref.read(query.notifier).search('');
          },
        ),
      ),
    );
  }
}
