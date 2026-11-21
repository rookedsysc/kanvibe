import 'dart:typed_data';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/providers.dart';

typedef ChatAttachmentTarget = ({
  String taskId,
  String provider,
  String sessionId,
  String attachmentId,
});

final chatAttachmentProvider = FutureProvider.autoDispose
    .family<Uint8List, ChatAttachmentTarget>((ref, target) async {
      final connection = (await ref.watch(connectionControllerProvider.future))
          .activeConnection;
      if (connection == null) throw StateError('연결된 PC가 없습니다.');
      final bytes = await ref
          .read(desktopClientProvider)
          .fetchChatAttachment(
            connection,
            target.taskId,
            target.provider,
            target.sessionId,
            target.attachmentId,
          );
      return Uint8List.fromList(bytes);
    }, retry: (_, _) => null);
