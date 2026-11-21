import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/providers.dart';
import '../../../common/network/desktop_client.dart';

class RunningAiPane {
  const RunningAiPane({
    required this.provider,
    required this.worktreePath,
    required this.windowId,
  });
  final String provider;
  final String worktreePath;
  final String windowId;
}

/// One request per board, shared by every task card. Backgrounding stops polling.
final runningAiPanesProvider = StreamProvider.autoDispose<List<RunningAiPane>>((
  ref,
) {
  final connection = ref
      .watch(connectionControllerProvider)
      .value
      ?.activeConnection;
  if (connection == null) return Stream.value(const []);
  final stream = StreamController<List<RunningAiPane>>();
  var busy = false;
  var disposed = false;
  Future<void> refresh() async {
    if (busy || disposed) return;
    busy = true;
    try {
      final result = await ref.read(desktopClientProvider).requestJson(
        connection,
        'GET',
        ['ai', 'running-panes'],
      );
      final panes = (result['panes'] as List? ?? [])
          .whereType<Map<String, dynamic>>()
          .where((p) => p['worktreePath'] is String && p['windowId'] is String)
          .map(
            (p) => RunningAiPane(
              provider: p['provider']?.toString() ?? '',
              worktreePath: p['worktreePath'] as String,
              windowId: p['windowId'] as String,
            ),
          )
          .toList();
      if (!disposed) stream.add(panes);
    } on DesktopRequestException catch (error, stack) {
      if (!disposed) {
        stream.addError(error, stack);
        if (error.isUnauthorized) {
          await ref
              .read(connectionControllerProvider.notifier)
              .forgetLocally(connection.id, connection.token);
        }
      }
    } catch (error, stack) {
      if (!disposed) stream.addError(error, stack);
    } finally {
      busy = false;
    }
  }

  final observer = _PollingLifecycle(refresh);
  WidgetsBinding.instance.addObserver(observer);
  observer.start();
  ref.onDispose(() {
    disposed = true;
    observer.stop();
    WidgetsBinding.instance.removeObserver(observer);
    unawaited(stream.close());
  });
  return stream.stream;
}, retry: (_, _) => null);

class _PollingLifecycle with WidgetsBindingObserver {
  _PollingLifecycle(this.refresh);
  final Future<void> Function() refresh;
  Timer? timer;
  void stop() {
    timer?.cancel();
    timer = null;
  }

  void start() {
    stop();
    final state = WidgetsBinding.instance.lifecycleState;
    if (state != null && state != AppLifecycleState.resumed) return;
    unawaited(refresh());
    timer = Timer.periodic(
      const Duration(seconds: 5),
      (_) => unawaited(refresh()),
    );
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      start();
    } else {
      stop();
    }
  }
}
