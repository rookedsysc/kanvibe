import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../data/mermaid_repository.dart';
import '../domain/mermaid_image.dart';

final mermaidRepositoryProvider = Provider<MermaidRepository>(
  (ref) => MermaidRepository(ref.watch(desktopClientProvider)),
);
final mermaidImageProvider = FutureProvider.autoDispose
    .family<MermaidImage, MermaidRenderTarget>((ref, target) async {
      final connection = (await ref.watch(connectionControllerProvider.future))
          .activeConnection;
      if (connection == null) throw StateError('연결된 PC가 없습니다.');
      bool current() =>
          ref.mounted &&
          _same(
            ref.read(connectionControllerProvider).value?.activeConnection,
            connection,
          );
      try {
        final image = await ref
            .read(mermaidRepositoryProvider)
            .render(connection, target);
        if (!current()) throw StateError('선택한 PC가 변경되었습니다.');
        return image;
      } on DesktopRequestException catch (error) {
        if (error.isUnauthorized && current()) {
          await ref
              .read(connectionControllerProvider.notifier)
              .forgetLocally(connection.id, connection.token);
        }
        rethrow;
      }
    }, retry: (_, _) => null);

bool _same(DesktopConnection? active, DesktopConnection expected) =>
    active?.id == expected.id && active?.token == expected.token;
