import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_sizes.dart';
import '../../../../common/l10n/app_localizations.dart';
import '../../application/board_sort_controller.dart';
import '../../domain/board_view_options.dart';

class BoardSortSheet extends ConsumerStatefulWidget {
  const BoardSortSheet({super.key});
  @override
  ConsumerState<BoardSortSheet> createState() => _BoardSortSheetState();
}

class _BoardSortSheetState extends ConsumerState<BoardSortSheet> {
  List<BoardSortKey>? _keys;
  bool _isSaving = false;
  bool _saveFailed = false;

  Future<void> _save() async {
    setState(() => _isSaving = true);
    final saved = await ref.read(boardSortProvider.notifier).save(_keys ?? []);
    if (!mounted) return;
    if (saved) {
      Navigator.of(context).pop();
      return;
    }
    setState(() {
      _isSaving = false;
      _saveFailed = true;
    });
  }

  @override
  Widget build(BuildContext context) {
    final preference = ref.watch(boardSortProvider);
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.all(AppSizes.p16),
        child: preference.when(
          loading: () => const SizedBox(
            height: 160,
            child: Center(child: CircularProgressIndicator()),
          ),
          error: (_, _) => Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(context.tr('정렬 설정을 불러오지 못했습니다.')),
              TextButton(
                onPressed: () => ref.invalidate(boardSortProvider),
                child: Text(context.tr('다시 시도')),
              ),
            ],
          ),
          data: (savedKeys) {
            final keys = _keys ??= List.of(savedKeys);
            final available = BoardSortField.values
                .where((field) => !keys.any((key) => key.field == field))
                .toList();
            return SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(
                    context.tr('보드 정렬'),
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: AppSizes.p8),
                  Text(context.tr('위에 있는 기준부터 적용하며 이 PC에 저장됩니다.')),
                  for (final (index, key) in keys.indexed)
                    ListTile(
                      contentPadding: EdgeInsets.zero,
                      title: Text(context.tr(boardSortLabels[key.field]!)),
                      subtitle: Text(
                        context.tr(key.descending ? '내림차순' : '오름차순'),
                      ),
                      trailing: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          IconButton(
                            tooltip: context.tr('정렬 방향 바꾸기'),
                            onPressed: _isSaving
                                ? null
                                : () => setState(
                                    () => keys[index] = BoardSortKey(
                                      key.field,
                                      descending: !key.descending,
                                    ),
                                  ),
                            icon: Icon(
                              key.descending ? Icons.south : Icons.north,
                            ),
                          ),
                          IconButton(
                            tooltip: context.tr('위로 이동'),
                            onPressed: _isSaving || index == 0
                                ? null
                                : () => setState(() {
                                    final previous = keys[index - 1];
                                    keys[index - 1] = key;
                                    keys[index] = previous;
                                  }),
                            icon: const Icon(Icons.arrow_upward),
                          ),
                          IconButton(
                            tooltip: context.tr('정렬 기준 삭제'),
                            onPressed: _isSaving
                                ? null
                                : () => setState(() => keys.removeAt(index)),
                            icon: const Icon(Icons.close),
                          ),
                        ],
                      ),
                    ),
                  if (available.isNotEmpty)
                    DropdownButtonFormField<BoardSortField>(
                      key: ValueKey(keys.length),
                      isExpanded: true,
                      decoration: InputDecoration(
                        labelText: context.tr('정렬 기준 추가'),
                      ),
                      items: [
                        for (final field in available)
                          DropdownMenuItem(
                            value: field,
                            child: Text(context.tr(boardSortLabels[field]!)),
                          ),
                      ],
                      onChanged: _isSaving
                          ? null
                          : (field) {
                              if (field != null) {
                                setState(() => keys.add(BoardSortKey(field)));
                              }
                            },
                    ),
                  TextButton(
                    onPressed: _isSaving
                        ? null
                        : () => setState(() => _keys = []),
                    child: Text(context.tr('기본 순서')),
                  ),
                  if (_saveFailed)
                    Text(
                      context.tr('정렬 설정을 저장하지 못했습니다.'),
                      style: TextStyle(
                        color: Theme.of(context).colorScheme.error,
                      ),
                    ),
                  const SizedBox(height: AppSizes.p16),
                  FilledButton(
                    onPressed: _isSaving ? null : _save,
                    child: Text(context.tr(_isSaving ? '저장 중…' : '적용')),
                  ),
                ],
              ),
            );
          },
        ),
      ),
    );
  }
}

const boardSortLabels = {
  BoardSortField.priority: '우선순위',
  BoardSortField.createdAt: '생성일',
  BoardSortField.updatedAt: '수정일',
  BoardSortField.title: '제목',
  BoardSortField.project: '프로젝트',
};
