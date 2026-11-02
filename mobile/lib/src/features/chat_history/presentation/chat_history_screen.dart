import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../application/chat_history_controller.dart';
import '../domain/chat_history.dart';

class ChatHistoryScreen extends ConsumerStatefulWidget {
  const ChatHistoryScreen({required this.taskId, super.key});
  final String taskId;

  @override
  ConsumerState<ChatHistoryScreen> createState() => _ChatHistoryScreenState();
}

class _ChatHistoryScreenState extends ConsumerState<ChatHistoryScreen> {
  final _search = TextEditingController();
  String? _provider;

  @override
  void dispose() {
    _search.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final provider = chatHistoryProvider(widget.taskId);
    final history = ref.watch(provider);
    return Scaffold(
      appBar: AppBar(title: const Text('채팅 History')),
      body: SafeArea(
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.all(AppSizes.p12),
              child: TextField(
                controller: _search,
                decoration: InputDecoration(
                  labelText: '대화 검색',
                  suffixIcon: IconButton(
                    tooltip: '검색',
                    icon: const Icon(Icons.search),
                    onPressed: () =>
                        ref.read(provider.notifier).search(_search.text),
                  ),
                ),
                textInputAction: TextInputAction.search,
                onSubmitted: (query) =>
                    ref.read(provider.notifier).search(query),
              ),
            ),
            Expanded(
              child: history.when(
                loading: () => const Center(child: CircularProgressIndicator()),
                error: (_, _) =>
                    _HistoryRetry(onRetry: () => ref.invalidate(provider)),
                data: (state) {
                  final sessions = state.page.sessions
                      .where(
                        (session) =>
                            _provider == null || session.provider == _provider,
                      )
                      .toList();
                  return Column(
                    children: [
                      if (state.page.providers.isNotEmpty)
                        SizedBox(
                          height: AppSizes.minTouchTarget + AppSizes.p8,
                          child: ListView(
                            scrollDirection: Axis.horizontal,
                            padding: const EdgeInsets.symmetric(
                              horizontal: AppSizes.p12,
                            ),
                            children: [
                              for (final agent in <String?>[
                                null,
                                ...state.page.providers,
                              ])
                                Padding(
                                  padding: const EdgeInsets.only(
                                    right: AppSizes.p8,
                                  ),
                                  child: ChoiceChip(
                                    label: Text(agent ?? '전체'),
                                    selected: agent == _provider,
                                    onSelected: (_) =>
                                        setState(() => _provider = agent),
                                  ),
                                ),
                            ],
                          ),
                        ),
                      Expanded(
                        child: RefreshIndicator(
                          onRefresh: () async {
                            ref.invalidate(provider);
                            await ref.read(provider.future);
                          },
                          child: ListView.builder(
                            physics: const AlwaysScrollableScrollPhysics(),
                            itemCount: sessions.length + 1,
                            itemBuilder: (context, index) {
                              if (index == sessions.length) {
                                return Padding(
                                  padding: const EdgeInsets.all(AppSizes.p16),
                                  child: state.page.nextCursor != null
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
                                                ? '목록 다시 불러오기'
                                                : '대화 더 보기',
                                          ),
                                        )
                                      : sessions.isEmpty
                                      ? const Center(
                                          child: Text('이 태스크의 대화 기록이 없습니다.'),
                                        )
                                      : const SizedBox.shrink(),
                                );
                              }
                              final session = sessions[index];
                              return ListTile(
                                key: ValueKey(session.key),
                                title: Text(
                                  session.title,
                                  maxLines: 2,
                                  overflow: TextOverflow.ellipsis,
                                ),
                                subtitle: Text(
                                  '${session.provider}${session.updatedAt == null ? '' : ' · ${session.updatedAt!.toLocal().toString().split('.').first}'}',
                                ),
                                trailing: const Icon(Icons.chevron_right),
                                onTap: () => Navigator.of(context).push(
                                  MaterialPageRoute<void>(
                                    builder: (_) => ChatMessagesScreen(
                                      taskId: widget.taskId,
                                      session: session,
                                    ),
                                  ),
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
        ),
      ),
    );
  }
}

class ChatMessagesScreen extends ConsumerWidget {
  const ChatMessagesScreen({
    required this.taskId,
    required this.session,
    super.key,
  });
  final String taskId;
  final ChatSession session;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final provider = chatMessagesProvider((taskId: taskId, session: session));
    final messages = ref.watch(provider);
    return Scaffold(
      appBar: AppBar(
        title: Text(
          session.title,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
      ),
      body: SafeArea(
        child: messages.when(
          loading: () => const Center(child: CircularProgressIndicator()),
          error: (_, _) =>
              _HistoryRetry(onRetry: () => ref.invalidate(provider)),
          data: (state) => ListView.builder(
            reverse: true,
            padding: const EdgeInsets.all(AppSizes.p12),
            itemCount: state.page.messages.length + 1,
            itemBuilder: (context, index) {
              if (index == state.page.messages.length) {
                return state.page.nextCursor != null
                    ? TextButton(
                        onPressed: state.isLoadingMore
                            ? null
                            : () => ref.read(provider.notifier).loadMore(),
                        child: Text(
                          state.isLoadingMore
                              ? '불러오는 중…'
                              : state.loadMoreFailed
                              ? '이전 메시지 다시 불러오기'
                              : '이전 메시지 보기',
                        ),
                      )
                    : state.page.messages.isEmpty
                    ? const Center(child: Text('표시할 메시지가 없습니다.'))
                    : const SizedBox.shrink();
              }
              final message = state.page.messages[index];
              return Container(
                margin: const EdgeInsets.only(bottom: AppSizes.p12),
                padding: const EdgeInsets.all(AppSizes.p12),
                decoration: BoxDecoration(
                  color: message.role == 'user'
                      ? AppColors.brandSubtle
                      : AppColors.bgSurface,
                  borderRadius: BorderRadius.circular(AppSizes.radiusLg),
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Expanded(
                          child: Text(
                            message.role,
                            style: const TextStyle(
                              color: AppColors.textSecondary,
                            ),
                          ),
                        ),
                        IconButton(
                          tooltip: '메시지 복사',
                          icon: const Icon(Icons.copy, size: AppSizes.p16),
                          onPressed: () async {
                            await Clipboard.setData(
                              ClipboardData(text: message.text),
                            );
                            if (context.mounted) {
                              ScaffoldMessenger.of(context).showSnackBar(
                                const SnackBar(content: Text('메시지를 복사했습니다.')),
                              );
                            }
                          },
                        ),
                      ],
                    ),
                    SelectableText(message.text),
                  ],
                ),
              );
            },
          ),
        ),
      ),
    );
  }
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
