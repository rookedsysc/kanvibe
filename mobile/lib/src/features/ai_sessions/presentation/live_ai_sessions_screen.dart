import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_sizes.dart';
import '../../../common/l10n/app_localizations.dart';
import '../application/ai_sessions_controller.dart';
import '../domain/live_ai_session.dart';
import 'agent_call_graph_screen.dart';
import 'widgets/ai_polling.dart';

class LiveAiSessionsScreen extends ConsumerWidget {
  const LiveAiSessionsScreen({
    required this.taskId,
    this.onSelectWindow,
    super.key,
  });
  final String taskId;
  final ValueChanged<String>? onSelectWindow;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final provider = liveAiSessionsProvider(taskId);
    final sessions = ref.watch(provider);
    return AiPolling(
      interval: const Duration(seconds: 2),
      onRefresh: () {
        if (!ref.read(provider).isLoading) ref.invalidate(provider);
      },
      child: Scaffold(
        appBar: AppBar(
          title: Text(context.tr('실행 중 AI 세션')),
          actions: [
            IconButton(
              tooltip: context.tr('새로고침'),
              onPressed: () => ref.invalidate(provider),
              icon: const Icon(Icons.refresh),
            ),
          ],
        ),
        body: SafeArea(
          child: Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 840),
              child: sessions.when(
                loading: () => const Center(child: CircularProgressIndicator()),
                error: (_, _) => Center(
                  child: Padding(
                    padding: const EdgeInsets.all(AppSizes.p24),
                    child: Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(
                          context.tr(
                            'AI 세션을 불러오지 못했습니다. PC 연결과 앱 버전을 확인해 주세요.',
                          ),
                          textAlign: TextAlign.center,
                        ),
                        TextButton(
                          onPressed: () => ref.invalidate(provider),
                          child: Text(context.tr('다시 시도')),
                        ),
                      ],
                    ),
                  ),
                ),
                data: (snapshot) => RefreshIndicator(
                  onRefresh: () async {
                    await AsyncValue.guard(() => ref.refresh(provider.future));
                  },
                  child: ListView(
                    padding: const EdgeInsets.all(AppSizes.p16),
                    physics: const AlwaysScrollableScrollPhysics(),
                    children: [
                      if (snapshot.sessions.isEmpty)
                        Padding(
                          padding: EdgeInsets.all(AppSizes.p24),
                          child: Text(
                            context.tr('이 태스크에서 실행 중이거나 최근 활동한 AI 세션이 없습니다.'),
                            textAlign: TextAlign.center,
                          ),
                        )
                      else ...[
                        Text(
                          context.tr('{count}개 세션 · {running}개 실행 중', {
                            'count': snapshot.sessions.length,
                            'running': snapshot.sessions
                                .where((session) => session.state == 'running')
                                .length,
                          }),
                          style: Theme.of(context).textTheme.titleMedium,
                        ),
                        const SizedBox(height: AppSizes.p16),
                        for (final session in snapshot.sessions)
                          _LiveSessionCard(
                            taskId: taskId,
                            session: session,
                            onSelectWindow: onSelectWindow,
                          ),
                      ],
                    ],
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _LiveSessionCard extends StatelessWidget {
  const _LiveSessionCard({
    required this.taskId,
    required this.session,
    this.onSelectWindow,
  });
  final String taskId;
  final LiveAiSession session;
  final ValueChanged<String>? onSelectWindow;
  @override
  Widget build(BuildContext context) {
    final isRunning = session.state == 'running';
    return Card(
      margin: const EdgeInsets.only(bottom: AppSizes.p12),
      child: Padding(
        padding: const EdgeInsets.all(AppSizes.p16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Wrap(
              spacing: AppSizes.p8,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                Text(
                  session.provider,
                  style: Theme.of(context).textTheme.titleMedium,
                ),
                Chip(
                  avatar: Icon(
                    isRunning ? Icons.play_arrow : Icons.pause,
                    size: AppSizes.p16,
                  ),
                  label: Text(
                    isRunning ? context.tr('실행 중') : context.tr('대기 중'),
                  ),
                ),
              ],
            ),
            if (session.currentTask != null)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: AppSizes.p8),
                child: SelectableText(session.currentTask!),
              ),
            if (session.lastActiveAt != null)
              Text(
                context.tr('최근 활동 {time}', {
                  'time': session.lastActiveAt!
                      .toLocal()
                      .toString()
                      .split('.')
                      .first,
                }),
                style: Theme.of(context).textTheme.bodySmall,
              ),
            if (session.runningSubtasks.isNotEmpty) ...[
              const SizedBox(height: AppSizes.p12),
              Text(
                context.tr('실행 중 하위 작업 {count}개', {
                  'count': session.runningSubtasks.length,
                }),
              ),
              for (final subtask in session.runningSubtasks)
                ListTile(
                  contentPadding: EdgeInsets.zero,
                  leading: const Icon(Icons.subdirectory_arrow_right),
                  title: Text(subtask.name ?? subtask.id),
                ),
            ],
            Wrap(
              spacing: AppSizes.p8,
              children: [
                if (session.terminalWindowId != null && onSelectWindow != null)
                  Semantics(
                    identifier: 'ai-session-terminal',
                    child: TextButton.icon(
                      onPressed: () {
                        onSelectWindow!(session.terminalWindowId!);
                      },
                      icon: const Icon(Icons.terminal),
                      label: Text(
                        session.terminalWindowName == null
                            ? context.tr('터미널 열기')
                            : context.tr('터미널 열기 · {name}', {
                                'name': session.terminalWindowName!,
                              }),
                      ),
                    ),
                  ),
                if (session.sessionId != null)
                  Semantics(
                    identifier: 'ai-session-graph',
                    child: TextButton.icon(
                      onPressed: () => Navigator.of(context).push(
                        MaterialPageRoute<void>(
                          builder: (_) => AgentCallGraphScreen(
                            target: (
                              taskId: taskId,
                              provider: session.provider,
                              sessionId: session.sessionId!,
                            ),
                          ),
                        ),
                      ),
                      icon: const Icon(Icons.account_tree_outlined),
                      label: Text(context.tr('호출 그래프')),
                    ),
                  ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
