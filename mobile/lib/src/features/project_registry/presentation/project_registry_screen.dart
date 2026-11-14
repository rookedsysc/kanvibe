import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_sizes.dart';
import '../../../common/network/desktop_connection.dart';
import '../application/project_registry_controller.dart';
import '../application/registered_projects_controller.dart';
import '../../../common/l10n/app_localizations.dart';
import 'registered_projects_screen.dart';
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
    final hosts = ref.watch(projectHostsProvider(widget.connection));
    ref.listen(provider.select((value) => (value.path, value.isLoading)), (
      _,
      selection,
    ) {
      _pathController.text = selection.$1;
    });

    return PopScope(
      canPop: !state.isScanning,
      child: Scaffold(
        appBar: AppBar(
          title: Text(context.tr('프로젝트 등록')),
          actions: [
            IconButton(
              tooltip: context.tr('등록된 프로젝트'),
              onPressed: state.isScanning
                  ? null
                  : () => Navigator.of(context).push(
                      MaterialPageRoute<void>(
                        builder: (_) => RegisteredProjectsScreen(
                          connection: widget.connection,
                        ),
                      ),
                    ),
              icon: const Icon(Icons.list_alt),
            ),
          ],
        ),
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
                        context.tr('{host}의 폴더', {
                          'host':
                              state.sshHost ?? widget.connection.displayName,
                        }),
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                      const SizedBox(height: AppSizes.p8),
                      Text(context.tr('선택한 폴더와 하위 폴더의 Git 저장소를 찾아 등록합니다.')),
                      const SizedBox(height: AppSizes.p16),
                      Semantics(
                        identifier: 'project-ssh-host',
                        child: DropdownButtonFormField<String>(
                          key: ValueKey(state.sshHost),
                          initialValue: state.sshHost ?? '',
                          isExpanded: true,
                          decoration: InputDecoration(
                            labelText: context.tr('저장소 위치'),
                          ),
                          items: [
                            DropdownMenuItem(
                              value: '',
                              child: Text(context.tr('이 PC의 로컬 폴더')),
                            ),
                            for (final host in hosts.value ?? <String>[])
                              DropdownMenuItem(value: host, child: Text(host)),
                          ],
                          onChanged: state.isScanning
                              ? null
                              : (value) => controller.selectHost(
                                  value == '' ? null : value,
                                ),
                        ),
                      ),
                      if (hosts.hasError)
                        TextButton(
                          onPressed: () => ref.invalidate(
                            projectHostsProvider(widget.connection),
                          ),
                          child: Text(context.tr('SSH 호스트 목록 다시 불러오기')),
                        ),
                      const SizedBox(height: AppSizes.p16),
                      Semantics(
                        identifier: 'project-path',
                        child: TextField(
                          key: const ValueKey('project-path'),
                          controller: _pathController,
                          enabled: !state.isBusy,
                          autocorrect: false,
                          textInputAction: TextInputAction.go,
                          decoration: InputDecoration(
                            labelText: context.tr('폴더 경로'),
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
                            label: Text(context.tr('상위 폴더')),
                          ),
                          const Spacer(),
                          TextButton(
                            onPressed: state.isBusy
                                ? null
                                : () => _openPath(_pathController.text),
                            child: Text(context.tr('경로 열기')),
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
                          child: Text(context.tr('다시 시도')),
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
                  SliverToBoxAdapter(
                    child: Padding(
                      padding: EdgeInsets.all(AppSizes.p16),
                      child: Text(
                        context.tr('하위 폴더가 없습니다. 이 폴더에서 등록할 수 있습니다.'),
                      ),
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
          context.tr('프로젝트 {count}개 등록', {'count': result.registered.length}),
          style: Theme.of(context).textTheme.titleMedium,
        ),
        for (final name in result.registered) Text(name),
        if (result.skipped.isNotEmpty)
          Text(
            context.tr('이미 등록된 프로젝트 {count}개', {
              'count': result.skipped.length,
            }),
          ),
        if (result.worktreeTasks.isNotEmpty)
          Text(
            context.tr('Worktree 태스크 {count}개 등록', {
              'count': result.worktreeTasks.length,
            }),
          ),
        if (result.registeredWorktrees.isNotEmpty)
          Text(
            context.tr('Worktree {count}개 등록', {
              'count': result.registeredWorktrees.length,
            }),
          ),
        if (result.isEmpty) Text(context.tr('이 폴더에서 Git 저장소를 찾지 못했습니다.')),
        for (final error in result.errors)
          Text(
            error,
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
      ],
    ),
  );
}
