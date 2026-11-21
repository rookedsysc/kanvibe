import '../../../common/providers.dart';
import '../../../common/network/desktop_connection.dart';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_sizes.dart';
import '../../../common/l10n/app_localizations.dart';
import '../../board/domain/board_task.dart';
import '../../board/domain/task_priority.dart';
import '../application/task_details_controller.dart';

class TaskEditorScreen extends ConsumerStatefulWidget {
  const TaskEditorScreen({required this.task, super.key});
  final BoardTask task;
  @override
  ConsumerState<TaskEditorScreen> createState() => _TaskEditorScreenState();
}

class _TaskEditorScreenState extends ConsumerState<TaskEditorScreen> {
  final _formKey = GlobalKey<FormState>();
  late final _description = TextEditingController(
    text: widget.task.description,
  );
  late TaskPriority? _priority = widget.task.priority;
  bool _isSaving = false;
  late String _projectColor = widget.task.projectColor ?? '#0064FF';
  String? _error;
  DesktopConnection? _editingConnection;
  @override
  void initState() {
    super.initState();
    _editingConnection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
  }

  bool _isSameConnection() {
    final active = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    return _editingConnection != null &&
        active?.id == _editingConnection!.id &&
        active?.token == _editingConnection!.token;
  }

  bool _canSave() {
    if (_isSameConnection()) return true;
    if (mounted) {
      setState(() => _error = context.tr('PC 연결이 변경되었습니다. 화면을 닫고 다시 열어 주세요.'));
    }
    return false;
  }

  @override
  void dispose() {
    _description.dispose();
    super.dispose();
  }

  Future<void> _save() async {
    if (_isSaving || !_canSave() || !_formKey.currentState!.validate()) return;
    setState(() {
      _isSaving = true;
      _error = null;
    });
    try {
      final task = await ref
          .read(taskDetailsProvider(widget.task.id).notifier)
          .save(
            description: _description.text,
            priority: _priority?.wireValue,
            expectedConnection: _editingConnection,
          );
      if (mounted && task != null) Navigator.of(context).pop(task);
    } catch (_) {
      if (mounted) {
        setState(() => _error = context.tr('정보를 저장하지 못했습니다. 다시 시도해 주세요.'));
      }
    } finally {
      if (mounted) setState(() => _isSaving = false);
    }
  }

  Future<void> _editColor() async {
    final projectId = widget.task.projectId;
    if (projectId == null || !_canSave()) return;
    var value = _projectColor;
    final colorForm = GlobalKey<FormState>();
    final color = await showDialog<String>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: Text(context.tr('프로젝트 색상')),
        content: Form(
          key: colorForm,
          child: TextFormField(
            initialValue: value,
            onChanged: (text) => value = text,
            maxLength: 7,
            decoration: const InputDecoration(
              hintText: '#0064FF',
              errorMaxLines: 2,
            ),
            validator: (text) =>
                RegExp(r'^#[0-9a-fA-F]{6}$').hasMatch(text ?? '')
                ? null
                : context.tr('#RRGGBB 형식으로 입력해 주세요.'),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(dialogContext),
            child: Text(context.tr('취소')),
          ),
          TextButton(
            onPressed: () {
              if (colorForm.currentState!.validate()) {
                Navigator.pop(dialogContext, value);
              }
            },
            child: Text(context.tr('저장')),
          ),
        ],
      ),
    );
    if (color == null || !mounted || !_canSave()) return;
    setState(() => _isSaving = true);
    try {
      final saved = await ref
          .read(taskDetailsProvider(widget.task.id).notifier)
          .setProjectColor(
            projectId,
            color,
            expectedConnection: _editingConnection,
          );
      if (mounted && saved && _isSameConnection()) {
        setState(() => _projectColor = color);
      }
    } catch (_) {
      if (mounted) setState(() => _error = context.tr('색상을 저장하지 못했습니다.'));
    } finally {
      if (mounted) setState(() => _isSaving = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    ref.watch(taskDetailsProvider(widget.task.id));
    return Scaffold(
      appBar: AppBar(title: Text(context.tr('정보 수정'))),
      body: SafeArea(
        child: Form(
          key: _formKey,
          child: ListView(
            padding: const EdgeInsets.all(AppSizes.p24),
            children: [
              Text(
                widget.task.title,
                style: Theme.of(context).textTheme.titleMedium,
              ),
              const SizedBox(height: AppSizes.p16),
              TextFormField(
                controller: _description,
                enabled: !_isSaving,
                minLines: 4,
                maxLines: 10,
                maxLength: 16000,
                decoration: InputDecoration(labelText: context.tr('설명')),
              ),
              const SizedBox(height: AppSizes.p16),
              DropdownButtonFormField<TaskPriority?>(
                initialValue: _priority,
                decoration: InputDecoration(labelText: context.tr('우선순위')),
                items: [
                  DropdownMenuItem(
                    value: null,
                    child: Text(context.tr('고르지 않음')),
                  ),
                  for (final p in TaskPriority.values)
                    DropdownMenuItem(
                      value: p,
                      child: Text(context.tr(p.label)),
                    ),
                ],
                onChanged: _isSaving
                    ? null
                    : (value) => setState(() => _priority = value),
              ),
              if (widget.task.projectId != null)
                TextButton.icon(
                  onPressed: _isSaving ? null : _editColor,
                  icon: Icon(
                    Icons.palette_outlined,
                    color: Color(
                      int.parse(_projectColor.substring(1), radix: 16) |
                          0xff000000,
                    ),
                  ),
                  label: Text(context.tr('프로젝트 색상')),
                ),
              const SizedBox(height: AppSizes.p16),
              if (_error != null)
                Padding(
                  padding: const EdgeInsets.only(bottom: AppSizes.p12),
                  child: Text(
                    _error!,
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                    ),
                  ),
                ),
              FilledButton(
                onPressed: _isSaving ? null : _save,
                child: Text(context.tr(_isSaving ? '저장하는 중…' : '저장')),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
