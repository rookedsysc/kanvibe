import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../data/project_scan_result.dart';

final projectRegistryProvider = NotifierProvider.autoDispose
    .family<ProjectRegistryController, ProjectRegistryState, DesktopConnection>(
      ProjectRegistryController.new,
    );

class ProjectRegistryState {
  const ProjectRegistryState({
    this.path = '~',
    this.directories = const [],
    this.isLoading = false,
    this.isScanning = false,
    this.hasDirectory = false,
    this.failure,
    this.result,
  });

  final String path;
  final List<String> directories;
  final bool isLoading;
  final bool isScanning;
  final bool hasDirectory;
  final String? failure;
  final ProjectScanResult? result;

  bool get isBusy => isLoading || isScanning;
  bool get canGoUp => path != '~' && path != '/';
}

class ProjectRegistryController extends Notifier<ProjectRegistryState> {
  ProjectRegistryController(this.connection);

  final DesktopConnection connection;

  @override
  ProjectRegistryState build() => const ProjectRegistryState();

  Future<void> browse(String path) async {
    if (state.isBusy) return;
    final normalized = path.trim().replaceFirst(RegExp(r'/+$'), '');
    final target = normalized.isEmpty && path.trim().startsWith('/')
        ? '/'
        : normalized;
    if (target != '~' && !target.startsWith('~/') && !target.startsWith('/')) {
      state = ProjectRegistryState(
        path: state.path,
        directories: state.directories,
        hasDirectory: state.hasDirectory,
        failure: '절대 경로 또는 ~/로 시작하는 경로를 입력해 주세요.',
      );
      return;
    }
    state = ProjectRegistryState(path: target, isLoading: true);
    try {
      final directories = await ref
          .read(desktopClientProvider)
          .fetchProjectDirectories(connection, target);
      if (!ref.mounted) return;
      state = ProjectRegistryState(
        path: target,
        directories: directories,
        hasDirectory: true,
      );
    } catch (error) {
      if (!ref.mounted) return;
      state = ProjectRegistryState(
        path: target,
        failure: _failureMessage(error),
      );
    }
  }

  Future<void> openDirectory(String name) =>
      browse('${state.path == '/' ? '' : state.path}/$name');

  Future<void> goUp() async {
    if (!state.canGoUp) return;
    final index = state.path.lastIndexOf('/');
    await browse(index == 0 ? '/' : state.path.substring(0, index));
  }

  Future<void> scan() async {
    final previous = state;
    if (previous.isBusy || !previous.hasDirectory) return;
    state = ProjectRegistryState(
      path: previous.path,
      directories: previous.directories,
      hasDirectory: true,
      isScanning: true,
    );
    try {
      final result = await ref
          .read(desktopClientProvider)
          .scanProjects(connection, previous.path);
      if (!ref.mounted) return;
      ref.invalidate(boardProvider);
      state = ProjectRegistryState(
        path: previous.path,
        directories: previous.directories,
        hasDirectory: true,
        result: result,
      );
    } catch (error) {
      if (!ref.mounted) return;
      state = ProjectRegistryState(
        path: previous.path,
        directories: previous.directories,
        hasDirectory: true,
        failure: _failureMessage(error),
      );
    }
  }

  String _failureMessage(Object error) {
    if (error is DesktopRequestException) {
      if (error.statusCode == 404) return '프로젝트 등록을 사용하려면 데스크톱 앱을 업데이트해 주세요.';
      if (error.isUnauthorized) return 'PC 연결 인증이 만료되었습니다. PC를 다시 연결해 주세요.';
      if (error.statusCode == 400) {
        return '폴더를 열 수 없습니다. 경로와 PC의 접근 권한을 확인해 주세요.';
      }
    }
    return 'PC에 요청하지 못했습니다. 연결을 확인하고 다시 시도해 주세요.';
  }
}
