import 'package:flutter/material.dart';

import '../../../common/constants/app_sizes.dart';
import '../application/pane_chat_composer_controller.dart';

class PaneChatAttachmentStrip extends StatelessWidget {
  const PaneChatAttachmentStrip({
    required this.attachments,
    required this.onRemove,
    required this.onRetry,
    required this.sending,
    super.key,
  });
  final List<PaneChatDraftAttachment> attachments;
  final ValueChanged<String> onRemove, onRetry;
  final bool sending;

  @override
  Widget build(BuildContext context) => SizedBox(
    height: 116,
    child: ListView.separated(
      scrollDirection: Axis.horizontal,
      itemCount: attachments.length,
      separatorBuilder: (_, _) => const SizedBox(width: AppSizes.p8),
      itemBuilder: (context, index) {
        final item = attachments[index];
        return Semantics(
          identifier: 'pane-chat-attachment-${item.localId}',
          label: '첨부 ${item.file.name}',
          child: SizedBox(
            width: 144,
            child: Card(
              child: InkWell(
                onTap: item.error != null && !sending
                    ? () => onRetry(item.localId)
                    : null,
                child: Stack(
                  children: [
                    Padding(
                      padding: const EdgeInsets.all(AppSizes.p8),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Expanded(
                            child: item.file.isImage
                                ? Image.memory(
                                    item.file.bytes,
                                    width: 96,
                                    fit: BoxFit.cover,
                                    errorBuilder: (_, _, _) =>
                                        const Icon(Icons.broken_image_outlined),
                                  )
                                : const Icon(
                                    Icons.insert_drive_file_outlined,
                                    size: 32,
                                  ),
                          ),
                          Text(
                            item.file.name,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                          if (item.isUploading)
                            const LinearProgressIndicator()
                          else if (item.error != null)
                            Text(
                              item.error!,
                              style: TextStyle(
                                color: Theme.of(context).colorScheme.error,
                              ),
                            )
                          else
                            Text(
                              '전송 준비됨',
                              style: Theme.of(context).textTheme.bodySmall,
                            ),
                        ],
                      ),
                    ),
                    Positioned(
                      top: 0,
                      right: 0,
                      child: IconButton(
                        tooltip: '${item.file.name} 첨부 제거',
                        onPressed: sending
                            ? null
                            : () => onRemove(item.localId),
                        icon: const Icon(Icons.close, size: AppSizes.p16),
                      ),
                    ),
                  ],
                ),
              ),
            ),
          ),
        );
      },
    ),
  );
}
