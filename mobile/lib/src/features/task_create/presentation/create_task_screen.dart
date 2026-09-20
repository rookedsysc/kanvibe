import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_sizes.dart';
import '../../../common/network/desktop_client.dart';
import '../../../common/providers.dart';
import '../../board/domain/board_project.dart';
import '../../board/domain/board_task.dart';
import '../../board/domain/task_priority.dart';
import '../domain/session_type.dart';

/// 태스크를 새로 만드는 화면.
///
/// 받는 칸과 순서는 데스크탑 생성 창을 그대로 따른다. 제목 칸이 없는 것도 데스크탑과 같다.
/// 그쪽은 브랜치 이름을 제목으로 쓰고, 두 화면이 다른 규칙으로 제목을 정하면
/// 같은 보드에서 어느 쪽으로 만들었는지에 따라 제목 모양이 갈린다.
///
/// 만들기에 성공하면 만들어진 태스크를 돌려주며 닫힌다. 데스크탑이 그렇듯 곧바로 그 태스크로 들어가기 위해서다.
class CreateTaskScreen extends ConsumerStatefulWidget {
  const CreateTaskScreen({required this.projects, super.key});

  /// 고를 수 있는 프로젝트. 보드가 이미 받아 둔 목록이라 여기서 다시 부르지 않는다
  final List<BoardProject> projects;

  @override
  ConsumerState<CreateTaskScreen> createState() => _CreateTaskScreenState();
}

class _CreateTaskScreenState extends ConsumerState<CreateTaskScreen> {
  final _formKey = GlobalKey<FormState>();
  final _branchNameController = TextEditingController();
  final _descriptionController = TextEditingController();

  BoardProject? _selectedProject;
  String? _baseBranch;
  TaskPriority? _priority;
  SessionType _sessionType = SessionType.tmux;

  bool _isCreating = false;
  String? _failureMessage;

  @override
  void dispose() {
    _branchNameController.dispose();
    _descriptionController.dispose();
    super.dispose();
  }

  /// 프로젝트를 고르면 베이스 브랜치도 그 프로젝트의 기본 브랜치로 되돌린다.
  /// 앞서 고른 브랜치를 그대로 두면 다른 저장소에 없는 이름이 남는다
  void _selectProject(BoardProject? project) {
    setState(() {
      _selectedProject = project;
      _baseBranch = project?.defaultBranch;
    });
  }

