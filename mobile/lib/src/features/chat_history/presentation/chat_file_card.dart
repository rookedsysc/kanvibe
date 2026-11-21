import 'package:flutter/material.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../domain/chat_history.dart';
import 'chat_file_preview_screen.dart';

/// AI가 만들거나 고친 파일 하나. 누르면 그 파일을 전체 화면으로 연다.
class ChatFileCard extends StatelessWidget {
  const ChatFileCard({required this.taskId, required this.file, super.key});
  final String taskId;
  final ChatMessageFile file;

  @override
  Widget build(BuildContext context) => Tooltip(
    message: file.path,
    child: Semantics(
      identifier: 'chat-file-card',
      child: OutlinedButton(
        style: OutlinedButton.styleFrom(
          foregroundColor: AppColors.textPrimary,
          side: const BorderSide(color: AppColors.borderStrong),
          minimumSize: const Size(0, AppSizes.minTouchTarget),
          padding: const EdgeInsets.symmetric(horizontal: AppSizes.p12),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(AppSizes.radiusLg),
          ),
        ),
        onPressed: () => Navigator.of(context).push(
          MaterialPageRoute<void>(
            builder: (_) => ChatFilePreviewScreen(taskId: taskId, file: file),
          ),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Icon(Icons.description_outlined, size: AppSizes.p16),
            const SizedBox(width: AppSizes.p8),
            Flexible(
              child: Text(
                file.name,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ),
            const SizedBox(width: AppSizes.p8),
            Text(switch (file.change) {
              ChatMessageFileChange.created => '새로 만듦',
              ChatMessageFileChange.modified => '수정함',
            }, style: Theme.of(context).textTheme.bodySmall),
          ],
        ),
      ),
    ),
  );
}
