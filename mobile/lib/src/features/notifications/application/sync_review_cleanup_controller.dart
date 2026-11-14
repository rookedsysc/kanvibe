import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';

final syncReviewCleanupProvider =
    NotifierProvider<SyncReviewCleanupController, SyncReviewCleanupJob?>(
      SyncReviewCleanupController.new,
    );

class SyncReviewCleanupJob {
  const SyncReviewCleanupJob(this.notificationId);
  final String notificationId;
}

class SyncReviewCleanupController extends Notifier<SyncReviewCleanupJob?> {
  @override
  SyncReviewCleanupJob? build() {
    ref.watch(
      connectionControllerProvider.select(
        (value) => (
          value.value?.activeConnection?.id,
          value.value?.activeConnection?.token,
        ),
      ),
    );
    return null;
  }

  SyncReviewCleanupJob? start(
    String notificationId,
    DesktopConnection connection,
  ) {
    final active = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (state != null ||
        active?.id != connection.id ||
        active?.token != connection.token) {
      return null;
    }
    final job = SyncReviewCleanupJob(notificationId);
    state = job;
    return job;
  }

  void finish(SyncReviewCleanupJob job) {
    if (ref.mounted && identical(state, job)) state = null;
  }
}
