import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_sizes.dart';
import '../../../common/network/desktop_connection.dart';
import '../application/project_registry_controller.dart';
import '../data/project_scan_result.dart';

class ProjectRegistryScreen extends ConsumerStatefulWidget {
  const ProjectRegistryScreen({required this.connection, super.key});

  final DesktopConnection connection;

  @override
  ConsumerState<ProjectRegistryScreen> createState() =>
      _ProjectRegistryScreenState();
}

class _ProjectRegistryScreenState extends ConsumerState<ProjectRegistryScreen> {
  final _pathController = TextEditingController(text: '~');

  @override
  void initState() {
    super.initState();
    Future.microtask(() {
      if (mounted) {
        ref
            .read(projectRegistryProvider(widget.connection).notifier)
            .browse('~');
      }
    });
  }

  @override
  void dispose() {
    _pathController.dispose();
    super.dispose();
  }

  Future<void> _openPath(String path) {
    FocusScope.of(context).unfocus();
    return ref
        .read(projectRegistryProvider(widget.connection).notifier)
        .browse(path);
  }

  @override
  Widget build(BuildContext context) {
    final provider = projectRegistryProvider(widget.connection);
    final state = ref.watch(provider);
    final controller = ref.read(provider.notifier);
    ref.listen(provider.select((value) => (value.path, value.isLoading)), (
      _,
      selection,
    ) {
      _pathController.text = selection.$1;
    });

    return PopScope(
      canPop: !state.isScanning,
      child: Scaffold(
        appBar: AppBar(title: const Text('프로젝트 등록')),
        bottomNavigationBar: SafeArea(
          child: Padding(
            padding: const EdgeInsets.all(AppSizes.p16),
            child: Semantics(
              identifier: 'project-scan',
              child: FilledButton.icon(
                key: const ValueKey('project-scan'),
                onPressed:
                    state.isBusy ||
                        !state.hasDirectory ||
                        _pathController.text != state.path
                    ? null
                    : controller.scan,
                icon: state.isScanning
                    ? const SizedBox(
                        width: 20,
                        height: 20,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.create_new_folder_outlined),
                label: Text(state.isScanning ? '프로젝트 등록 중…' : '이 폴더에서 프로젝트 등록'),
              ),
            ),
          ),
        ),
        body: SafeArea(
          child: CustomScrollView(
            slivers: [
              SliverToBoxAdapter(
                child: Padding(
                  padding: const EdgeInsets.all(AppSizes.p16),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Text(
                        '${widget.connection.displayName}의 폴더',
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                      const SizedBox(height: AppSizes.p8),
                      const Text('선택한 폴더와 하위 폴더의 Git 저장소를 찾아 등록합니다.'),
                      const SizedBox(height: AppSizes.p16),
                      Semantics(
                        identifier: 'project-path',
                        child: TextField(
                          key: const ValueKey('project-path'),
                          controller: _pathController,
                          enabled: !state.isBusy,
                          autocorrect: false,
                          textInputAction: TextInputAction.go,
                          decoration: const InputDecoration(
                            labelText: '폴더 경로',
                            hintText: '~/Documents',
                          ),
                          onSubmitted: _openPath,
                          onChanged: (_) => setState(() {}),
                        ),
                      ),
                      const SizedBox(height: AppSizes.p8),
                      Row(
                        children: [
                          TextButton.icon(
                            onPressed: state.isBusy || !state.canGoUp
                                ? null
                                : controller.goUp,
                            icon: const Icon(Icons.arrow_upward),
                            label: const Text('상위 폴더'),
                          ),
                          const Spacer(),
                          TextButton(
                            onPressed: state.isBusy
                                ? null
                                : () => _openPath(_pathController.text),
                            child: const Text('경로 열기'),
                          ),
                        ],
                      ),
                      if (state.failure != null) ...[
                        Text(
                          state.failure!,
                          style: TextStyle(
                            color: Theme.of(context).colorScheme.error,
                          ),
                        ),
                        TextButton(
                          onPressed: state.isBusy
                              ? null
                              : () => controller.browse(state.path),
                          child: const Text('다시 시도'),
                        ),
                      ],
                    ],
                  ),
                ),
              ),
              if (state.isLoading)
                const SliverFillRemaining(
                  hasScrollBody: false,
                  child: Center(child: CircularProgressIndicator()),
                )
              else ...[
                if (state.result case final result?)
                  SliverPadding(
                    padding: const EdgeInsets.symmetric(
                      horizontal: AppSizes.p16,
                    ),
                    sliver: SliverToBoxAdapter(
                      child: _ScanResult(result: result),
                    ),
                  ),
                if (state.hasDirectory && state.directories.isEmpty)
                  const SliverToBoxAdapter(
                    child: Padding(
                      padding: EdgeInsets.all(AppSizes.p16),
                      child: Text('하위 폴더가 없습니다. 이 폴더에서 등록할 수 있습니다.'),
                    ),
                  ),
                SliverPadding(
                  padding: const EdgeInsets.symmetric(horizontal: AppSizes.p16),
                  sliver: SliverList.builder(
                    itemCount: state.directories.length,
                    itemBuilder: (context, index) {
                      final name = state.directories[index];
                      return Semantics(
                        identifier: 'project-folder-$name',
                        child: ListTile(
                          enabled: !state.isBusy,
                          contentPadding: EdgeInsets.zero,
                          leading: const Icon(Icons.folder_outlined),
                          title: Text(name),
                          trailing: const Icon(Icons.chevron_right),
                          onTap: () => controller.openDirectory(name),
                        ),
                      );
                    },
                  ),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }
}

class _ScanResult extends StatelessWidget {
  const _ScanResult({required this.result});

  final ProjectScanResult result;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.symmetric(vertical: AppSizes.p12),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          '프로젝트 ${result.registered.length}개 등록',
          style: Theme.of(context).textTheme.titleMedium,
        ),
        for (final name in result.registered) Text(name),
        if (result.skipped.isNotEmpty)
          Text('이미 등록된 프로젝트 ${result.skipped.length}개'),
        if (result.worktreeTasks.isNotEmpty)
          Text('Worktree 태스크 ${result.worktreeTasks.length}개 등록'),
        if (result.registeredWorktrees.isNotEmpty)
          Text('Worktree ${result.registeredWorktrees.length}개 등록'),
        if (result.isEmpty) const Text('이 폴더에서 Git 저장소를 찾지 못했습니다.'),
        for (final error in result.errors)
          Text(
            error,
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
      ],
    ),
  );
}
