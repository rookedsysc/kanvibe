import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/providers.dart';
import '../application/chat_history_controller.dart';
import '../domain/chat_history.dart';
import 'chat_conversation_view.dart';

class ChatHistoryScreen extends ConsumerStatefulWidget {
  const ChatHistoryScreen({required this.taskId, super.key});
  final String taskId;

  @override
  ConsumerState<ChatHistoryScreen> createState() => _ChatHistoryScreenState();
}

class _ChatHistoryScreenState extends ConsumerState<ChatHistoryScreen> {
  final _search = TextEditingController();
  String? _provider;
  ChatSession? _selected;
  static const _sessionListWidth = 320.0;

  @override
  void dispose() {
    _search.dispose();
    super.dispose();
  }

  void _searchHistory(String query) {
    setState(() => _selected = null);
    ref.read(chatHistoryProvider(widget.taskId).notifier).search(query);
  }

  @override
  Widget build(BuildContext context) {
    ref.listen(
      connectionControllerProvider.select((state) => state.value?.activeId),
      (previous, next) {
        if (previous != next) {
          setState(() {
            _selected = null;
            _provider = null;
          });
        }
      },
    );
    final provider = chatHistoryProvider(widget.taskId);
    return Scaffold(
      appBar: AppBar(title: const Text('대화 기록')),
      body: SafeArea(
        child: LayoutBuilder(
          builder: (context, constraints) {
            final isWide = isTabletLayout(constraints);
            final list = Column(
              key: const ValueKey('chat-session-list'),
              children: [
                Padding(
                  padding: const EdgeInsets.all(AppSizes.p16),
                  child: TextField(
                    controller: _search,
                    onChanged: (_) => setState(() {}),
                    decoration: InputDecoration(
                      labelText: '대화 검색',
                      prefixIcon: const Icon(Icons.search),
                      suffixIcon: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          if (_search.text.isNotEmpty)
                            IconButton(
                              tooltip: '검색 지우기',
                              icon: const Icon(Icons.close),
                              onPressed: () {
                                setState(() => _search.clear());
                                _searchHistory('');
                              },
                            ),
                          IconButton(
                            tooltip: '검색',
                            icon: const Icon(Icons.arrow_forward),
                            onPressed: () => _searchHistory(_search.text),
                          ),
                        ],
                      ),
                    ),
                    textInputAction: TextInputAction.search,
                    onSubmitted: (query) => _searchHistory(query),
                  ),
                ),
                Expanded(
                  child: ref
                      .watch(provider)
                      .when(
                        loading: () =>
                            const Center(child: CircularProgressIndicator()),
                        error: (_, _) => _HistoryRetry(
                          onRetry: () => ref.invalidate(provider),
                        ),
                        data: (state) {
                          final sessions = state.page.sessions
                              .where(
                                (session) =>
                                    _provider == null ||
                                    session.provider == _provider,
                              )
                              .toList();
                          final providers = {
                            ...state.page.providers,
                            ...state.page.sessions.map(
                              (session) => session.provider,
                            ),
                          };
                          return Column(
                            children: [
                              if (providers.isNotEmpty)
                                SizedBox(
                                  height: AppSizes.minTouchTarget + AppSizes.p8,
                                  child: ListView(
                                    scrollDirection: Axis.horizontal,
                                    padding: const EdgeInsets.symmetric(
                                      horizontal: AppSizes.p16,
                                    ),
                                    children: [
                                      for (final agent in <String?>[
                                        null,
                                        ...providers,
                                      ])
                                        Padding(
                                          padding: const EdgeInsets.only(
                                            right: AppSizes.p8,
                                          ),
                                          child: ChoiceChip(
                                            label: Text(agent ?? '전체'),
                                            selected: agent == _provider,
                                            selectedColor:
                                                AppColors.brandSubtle,
                                            backgroundColor:
                                                AppColors.bgSurface,
                                            checkmarkColor:
                                                AppColors.brandPrimary,
                                            side: BorderSide(
                                              color: agent == _provider
                                                  ? AppColors.brandPrimary
                                                  : AppColors.borderDefault,
                                            ),
                                            labelStyle: TextStyle(
                                              color: agent == _provider
                                                  ? AppColors.textPrimary
                                                  : AppColors.textSecondary,
                                            ),
                                            onSelected: (_) => setState(
                                              () => _provider = agent,
                                            ),
                                          ),
                                        ),
                                    ],
                                  ),
                                ),
                              Padding(
                                padding: const EdgeInsets.symmetric(
                                  horizontal: AppSizes.p16,
                                  vertical: AppSizes.p8,
                                ),
                                child: Align(
                                  alignment: Alignment.centerLeft,
                                  child: Text(
                                    '${sessions.length}개 대화',
                                    style: Theme.of(context)
                                        .textTheme
                                        .bodySmall,
                                  ),
                                ),
                              ),
                              Expanded(
                                child: RefreshIndicator(
                                  onRefresh: () async {
                                    ref.invalidate(provider);
                                    await ref.read(provider.future);
                                  },
                                  child: ListView.builder(
                                    key: PageStorageKey(
                                      'chat-history:${widget.taskId}:$_provider',
                                    ),
                                    physics:
                                        const AlwaysScrollableScrollPhysics(),
                                    padding: const EdgeInsets.symmetric(
                                      horizontal: AppSizes.p8,
                                    ),
                                    itemCount: sessions.length + 1,
                                    itemBuilder: (context, index) {
                                      if (index == sessions.length) {
                                        return Padding(
                                          padding: const EdgeInsets.all(
                                            AppSizes.p16,
                                          ),
                                          child: state.page.nextCursor != null
                                              ? TextButton(
                                                  onPressed: state.isLoadingMore
                                                      ? null
                                                      : () => ref
                                                            .read(
                                                              provider.notifier,
                                                            )
                                                            .loadMore(),
                                                  child: Text(
                                                    state.isLoadingMore
                                                        ? '불러오는 중…'
                                                        : state.loadMoreFailed
                                                        ? '목록 다시 불러오기'
                                                        : '대화 더 보기',
                                                  ),
                                                )
                                              : sessions.isEmpty
                                              ? Center(
                                                  child: Text(
                                                    _search.text.isNotEmpty ||
                                                            _provider != null
                                                        ? '검색 조건에 맞는 대화가 없습니다.'
                                                        : '이 태스크의 대화 기록이 없습니다.',
                                                  ),
                                                )
                                              : const SizedBox.shrink(),
                                        );
                                      }
                                      final session = sessions[index];
                                      return Padding(
                                        padding: const EdgeInsets.only(
                                          bottom: AppSizes.p4,
                                        ),
                                        child: ListTile(
                                          key: ValueKey(session.key),
                                          selected:
                                              isWide &&
                                              _selected?.key == session.key,
                                          selectedTileColor:
                                              AppColors.bgSurface,
                                          selectedColor: AppColors.textPrimary,
                                          shape: RoundedRectangleBorder(
                                            borderRadius: BorderRadius.circular(
                                              AppSizes.radiusLg,
                                            ),
                                          ),
                                          contentPadding:
                                              const EdgeInsets.symmetric(
                                                horizontal: AppSizes.p12,
                                                vertical: AppSizes.p8,
                                              ),
                                          title: Text(
                                            session.title,
                                            maxLines: 2,
                                            overflow: TextOverflow.ellipsis,
                                          ),
                                          subtitle: Padding(
                                            padding: const EdgeInsets.only(
                                              top: AppSizes.p4,
                                            ),
                                            child: Column(
                                              crossAxisAlignment:
                                                  CrossAxisAlignment.start,
                                              children: [
                                                if (session.preview != null &&
                                                    session.preview !=
                                                        session.title)
                                                  Text(
                                                    session.preview!,
                                                    maxLines: 1,
                                                    overflow:
                                                        TextOverflow.ellipsis,
                                                  ),
                                                Text(
                                                  [
                                                    session.provider,
                                                    if (session.updatedAt !=
                                                        null)
                                                      MaterialLocalizations.of(
                                                        context,
                                                      ).formatShortDate(
                                                        session.updatedAt!
                                                            .toLocal(),
                                                      ),
                                                    if (session.messageCount >
                                                        0)
                                                      '${session.messageCount}개 메시지',
                                                  ].join(' · '),
                                                  style: Theme.of(context)
                                                      .textTheme
                                                      .bodySmall,
                                                ),
                                              ],
                                            ),
                                          ),
                                          trailing: const Icon(
                                            Icons.chevron_right,
                                            size: AppSizes.p16,
                                          ),
                                          onTap: () {
                                            if (isWide) {
                                              setState(
                                                () => _selected = session,
                                              );
                                            } else {
                                              Navigator.of(context).push(
                                                MaterialPageRoute<void>(
                                                  builder: (_) =>
                                                      ChatMessagesScreen(
                                                        taskId: widget.taskId,
                                                        session: session,
                                                      ),
                                                ),
                                              );
                                            }
                                          },
                                        ),
                                      );
                                    },
                                  ),
                                ),
                              ),
                            ],
                          );
                        },
                      ),
                ),
              ],
            );
            if (!isWide) return list;
            final selected = _selected;
            return Row(
              children: [
                SizedBox(width: _sessionListWidth, child: list),
                const VerticalDivider(width: 1, color: AppColors.borderDefault),
                Expanded(
                  child: selected == null
                      ? const Center(child: Text('목록에서 대화를 선택하세요.'))
                      : Column(
                          children: [
                            ListTile(
                              title: Text(
                                selected.title,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                              ),
                              subtitle: Text(selected.provider),
                            ),
                            const Divider(
                              height: 1,
                              color: AppColors.borderDefault,
                            ),
                            Expanded(
                              child: ChatConversationView(
                                key: ValueKey(selected.key),
                                taskId: widget.taskId,
                                session: selected,
                              ),
                            ),
                          ],
                        ),
                ),
              ],
            );
          },
        ),
      ),
    );
  }
}

class ChatMessagesScreen extends StatelessWidget {
  const ChatMessagesScreen({
    required this.taskId,
    required this.session,
    super.key,
  });
  final String taskId;
  final ChatSession session;

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(
      title: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(session.title, maxLines: 1, overflow: TextOverflow.ellipsis),
          Text(session.provider, style: Theme.of(context).textTheme.bodySmall),
        ],
      ),
    ),
    body: SafeArea(
      child: ChatConversationView(taskId: taskId, session: session),
    ),
  );
}

class _HistoryRetry extends StatelessWidget {
  const _HistoryRetry({required this.onRetry});
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) => Center(
    child: Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        const Text('대화를 불러오지 못했습니다.'),
        TextButton(onPressed: onRetry, child: const Text('다시 시도')),
      ],
    ),
  );
}
