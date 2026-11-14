import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_sizes.dart';
import '../../../common/providers.dart';
import '../../board/domain/board_project.dart';
import '../../board/domain/task_priority.dart';
import '../../task_detail/presentation/task_detail_screen.dart';
import '../application/task_creation.dart';

/// 태스크를 새로 만드는 화면.
///
/// 받는 칸과 순서는 데스크탑 생성 창을 그대로 따른다. 제목 칸이 없는 것도 데스크탑과 같다.
/// 그쪽은 브랜치 이름을 제목으로 쓰고, 두 화면이 다른 규칙으로 제목을 정하면
/// 같은 보드에서 어느 쪽으로 만들었는지에 따라 제목 모양이 갈린다.
///
/// 만들기에 성공하면 만들어진 태스크를 돌려주며 닫힌다. 데스크탑이 그렇듯 곧바로 그 태스크로 들어가기 위해서다.
class CreateTaskScreen extends ConsumerStatefulWidget {
  const CreateTaskScreen({
    required this.projects,
    this.defaultProjectId,
    this.defaultBaseBranch,
    super.key,
  });

  /// 고를 수 있는 프로젝트. 보드가 이미 받아 둔 목록이라 여기서 다시 부르지 않는다
  final List<BoardProject> projects;

  final String? defaultProjectId;
  final String? defaultBaseBranch;

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

  bool _isCreating = false;

  /// 생성 응답이 늦어 보드를 다시 읽어 결과를 확인하는 동안
  bool _isConfirmingCreation = false;
  String? _failureMessage;

  @override
  void initState() {
    super.initState();
    final project = widget.projects
        .where((project) => project.id == widget.defaultProjectId)
        .firstOrNull;
    _selectedProject = project;
    if (project != null) {
      _baseBranch = widget.defaultBaseBranch;
      if (_baseBranch == null || _baseBranch!.isEmpty) {
        _baseBranch = project.defaultBranch;
      }
    }
  }

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

  /// 생성은 service가 끝까지 들고 있다. 화면을 먼저 나가면 결과를 앱 범위 안내로 알린다
  Future<void> _create() async {
    final project = _selectedProject;
    if (project == null || !_formKey.currentState!.validate()) {
      return;
    }

    final navigator = Navigator.of(context);
    final messenger = ScaffoldMessenger.of(context);
    final branchName = _branchNameController.text.trim();
    setState(() {
      _isCreating = true;
      _failureMessage = null;
    });

    final outcome = await ref
        .read(taskCreationServiceProvider)
        .create(
          project: project,
          branchName: branchName,
          baseBranch: _baseBranch,
          description: _descriptionController.text.trim(),
          priority: _priority?.wireValue,
          onConfirming: () {
            if (mounted) setState(() => _isConfirmingCreation = true);
          },
        );

    if (!mounted) {
      _announceOutcomeAfterLeaving(
        outcome,
        branchName: branchName,
        navigator: navigator,
        messenger: messenger,
      );
      return;
    }

    switch (outcome) {
      case TaskCreationSucceeded(:final task):
        navigator.pop(task);
      case TaskCreationFailed(:final message):
        setState(() {
          _isCreating = false;
          _isConfirmingCreation = false;
          _failureMessage = message;
        });
      case TaskCreationSkipped():
        setState(() => _isCreating = false);
    }
  }

  /// 화면을 나간 뒤 끝난 생성은 보드나 상세 위에 결과를 띄운다. 성공하면 바로 그 태스크로 들어갈 수 있게 한다
  void _announceOutcomeAfterLeaving(
    TaskCreationOutcome outcome, {
    required String branchName,
    required NavigatorState navigator,
    required ScaffoldMessengerState messenger,
  }) {
    if (!messenger.mounted) {
      return;
    }
    final snackBar = switch (outcome) {
      TaskCreationSucceeded(:final task) => SnackBar(
        content: Text('$branchName 태스크를 만들었습니다.'),
        action: SnackBarAction(
          label: '열기',
          onPressed: () => navigator.push(
            MaterialPageRoute<void>(
              builder: (_) => TaskDetailScreen(task: task),
            ),
          ),
        ),
      ),
      TaskCreationUnconfirmed(:final message) => SnackBar(
        content: Text(message),
      ),
      TaskCreationFailed(:final message) => SnackBar(
        content: Text('$branchName 태스크를 만들지 못했습니다: $message'),
      ),
      TaskCreationSkipped() => null,
    };
    if (snackBar != null) {
      messenger.showSnackBar(snackBar);
    }
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
      child: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 420),
          child: Form(
            key: _formKey,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                DropdownButtonFormField<BoardProject>(
                  isExpanded: true,
                  initialValue: _selectedProject,
                  decoration: const InputDecoration(labelText: '프로젝트 *'),
                  items: [
                    for (final project in widget.projects)
                      DropdownMenuItem(
                        value: project,
                        child: Text(
                          project.name,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
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
                if (_failureMessage != null) ...[
                  const SizedBox(height: AppSizes.p16),
                  Text(
                    _failureMessage!,
                    style: Theme.of(context).textTheme.bodySmall
                        ?.copyWith(color: Theme.of(context).colorScheme.error),
                  ),
                ],
                const SizedBox(height: AppSizes.p24),
                FilledButton(
                  onPressed: _isCreating ? null : _create,
                  child: Text(
                    _isConfirmingCreation
                        ? '결과 확인 중…'
                        : _isCreating
                        ? '만드는 중…'
                        : '만들기',
                  ),
                ),
                const SizedBox(height: AppSizes.p8),
                Text(
                  _isCreating
                      ? '화면을 나가도 생성은 계속되고, 끝나면 알려 드립니다.'
                      : '데스크탑에 worktree와 터미널 세션이 함께 만들어져 조금 걸립니다.',
                  textAlign: TextAlign.center,
                  style: Theme.of(context).textTheme.labelSmall,
                ),
              ],
            ),
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
/// 다만 다른 브랜치가 없는 것처럼 보이지 않도록 조회 중과 조회 실패를 칸 아래에 따로 알린다.
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
        isExpanded: true,
        initialValue: null,
        decoration: const InputDecoration(labelText: '베이스 브랜치'),
        items: const [],
        onChanged: null,
      );
    }

    final branchesProvider = projectBranchesProvider(currentProject.id);
    final branchList = ref.watch(branchesProvider);
    final options = {?selectedBranch, ...?branchList.value}.toList();
    final hasBranchListFailed = branchList.hasError && !branchList.isLoading;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: MainAxisSize.min,
      children: [
        DropdownButtonFormField<String>(
          isExpanded: true,
          initialValue: selectedBranch,
          decoration: InputDecoration(
            labelText: '베이스 브랜치',
            helperText: branchList.isLoading ? '브랜치 목록을 불러오는 중…' : null,
            errorText: hasBranchListFailed
                ? '브랜치 목록을 불러오지 못해 기본 브랜치만 보입니다.'
                : null,
          ),
          items: [
            for (final branch in options)
              DropdownMenuItem(
                value: branch,
                child: Text(
                  branch,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
          ],
          onChanged: isEnabled ? onChanged : null,
        ),
        if (hasBranchListFailed)
          Align(
            alignment: Alignment.centerRight,
            child: TextButton(
              onPressed: () => ref.invalidate(branchesProvider),
              child: const Text('다시 불러오기'),
            ),
          ),
      ],
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
