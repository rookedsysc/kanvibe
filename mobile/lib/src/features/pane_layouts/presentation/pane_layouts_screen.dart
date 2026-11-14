import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/l10n/app_localizations.dart';
import '../../../common/providers.dart';
import '../../../common/network/desktop_connection.dart';
import '../../board/domain/board_project.dart';
import '../application/pane_layout_controller.dart';
import '../domain/pane_layout.dart';

class PaneLayoutsScreen extends ConsumerStatefulWidget {
  const PaneLayoutsScreen({super.key});
  @override
  ConsumerState<PaneLayoutsScreen> createState() => _PaneLayoutsScreenState();
}

class _PaneLayoutsScreenState extends ConsumerState<PaneLayoutsScreen> {
  String? _projectId;
  String _loadedScope = '';
  PaneLayout? _loadedLayout;
  PaneLayoutKind _kind = PaneLayoutKind.single;
  final _commands = List.generate(4, (_) => TextEditingController());
  bool _busy = false;
  String? _failure;
  DesktopConnection? _lastConnection;
  int _connectionGeneration = 0;
  @override
  void dispose() {
    for (final controller in _commands) {
      controller.dispose();
    }
    super.dispose();
  }

  Future<void> _save() async {
    if (_busy) return;
    final generation = _connectionGeneration;
    setState(() {
      _busy = true;
      _failure = null;
    });
    try {
      final saved = await ref
          .read(paneLayoutsProvider.notifier)
          .save(
            _kind,
            _commands
                .take(_kind.paneCount)
                .map((controller) => controller.text)
                .toList(),
            _projectId,
          );
      if (!mounted || generation != _connectionGeneration) return;
      if (saved) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(context.tr('레이아웃을 저장했습니다. 새 세션에 적용됩니다.'))),
        );
        _loadedScope = '';
      } else {
        _failure = '레이아웃을 저장하지 못했습니다.';
      }
    } catch (_) {
      if (generation == _connectionGeneration) {
        _failure = '레이아웃을 저장하지 못했습니다.';
      }
    } finally {
      if (mounted && generation == _connectionGeneration) {
        setState(() => _busy = false);
      }
    }
  }

  Future<void> _delete(PaneLayout layout) async {
    final generation = _connectionGeneration;
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(context.tr('레이아웃 설정을 삭제할까요?')),
        content: Text(context.tr('프로젝트 설정을 삭제하면 글로벌 기본값을 사용합니다.')),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: Text(context.tr('취소')),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(context.tr('삭제')),
          ),
        ],
      ),
    );
    if (!mounted || confirmed != true || generation != _connectionGeneration) {
      return;
    }
    setState(() => _busy = true);
    try {
      await ref.read(paneLayoutsProvider.notifier).delete(layout.id);
      if (generation != _connectionGeneration) return;
      _loadedScope = '';
    } catch (_) {
      if (generation == _connectionGeneration) _failure = '레이아웃을 삭제하지 못했습니다.';
    } finally {
      if (mounted && generation == _connectionGeneration) {
        setState(() => _busy = false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final layouts = ref.watch(paneLayoutsProvider);
    final board = ref.watch(boardProvider);
    final connection = ref
        .watch(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (connection?.id != _lastConnection?.id ||
        connection?.token != _lastConnection?.token) {
      _connectionGeneration++;
      _busy = false;
      _failure = null;
      _projectId = null;
      _lastConnection = connection;
      _loadedScope = '';
    }
    final scope = '${connection?.id}:${connection?.token}:$_projectId';
    final projects = board.value?.creatableProjects ?? const <BoardProject>[];
    final value = layouts.value
        ?.where(
          (layout) => _projectId == null
              ? layout.isGlobal
              : layout.projectId == _projectId,
        )
        .firstOrNull;
    if (layouts.hasValue &&
        (_loadedScope != scope || !identical(_loadedLayout, value))) {
      _loadedScope = scope;
      _loadedLayout = value;
      _kind = value?.kind ?? PaneLayoutKind.single;
      for (var index = 0; index < _commands.length; index++) {
        _commands[index].text = index < (value?.commands.length ?? 0)
            ? value!.commands[index]
            : '';
      }
    }
    return Scaffold(
      appBar: AppBar(title: Text(context.tr('Pane 레이아웃'))),
      body: SafeArea(
        child: layouts.when(
          loading: () => const Center(child: CircularProgressIndicator()),
          error: (_, _) => Center(
            child: FilledButton(
              onPressed: () => ref.invalidate(paneLayoutsProvider),
              child: Text(context.tr('레이아웃 다시 조회')),
            ),
          ),
          data: (_) => ListView(
            padding: const EdgeInsets.all(16),
            children: [
              Text(context.tr('글로벌 기본값과 프로젝트별 시작 명령을 설정합니다. 기존 세션은 유지됩니다.')),
              const SizedBox(height: 20),
              DropdownButtonFormField<String>(
                key: ValueKey(scope),
                initialValue: _projectId ?? '',
                decoration: InputDecoration(labelText: context.tr('적용 대상')),
                items: [
                  DropdownMenuItem(
                    value: '',
                    child: Text(context.tr('글로벌 기본값')),
                  ),
                  for (final project in projects)
                    DropdownMenuItem(
                      value: project.id,
                      child: Text(project.name),
                    ),
                ],
                onChanged: _busy
                    ? null
                    : (id) => setState(() {
                        _projectId = id == '' ? null : id;
                        _loadedScope = '';
                      }),
              ),
              const SizedBox(height: 16),
              DropdownButtonFormField<PaneLayoutKind>(
                key: ValueKey('kind-$scope-$_loadedScope'),
                initialValue: _kind,
                decoration: InputDecoration(labelText: context.tr('분할 배치')),
                items: [
                  for (final kind in PaneLayoutKind.values)
                    DropdownMenuItem(
                      value: kind,
                      child: Text(context.tr(kind.label)),
                    ),
                ],
                onChanged: _busy
                    ? null
                    : (kind) =>
                          setState(() => _kind = kind ?? PaneLayoutKind.single),
              ),
              const SizedBox(height: 16),
              for (var index = 0; index < _kind.paneCount; index++)
                Padding(
                  padding: const EdgeInsets.only(bottom: 12),
                  child: TextField(
                    key: ValueKey('pane-command-$index'),
                    controller: _commands[index],
                    enabled: !_busy,
                    maxLength: 4096,
                    decoration: InputDecoration(
                      labelText: context.tr('Pane {number} 시작 명령', {
                        'number': index + 1,
                      }),
                      helperText: context.tr('비워 두면 기본 셸을 엽니다.'),
                      border: const OutlineInputBorder(),
                    ),
                  ),
                ),
              if (_failure != null) Text(context.tr(_failure!)),
              FilledButton.icon(
                key: const ValueKey('save-pane-layout'),
                onPressed: _busy ? null : _save,
                icon: _busy
                    ? const SizedBox(
                        width: 16,
                        height: 16,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Icon(Icons.save_outlined),
                label: Text(context.tr('레이아웃 저장')),
              ),
              if (value != null)
                TextButton.icon(
                  onPressed: _busy ? null : () => _delete(value),
                  icon: const Icon(Icons.delete_outline),
                  label: Text(context.tr('레이아웃 설정 삭제')),
                ),
            ],
          ),
        ),
      ),
    );
  }
}
