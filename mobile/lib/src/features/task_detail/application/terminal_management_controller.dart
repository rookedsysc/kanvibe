import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../data/terminal_management_repository.dart';

final terminalManagementRepositoryProvider =
    Provider<TerminalManagementRepository>(
      (ref) => TerminalManagementRepository(ref.watch(desktopClientProvider)),
    );
final clipboardImageReaderProvider = Provider<ClipboardImageReader>(
  (ref) => const ClipboardImageReader(),
);
final terminalManagementProvider = NotifierProvider.autoDispose
    .family<TerminalManagementController, TerminalManagementState, String>(
      TerminalManagementController.new,
    );

class TerminalManagementState {
  const TerminalManagementState({this.isBusy = false, this.message});
  final bool isBusy;
  final String? message;
}

class TerminalManagementController extends Notifier<TerminalManagementState> {
  TerminalManagementController(this.taskId);
  final String taskId;
  @override
  TerminalManagementState build() {
    ref.watch(connectionControllerProvider);
    return const TerminalManagementState();
  }

  Future<void> _run(
    Future<({String message, bool didChange})> Function(
      DesktopConnection connection,
    )
    action,
  ) async {
    if (state.isBusy) return;
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (connection == null) {
      state = const TerminalManagementState(message: '연결된 PC가 없습니다.');
      return;
    }
    state = const TerminalManagementState(isBusy: true);
    try {
      final result = await action(connection);
      if (!_isCurrent(connection)) return;
      if (result.didChange) ref.invalidate(surfacesProvider(taskId));
      state = TerminalManagementState(message: result.message);
    } catch (_) {
      if (_isCurrent(connection)) {
        state = const TerminalManagementState(
          message: '요청을 처리하지 못했습니다. 다시 시도해 주세요.',
        );
      }
    }
  }

  bool _isCurrent(DesktopConnection connection) {
    if (!ref.mounted) return false;
    final active = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    return active?.id == connection.id && active?.token == connection.token;
  }

  Future<void> rename(String tabId, String name) => _run((connection) async {
    if (name.trim().isEmpty || name.trim().length > 128) {
      return (message: '탭 이름은 1자에서 128자 사이로 입력해 주세요.', didChange: false);
    }
    await ref
        .read(terminalManagementRepositoryProvider)
        .rename(connection, taskId, tabId, name.trim());
    return (message: '탭 이름을 변경했습니다.', didChange: true);
  });
  Future<void> move(String tabId, int index) => _run((connection) async {
    await ref
        .read(terminalManagementRepositoryProvider)
        .move(connection, taskId, tabId, index);
    return (message: '탭 순서를 변경했습니다.', didChange: true);
  });
  Future<void> pasteImage(String paneId) => _run((connection) async {
    final image = await ref.read(clipboardImageReaderProvider).read();
    if (image == null) return (message: '클립보드에 이미지가 없습니다.', didChange: false);
    if (!_isCurrent(connection)) return (message: '', didChange: false);
    await ref
        .read(terminalManagementRepositoryProvider)
        .pasteImage(connection, taskId, paneId, image);
    return (message: '이미지를 원격 터미널에 붙여넣었습니다.', didChange: true);
  });
}