  Future<void> _create() async {
    final project = _selectedProject;
    if (project == null || !_formKey.currentState!.validate()) {
      return;
    }

    final connection = ref.read(connectionControllerProvider).value;
    if (connection == null) {
      return;
    }

    final navigator = Navigator.of(context);
    setState(() {
      _isCreating = true;
      _failureMessage = null;
    });

    final Map<String, dynamic> created;
    try {
      created = await ref
          .read(desktopClientProvider)
          .createTask(
            connection,
            projectId: project.id,
            branchName: _branchNameController.text.trim(),
            baseBranch: _baseBranch,
            description: _descriptionController.text.trim(),
            priority: _priority?.wireValue,
            sessionType: _sessionType.wireValue,
          );
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _isCreating = false;
        _failureMessage = _createFailureMessage(error);
      });
      return;
    }

    /// 보드에도 새 태스크가 보여야 돌아갔을 때 빈자리가 아니다
    ref.invalidate(boardProvider);

    navigator.pop(BoardTask.fromJson(created, {project.id: project.name}));
  }

  /// 데스크탑이 값을 거절한 것만 사유를 그대로 옮긴다.
  ///
  /// 그 밖의 실패 메시지에는 git 명령줄과 데스크탑의 경로가 섞여 있어 기기로 오지 않는다.
  /// 무엇이 잘못됐는지는 데스크탑에서만 볼 수 있으므로 그쪽을 보라고 말해 준다.
  String _createFailureMessage(Object error) {
    if (error is! DesktopRequestException) {
      return '데스크탑에 닿지 못했습니다. 잠시 뒤 다시 시도해 주세요.';
    }
    return error.message ?? '태스크를 만들지 못했습니다. 데스크탑에서 자세한 사유를 확인해 주세요.';
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('새 태스크')),
      body: SafeArea(
        child: widget.projects.isEmpty
            ? const _NoProjectGuide()
            : _buildForm(context),
      ),
    );
  }

  Widget _buildForm(BuildContext context) {
    return SingleChildScrollView(
      padding: const EdgeInsets.all(AppSizes.p24),
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 420),
        child: Form(
          key: _formKey,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              DropdownButtonFormField<BoardProject>(
                initialValue: _selectedProject,
                decoration: const InputDecoration(labelText: '프로젝트 *'),
                items: [
                  for (final project in widget.projects)
                    DropdownMenuItem(
                      value: project,
                      child: Text(project.name),
                    ),
                ],
                onChanged: _isCreating ? null : _selectProject,
                validator: (project) =>
                    project == null ? '프로젝트를 골라 주세요.' : null,
              ),
              const SizedBox(height: AppSizes.p16),
              _BaseBranchField(
                project: _selectedProject,
                selectedBranch: _baseBranch,
                isEnabled: !_isCreating,
                onChanged: (branch) => setState(() => _baseBranch = branch),
              ),
              const SizedBox(height: AppSizes.p16),
              TextFormField(
                controller: _branchNameController,
                enabled: !_isCreating,
                autocorrect: false,
                decoration: const InputDecoration(
                  labelText: '브랜치 이름 *',
                  helperText: '태스크 제목으로도 쓰입니다.',
                ),
                validator: (value) => value == null || value.trim().isEmpty
                    ? '브랜치 이름을 적어 주세요.'
                    : null,
              ),
              const SizedBox(height: AppSizes.p16),
              TextFormField(
                controller: _descriptionController,
                enabled: !_isCreating,
                maxLines: 3,
                decoration: const InputDecoration(labelText: '설명'),
              ),
              const SizedBox(height: AppSizes.p16),
              DropdownButtonFormField<TaskPriority?>(
                initialValue: _priority,
                decoration: const InputDecoration(labelText: '우선순위'),
                items: [
                  const DropdownMenuItem(value: null, child: Text('고르지 않음')),
                  for (final priority in TaskPriority.values)
                    DropdownMenuItem(
                      value: priority,
                      child: Text(priority.label),
                    ),
                ],
                onChanged: _isCreating
                    ? null
                    : (priority) => setState(() => _priority = priority),
              ),
              const SizedBox(height: AppSizes.p16),
              DropdownButtonFormField<SessionType>(
                initialValue: _sessionType,
                decoration: const InputDecoration(labelText: '세션 타입'),
                items: [
                  for (final sessionType in SessionType.values)
                    DropdownMenuItem(
                      value: sessionType,
                      child: Text(sessionType.wireValue),
                    ),
                ],
                onChanged: _isCreating
                    ? null
                    : (sessionType) =>
                          setState(() => _sessionType = sessionType!),
              ),
              if (_failureMessage != null) ...[
                const SizedBox(height: AppSizes.p16),
                Text(
                  _failureMessage!,
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: Theme.of(context).colorScheme.error,
                  ),
                ),
              ],
              const SizedBox(height: AppSizes.p24),
              FilledButton(
                onPressed: _isCreating ? null : _create,
                child: Text(_isCreating ? '만드는 중…' : '만들기'),
              ),
              const SizedBox(height: AppSizes.p8),
              Text(
                '데스크탑에 worktree와 터미널 세션이 함께 만들어져 조금 걸립니다.',
                textAlign: TextAlign.center,
                style: Theme.of(context).textTheme.labelSmall,
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// 베이스 브랜치 칸.
///
/// 프로젝트를 고르기 전에는 물어볼 것이 없어 비활성으로 둔다.
/// 목록을 못 받아도 칸을 닫지 않고 기본 브랜치 하나만 남긴다. 그 이름이면 대개 만들 수 있다.
class _BaseBranchField extends ConsumerWidget {
  const _BaseBranchField({
    required this.project,
    required this.selectedBranch,
    required this.isEnabled,
    required this.onChanged,
  });

  final BoardProject? project;
  final String? selectedBranch;
  final bool isEnabled;
  final ValueChanged<String?> onChanged;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final currentProject = project;
    if (currentProject == null) {
      return DropdownButtonFormField<String>(
        initialValue: null,
        decoration: const InputDecoration(labelText: '베이스 브랜치'),
        items: const [],
        onChanged: null,
      );
    }

    final branches = ref
        .watch(projectBranchesProvider(currentProject.id))
        .value;
    final options = {?selectedBranch, ...?branches}.toList();

    return DropdownButtonFormField<String>(
      initialValue: selectedBranch,
      decoration: const InputDecoration(labelText: '베이스 브랜치'),
      items: [
        for (final branch in options)
          DropdownMenuItem(value: branch, child: Text(branch)),
      ],
      onChanged: isEnabled ? onChanged : null,
    );
  }
}

/// 고를 프로젝트가 없다. 등록은 데스크탑에서만 할 수 있어 여기서는 안내만 한다
class _NoProjectGuide extends StatelessWidget {
  const _NoProjectGuide();

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(AppSizes.p24),
        child: Text(
          '등록된 프로젝트가 없습니다.\n데스크탑에서 프로젝트를 먼저 등록해 주세요.',
          textAlign: TextAlign.center,
          style: Theme.of(context).textTheme.bodyMedium,
        ),
      ),
    );
  }
}
