import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/constants/app_theme.dart';
import '../../../common/l10n/app_localizations.dart';
import '../../../common/providers.dart';
import '../application/diff_controller.dart';
import '../domain/git_diff.dart';

class DiffFileScreen extends ConsumerStatefulWidget {
  const DiffFileScreen({required this.taskId, required this.file, super.key});
  final String taskId;
  final ChangedFile file;
  @override
  ConsumerState<DiffFileScreen> createState() => _DiffFileScreenState();
}

class _DiffFileScreenState extends ConsumerState<DiffFileScreen> {
  final _editor = TextEditingController();
  String? _saved;
  bool _editing = false;
  bool _saving = false;
  bool _allowPop = false;
  (String, String)? _connectionIdentity;
  int _connectionGeneration = 0;
  bool get _dirty => _saved != null && _editor.text != _saved;
  DiffFileTarget get _target =>
      (taskId: widget.taskId, filePath: widget.file.path);
  @override
  void dispose() {
    _editor.dispose();
    super.dispose();
  }

  Future<void> _discardAndPop() async {
    final generation = _connectionGeneration;
    final discard = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(context.tr('저장하지 않은 변경을 버릴까요?')),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(context.tr('계속 편집')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(context.tr('변경 버리기')),
          ),
        ],
      ),
    );
    if (!mounted || discard != true || generation != _connectionGeneration) {
      return;
    }
    setState(() => _allowPop = true);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && generation == _connectionGeneration) {
        Navigator.of(context).pop();
      }
    });
  }

  Future<void> _save() async {
    if (_saving || !_dirty) return;
    final generation = _connectionGeneration;
    setState(() => _saving = true);
    try {
      final saved = await ref
          .read(diffDocumentProvider(_target).notifier)
          .save(_editor.text);
      if (!mounted || generation != _connectionGeneration) return;
      if (saved) {
        setState(() => _saved = _editor.text);
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: Text(context.tr('파일을 저장했습니다.'))));
      } else {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(context.tr('파일을 저장하지 못했습니다. 다시 시도해 주세요.'))),
        );
      }
    } catch (_) {
      if (mounted && generation == _connectionGeneration) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(context.tr('파일을 저장하지 못했습니다. 다시 시도해 주세요.'))),
        );
      }
    } finally {
      if (mounted && generation == _connectionGeneration) {
        setState(() => _saving = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final active = ref
        .watch(connectionControllerProvider)
        .value
        ?.activeConnection;
    final identity = active == null ? null : (active.id, active.token);
    if (identity != _connectionIdentity) {
      _connectionIdentity = identity;
      _connectionGeneration++;
      _saved = null;
      _editor.clear();
      _editing = false;
      _saving = false;
      _allowPop = false;
    }
    final document = ref.watch(diffDocumentProvider(_target));
    return PopScope(
      canPop: !_saving && (!_dirty || _allowPop),
      onPopInvokedWithResult: (didPop, result) {
        if (!didPop && !_saving && _dirty) _discardAndPop();
      },
      child: Scaffold(
        appBar: AppBar(
          title: Text(
            widget.file.path,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
          ),
          actions: [
            IconButton(
              tooltip: context.tr(_editing ? '비교 보기' : '파일 편집'),
              onPressed: document.value == null || _saving
                  ? null
                  : () => setState(() => _editing = !_editing),
              icon: Icon(_editing ? Icons.compare : Icons.edit_outlined),
            ),
            IconButton(
              key: const ValueKey('diff-save'),
              tooltip: context.tr('파일 저장'),
              onPressed: _dirty && !_saving ? _save : null,
              icon: _saving
                  ? const SizedBox(
                      width: 20,
                      height: 20,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Icon(Icons.save_outlined),
            ),
          ],
        ),
        body: document.when(
          loading: () => const Center(child: CircularProgressIndicator()),
          error: (_, _) => Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(context.tr('파일 내용을 불러오지 못했습니다.')),
                FilledButton(
                  onPressed: () =>
                      ref.invalidate(diffDocumentProvider(_target)),
                  child: Text(context.tr('다시 시도')),
                ),
              ],
            ),
          ),
          data: (file) {
            if (_saved == null) {
              _saved = file.content;
              _editor.text = file.content;
            }
            if (_editing) {
              return Padding(
                padding: const EdgeInsets.all(16),
                child: TextField(
                  key: const ValueKey('diff-editor'),
                  controller: _editor,
                  onChanged: (_) => setState(() {}),
                  expands: true,
                  minLines: null,
                  maxLines: null,
                  readOnly: _saving,
                  textAlignVertical: TextAlignVertical.top,
                  style: const TextStyle(
                    fontFamily: monoFontFamily,
                    fontSize: 13,
                  ),
                  decoration: InputDecoration(
                    labelText: context.tr('현재 파일 내용'),
                    alignLabelWithHint: true,
                    border: const OutlineInputBorder(),
                  ),
                ),
              );
            }
            return LayoutBuilder(
              builder: (context, constraints) {
                final original = _CodePanel(
                  title: context.tr('원본'),
                  content: file.original,
                  other: _editor.text,
                  isOriginal: true,
                );
                final changed = _CodePanel(
                  title: context.tr('현재'),
                  content: _editor.text,
                  other: file.original,
                  isOriginal: false,
                );
                return isTabletLayout(constraints)
                    ? Row(
                        children: [
                          Expanded(child: original),
                          Expanded(child: changed),
                        ],
                      )
                    : Column(
                        children: [
                          Expanded(child: original),
                          Expanded(child: changed),
                        ],
                      );
              },
            );
          },
        ),
      ),
    );
  }
}

class _CodePanel extends StatelessWidget {
  const _CodePanel({
    required this.title,
    required this.content,
    required this.other,
    required this.isOriginal,
  });
  final String title, content, other;
  final bool isOriginal;
  @override
  Widget build(BuildContext context) {
    final lines = content.split('\n');
    final otherLines = other.split('\n').toSet();
    final changeColor = isOriginal
        ? AppColors.statusError
        : AppColors.statusDone;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Padding(
          padding: const EdgeInsets.all(12),
          child: Text(title, style: Theme.of(context).textTheme.titleSmall),
        ),
        Expanded(
          child: Container(
            decoration: BoxDecoration(
              border: Border.all(color: context.colors.borderDefault),
            ),
            child: SingleChildScrollView(
              padding: const EdgeInsets.all(12),
              child: SelectionArea(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    for (var index = 0; index < lines.length; index++)
                      Container(
                        color: otherLines.contains(lines[index])
                            ? null
                            : changeColor.withValues(alpha: 0.12),
                        padding: const EdgeInsets.symmetric(vertical: 2),
                        child: Text(
                          '${index + 1}  ${lines[index]}',
                          style: TextStyle(
                            fontFamily: monoFontFamily,
                            fontSize: 13,
                            color: otherLines.contains(lines[index])
                                ? context.colors.textPrimary
                                : changeColor,
                          ),
                        ),
                      ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }
}
