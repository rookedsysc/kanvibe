import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_sizes.dart';
import '../application/chat_attachment_provider.dart';
import '../domain/chat_history.dart';

class ChatAttachmentTile extends ConsumerWidget {
  const ChatAttachmentTile({
    required this.attachment,
    required this.taskId,
    required this.provider,
    required this.sessionId,
    super.key,
  });
  final ChatAttachment attachment;
  final String? taskId, sessionId;
  final String provider;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final targetTask = taskId, session = sessionId;
    if (attachment.kind != 'image' || targetTask == null || session == null) {
      return ListTile(
        leading: const Icon(Icons.insert_drive_file_outlined),
        title: Text(attachment.name),
        subtitle: const Text('첨부 파일'),
      );
    }
    final providerKey = chatAttachmentProvider((
      taskId: targetTask,
      provider: provider,
      sessionId: session,
      attachmentId: attachment.id,
    ));
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: AppSizes.p8),
      child: Semantics(
        identifier: 'chat-history-attachment',
        label: attachment.name,
        child: ref
            .watch(providerKey)
            .when(
              loading: () => const SizedBox(
                height: 100,
                child: Center(child: CircularProgressIndicator()),
              ),
              error: (_, _) => Row(
                children: [
                  const Icon(Icons.broken_image_outlined),
                  Expanded(
                    child: Text('${attachment.name} · 이미지를 불러오지 못했습니다.'),
                  ),
                  IconButton(
                    tooltip: '이미지 다시 불러오기',
                    icon: const Icon(Icons.refresh),
                    onPressed: () => ref.invalidate(providerKey),
                  ),
                ],
              ),
              data: (bytes) => ConstrainedBox(
                constraints: const BoxConstraints(maxHeight: 280),
                child: InkWell(
                  onTap: () => showDialog<void>(
                    context: context,
                    builder: (context) => Dialog(
                      child: Stack(
                        children: [
                          InteractiveViewer(
                            child: Image.memory(
                              bytes,
                              fit: BoxFit.contain,
                              errorBuilder: (_, _, _) =>
                                  Text('${attachment.name} · 표시할 수 없는 이미지'),
                            ),
                          ),
                          Positioned(
                            top: 0,
                            right: 0,
                            child: IconButton(
                              tooltip: '이미지 닫기',
                              icon: const Icon(Icons.close),
                              onPressed: () => Navigator.pop(context),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                  child: Image.memory(
                    bytes,
                    fit: BoxFit.contain,
                    semanticLabel: attachment.name,
                    errorBuilder: (_, _, _) =>
                        Text('${attachment.name} · 표시할 수 없는 이미지'),
                  ),
                ),
              ),
            ),
      ),
    );
  }
}
