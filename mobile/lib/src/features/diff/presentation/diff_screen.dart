import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/l10n/app_localizations.dart';
import '../application/diff_controller.dart';
import '../domain/git_diff.dart';
import 'diff_file_screen.dart';

class DiffScreen extends ConsumerStatefulWidget {
  const DiffScreen({required this.taskId, super.key});
  final String taskId;
  @override
  ConsumerState<DiffScreen> createState() => _DiffScreenState();
}

class _DiffScreenState extends ConsumerState<DiffScreen> {
  final _viewed = <String>{};
  @override
  Widget build(BuildContext context) {
    final files = ref.watch(diffFilesProvider(widget.taskId));
    return Scaffold(
      appBar: AppBar(
        title: Text(context.tr('변경 검토')),
        actions: [
          IconButton(
            tooltip: context.tr('새로고침'),
            onPressed: () => ref.invalidate(diffFilesProvider(widget.taskId)),
            icon: const Icon(Icons.refresh),
          ),
        ],
      ),
      body: files.when(
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (error, _) => Center(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(context.tr('변경 파일을 불러오지 못했습니다.')),
              const SizedBox(height: 12),
              FilledButton(
                onPressed: () =>
                    ref.invalidate(diffFilesProvider(widget.taskId)),
                child: Text(context.tr('다시 시도')),
              ),
            ],
          ),
        ),
        data: (files) {
          if (files.isEmpty) {
            return Center(child: Text(context.tr('변경된 파일이 없습니다.')));
          }
          final groups = <String, List<ChangedFile>>{};
          for (final file in files) {
            final index = file.path.lastIndexOf('/');
            final folder = index < 0 ? '.' : file.path.substring(0, index);
            groups.putIfAbsent(folder, () => []).add(file);
          }
          final additions = files.fold(0, (sum, file) => sum + file.additions);
          final deletions = files.fold(0, (sum, file) => sum + file.deletions);
          return ListView(
            padding: const EdgeInsets.all(16),
            children: [
              Text(
                context.tr('{count}개 파일 · +{added} −{deleted}', {
                  'count': files.length,
                  'added': additions,
                  'deleted': deletions,
                }),
                style: Theme.of(context).textTheme.titleMedium,
              ),
              const SizedBox(height: 8),
              Text(
                context.tr('검토 완료 표시는 이 화면을 나가면 초기화됩니다.'),
                style: TextStyle(color: context.colors.textMuted),
              ),
              const SizedBox(height: 16),
              for (final entry in groups.entries)
                ExpansionTile(
                  key: PageStorageKey(entry.key),
                  initiallyExpanded: true,
                  tilePadding: EdgeInsets.zero,
                  leading: const Icon(Icons.folder_outlined),
                  title: Text(entry.key),
                  children: [
                    for (final file in entry.value)
                      ListTile(
                        key: ValueKey('diff-file-${file.path}'),
                        leading: SizedBox(
                          width: 28,
                          child: Text(
                            file.statusLetter,
                            style: TextStyle(
                              color: file.status == 'deleted'
                                  ? AppColors.statusError
                                  : AppColors.statusDone,
                              fontWeight: FontWeight.bold,
                            ),
                          ),
                        ),
                        title: Text(file.path.split('/').last),
                        subtitle: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            if (file.originalPath != null)
                              Text('${file.originalPath} → ${file.path}'),
                            Text('+${file.additions}  −${file.deletions}'),
                            if (file.additions + file.deletions > 0)
                              Padding(
                                padding: const EdgeInsets.only(top: 4),
                                child: LinearProgressIndicator(
                                  value:
                                      file.additions /
                                      (file.additions + file.deletions),
                                  color: AppColors.statusDone,
                                  backgroundColor: AppColors.statusError,
                                  semanticsLabel: context.tr('추가·삭제 비율'),
                                ),
                              ),
                          ],
                        ),
                        trailing: Checkbox(
                          semanticLabel: context.tr('파일 검토 완료'),
                          value: _viewed.contains(file.path),
                          onChanged: (value) => setState(() {
                            if (value == true) {
                              _viewed.add(file.path);
                            } else {
                              _viewed.remove(file.path);
                            }
                          }),
                        ),
                        onTap: () => Navigator.of(context).push(
                          MaterialPageRoute<void>(
                            builder: (_) => DiffFileScreen(
                              taskId: widget.taskId,
                              file: file,
                            ),
                          ),
                        ),
                      ),
                  ],
                ),
            ],
          );
        },
      ),
    );
  }
}
