import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../../board/domain/board_project.dart';
import '../data/project_registry_repository.dart';

final projectRegistryRepositoryProvider = Provider<ProjectRegistryRepository>(
  (ref) => ProjectRegistryRepository(ref.watch(desktopClientProvider)),
);
final projectHostsProvider = FutureProvider.autoDispose
    .family<List<String>, DesktopConnection>(
      (ref, connection) =>
          ref.read(projectRegistryRepositoryProvider).fetchHosts(connection),
      retry: (_, _) => null,
    );
final registeredProjectsProvider = AsyncNotifierProvider.autoDispose
    .family<
      RegisteredProjectsController,
      RegisteredProjectsState,
      DesktopConnection
    >(RegisteredProjectsController.new, retry: (_, _) => null);

class RegisteredProjectsState {
  const RegisteredProjectsState(this.projects, {this.deletingId, this.error});
  final List<BoardProject> projects;
  final String? deletingId;
  final Object? error;
}

class RegisteredProjectsController
    extends AsyncNotifier<RegisteredProjectsState> {
  RegisteredProjectsController(this.connection);
  final DesktopConnection connection;
  int _generation = 0;
  bool _current(int generation) {
    final active = ref.mounted
        ? ref.read(connectionControllerProvider).value?.activeConnection
        : null;
    return ref.mounted &&
        generation == _generation &&
        active?.id == connection.id &&
        active?.token == connection.token;
  }

  @override
  Future<RegisteredProjectsState> build() async {
    final generation = ++_generation;
    await ref.watch(connectionControllerProvider.future);
    if (!_current(generation)) throw StateError('선택한 PC가 변경되었습니다.');
    try {
      return RegisteredProjectsState(
        await ref
            .read(projectRegistryRepositoryProvider)
            .fetchProjects(connection),
      );
    } catch (error) {
      await _forget(error);
      rethrow;
    }
  }

  Future<void> _forget(Object error) async {
    if (error is DesktopRequestException &&
        error.isUnauthorized &&
        _current(_generation)) {
      await ref
          .read(connectionControllerProvider.notifier)
          .forgetLocally(connection.id, connection.token);
    }
  }

  Future<bool> remove(String projectId) async {
    final previous = state.value;
    final generation = _generation;
    if (state.isLoading ||
        previous == null ||
        previous.deletingId != null ||
        !_current(generation) ||
        !previous.projects.any((project) => project.id == projectId)) {
      return false;
    }
    state = AsyncData(
      RegisteredProjectsState(previous.projects, deletingId: projectId),
    );
    try {
      await ref
          .read(projectRegistryRepositoryProvider)
          .remove(connection, projectId);
      if (!_current(generation)) return false;
      state = AsyncData(
        RegisteredProjectsState(
          List.unmodifiable(
            previous.projects.where((project) => project.id != projectId),
          ),
        ),
      );
      ref.invalidate(boardProvider);
      return true;
    } catch (error) {
      if (!_current(generation)) return false;
      state = AsyncData(
        RegisteredProjectsState(previous.projects, error: error),
      );
      await _forget(error);
      return false;
    }
  }
}
