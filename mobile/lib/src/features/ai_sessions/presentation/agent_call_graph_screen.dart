import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_sizes.dart';
import '../../../common/l10n/app_localizations.dart';
import '../application/ai_sessions_controller.dart';
import '../domain/agent_call_graph.dart';
import 'widgets/ai_polling.dart';

class AgentCallGraphScreen extends ConsumerWidget {
  const AgentCallGraphScreen({required this.target, super.key});
  final AgentGraphTarget target;
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final provider = agentCallGraphProvider(target);
    final graph = ref.watch(provider);
    return AiPolling(
      interval: const Duration(seconds: 10),
      onRefresh: () {
        if (!ref.read(provider).isLoading) ref.invalidate(provider);
      },
      child: Scaffold(
        appBar: AppBar(
          title: Text(context.tr('Agent 호출 그래프')),
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
              child: graph.when(
                loading: () => const Center(child: CircularProgressIndicator()),
                error: (_, _) => Center(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(context.tr('호출 그래프를 불러오지 못했습니다.')),
                      TextButton(
                        onPressed: () => ref.invalidate(provider),
                        child: Text(context.tr('다시 시도')),
                      ),
                    ],
                  ),
                ),
                data: (value) => RefreshIndicator(
                  onRefresh: () async {
                    await AsyncValue.guard(() => ref.refresh(provider.future));
                  },
                  child: ListView(
                    padding: const EdgeInsets.all(AppSizes.p16),
                    physics: const AlwaysScrollableScrollPhysics(),
                    children: [
                      Text(
                        target.provider,
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                      if (value == null || value.roots.isEmpty)
                        Padding(
                          padding: EdgeInsets.all(AppSizes.p24),
                          child: Text(
                            context.tr('이 세션에는 표시할 하위 Agent 호출이 없습니다.'),
                          ),
                        )
                      else
                        for (final row in _flatten(value.roots, 0))
                          _AgentTimelineRow(
                            node: row.node,
                            depth: row.depth,
                            graph: value,
                          ),
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

Iterable<({AgentCallNode node, int depth})> _flatten(
  List<AgentCallNode> nodes,
  int depth,
) sync* {
  for (final node in nodes) {
    yield (node: node, depth: depth);
    yield* _flatten(node.children, depth + 1);
  }
}

class _AgentTimelineRow extends StatelessWidget {
  const _AgentTimelineRow({
    required this.node,
    required this.depth,
    required this.graph,
  });
  final AgentCallNode node;
  final int depth;
  final AgentCallGraph graph;
  @override
  Widget build(BuildContext context) {
    final start = graph.startedAt ?? node.startedAt ?? graph.readAt;
    final end = graph.readAt ?? node.endedAt ?? node.startedAt;
    final total = start == null || end == null
        ? 1
        : math.max(1, end.difference(start).inMilliseconds);
    final offset = start == null || node.startedAt == null
        ? 0.0
        : (node.startedAt!.difference(start).inMilliseconds / total).clamp(
            0.0,
            1.0,
          );
    final finish = start == null || end == null
        ? 1.0
        : ((node.endedAt ?? end).difference(start).inMilliseconds / total)
              .clamp(offset, 1.0);
    final duration = node.startedAt == null || end == null
        ? null
        : (node.endedAt ?? end).difference(node.startedAt!).inSeconds;
    final colors = Theme.of(context).colorScheme;
    return Padding(
      padding: EdgeInsets.only(
        left: math.min(depth, 6) * AppSizes.p12,
        top: AppSizes.p12,
      ),
      child: Card(
        child: Padding(
          padding: const EdgeInsets.all(AppSizes.p16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                node.agentType ?? node.id,
                style: Theme.of(context).textTheme.titleSmall,
              ),
              if (node.task != null)
                Padding(
                  padding: const EdgeInsets.only(top: AppSizes.p8),
                  child: SelectableText(node.task!),
                ),
              if (node.skill != null)
                Text(
                  'Skill · ${node.skill}',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              const SizedBox(height: AppSizes.p12),
              Semantics(
                label: duration == null
                    ? context.tr(node.endedAt == null ? '실행 중' : '완료')
                    : context.tr('{status} · {seconds}초', {
                        'status': context.tr(
                          node.endedAt == null ? '실행 중' : '완료',
                        ),
                        'seconds': duration,
                      }),
                child: LayoutBuilder(
                  builder: (context, constraints) => SizedBox(
                    height: AppSizes.p12,
                    child: Stack(
                      children: [
                        Positioned.fill(
                          child: DecoratedBox(
                            decoration: BoxDecoration(
                              color: colors.surfaceContainerHighest,
                              borderRadius: BorderRadius.circular(
                                AppSizes.radiusSm,
                              ),
                            ),
                          ),
                        ),
                        Positioned(
                          left: offset * constraints.maxWidth,
                          width: math.max(
                            AppSizes.p4,
                            (finish - offset) * constraints.maxWidth,
                          ),
                          top: 0,
                          bottom: 0,
                          child: DecoratedBox(
                            decoration: BoxDecoration(
                              color: node.endedAt == null
                                  ? colors.primary
                                  : colors.secondary,
                              borderRadius: BorderRadius.circular(
                                AppSizes.radiusSm,
                              ),
                            ),
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
              const SizedBox(height: AppSizes.p8),
              Text(
                duration == null
                    ? context.tr(node.endedAt == null ? '실행 중' : '완료')
                    : context.tr('{status} · {seconds}초', {
                        'status': context.tr(
                          node.endedAt == null ? '실행 중' : '완료',
                        ),
                        'seconds': duration,
                      }),
                style: Theme.of(context).textTheme.bodySmall,
              ),
              if (node.startedAt != null)
                Text(
                  context.tr('시작 {time}', {
                    'time': node.startedAt!
                        .toLocal()
                        .toString()
                        .split('.')
                        .first,
                  }),
                  style: Theme.of(context).textTheme.bodySmall,
                ),
            ],
          ),
        ),
      ),
    );
  }
}
