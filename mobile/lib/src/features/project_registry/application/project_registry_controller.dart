import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../data/project_scan_result.dart';
import 'registered_projects_controller.dart';

final projectRegistryProvider = NotifierProvider.autoDispose
    .family<ProjectRegistryController, ProjectRegistryState, DesktopConnection>(
      ProjectRegistryController.new,
    );

class ProjectRegistryState {
  const ProjectRegistryState({
    this.path = '~',
    this.sshHost,
    this.directories = const [],
    this.isLoading = false,
    this.isScanning = false,
    this.hasDirectory = false,
    this.failure,
    this.result,
  });

  final String path;
  final String? sshHost;
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
  int _requestGeneration = 0;

  @override
  ProjectRegistryState build() => const ProjectRegistryState();

  Future<void> selectHost(String? sshHost) async {
    if (state.isScanning) return;
    _requestGeneration++;
    state = ProjectRegistryState(sshHost: sshHost);
    await browse('~');
  }

  bool _accepts(int generation) =>
      ref.mounted && generation == _requestGeneration;
  void _requireCurrentConnection() {
    final active = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (active?.id != connection.id || active?.token != connection.token) {
      throw StateError('선택한 PC가 변경되었습니다. 프로젝트 화면을 다시 열어 주세요.');
    }
  }

  Future<void> browse(String path) async {
    if (state.isBusy) return;
    final sshHost = state.sshHost;
    final normalized = path.trim().replaceFirst(RegExp(r'/+$'), '');
    final target = normalized.isEmpty && path.trim().startsWith('/')
        ? '/'
        : normalized;
    if (target != '~' && !target.startsWith('~/') && !target.startsWith('/')) {
      state = ProjectRegistryState(
        path: state.path,
        sshHost: sshHost,
        directories: state.directories,
        hasDirectory: state.hasDirectory,
        failure: '절대 경로 또는 ~/로 시작하는 경로를 입력해 주세요.',
      );
      return;
    }
    final generation = ++_requestGeneration;
    state = ProjectRegistryState(
      path: target,
      sshHost: sshHost,
      isLoading: true,
    );
    try {
      await ref.read(connectionControllerProvider.future);
      if (!_accepts(generation)) return;
      _requireCurrentConnection();
      final directories = await ref
          .read(projectRegistryRepositoryProvider)
          .browse(connection, target, sshHost);
      if (!_accepts(generation)) return;
      _requireCurrentConnection();
      state = ProjectRegistryState(
        path: target,
        sshHost: sshHost,
        directories: directories,
        hasDirectory: true,
      );
    } catch (error) {
      if (!_accepts(generation)) return;
      state = ProjectRegistryState(
        path: target,
        sshHost: sshHost,
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
    final generation = ++_requestGeneration;
    state = ProjectRegistryState(
      path: previous.path,
      sshHost: previous.sshHost,
      directories: previous.directories,
      hasDirectory: true,
      isScanning: true,
    );
    try {
      _requireCurrentConnection();
      final result = await ref
          .read(projectRegistryRepositoryProvider)
          .scan(connection, previous.path, previous.sshHost);
      if (!_accepts(generation)) return;
      _requireCurrentConnection();
      ref.invalidate(boardProvider);
      state = ProjectRegistryState(
        path: previous.path,
        sshHost: previous.sshHost,
        directories: previous.directories,
        hasDirectory: true,
        result: result,
      );
    } catch (error) {
      if (!_accepts(generation)) return;
      state = ProjectRegistryState(
        path: previous.path,
        sshHost: previous.sshHost,
        directories: previous.directories,
        hasDirectory: true,
        failure: _failureMessage(error),
      );
    }
  }

  String _failureMessage(Object error) {
    if (error is StateError) return error.message.toString();
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
