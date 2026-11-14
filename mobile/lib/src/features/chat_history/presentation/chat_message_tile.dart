import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../domain/chat_history.dart';
import 'chat_attachment_tile.dart';
import 'chat_file_card.dart';
import 'widgets/chat_message_content.dart';

class ChatMessageTile extends StatefulWidget {
  const ChatMessageTile({
    required this.message,
    required this.provider,
    this.taskId,
    this.sessionId,
    super.key,
  });

  final ChatMessage message;
  final String provider;
  final String? taskId, sessionId;

  @override
  State<ChatMessageTile> createState() => _ChatMessageTileState();
}

class _ChatMessageTileState extends State<ChatMessageTile> {
  bool _expanded = false;

  @override
  Widget build(BuildContext context) {
    final message = widget.message;
    final isUser = message.role == 'user';
    final isDisclosure = message.role == 'tool' || message.role == 'reasoning';
    final label = switch (message.role) {
      'user' => '나',
      'assistant' => widget.provider,
      'tool' => '도구 실행',
      'reasoning' => '추론 과정',
      'system' => '시스템',
      'developer' => '개발자',
      _ => message.role,
    };
    final time = message.timestamp?.toLocal();
    final timeLabel = time == null
        ? null
        : '${time.hour.toString().padLeft(2, '0')}:${time.minute.toString().padLeft(2, '0')}';
    final taskId = widget.taskId;
    final fileCards = taskId == null || message.files.isEmpty
        ? null
        : Padding(
            padding: const EdgeInsets.only(top: AppSizes.p8),
            child: Wrap(
              spacing: AppSizes.p8,
              runSpacing: AppSizes.p8,
              children: [
                for (final file in message.files)
                  ChatFileCard(taskId: taskId, file: file),
              ],
            ),
          );

    return Padding(
      padding: const EdgeInsets.only(bottom: AppSizes.p16),
      child: Align(
        alignment: isUser ? Alignment.centerRight : Alignment.centerLeft,
        child: FractionallySizedBox(
          widthFactor: isUser ? 0.88 : 1,
          child: Column(
            crossAxisAlignment: isUser
                ? CrossAxisAlignment.end
                : CrossAxisAlignment.start,
            children: [
              if (isDisclosure)
                Semantics(
                  expanded: _expanded,
                  child: TextButton.icon(
                    onPressed: () => setState(() => _expanded = !_expanded),
                    icon: Icon(
                      _expanded ? Icons.expand_more : Icons.chevron_right,
                      size: AppSizes.p16,
                    ),
                    label: Row(
                      children: [
                        Text(message.toolName ?? label),
                        const SizedBox(width: AppSizes.p8),
                        Expanded(
                          child: Text(
                            message.text.trimLeft().split('\n').first,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: Theme.of(context).textTheme.bodySmall,
                          ),
                        ),
                      ],
                    ),
                    style: TextButton.styleFrom(
                      foregroundColor: AppColors.textSecondary,
                      minimumSize: const Size(0, AppSizes.minTouchTarget),
                      padding: const EdgeInsets.symmetric(
                        horizontal: AppSizes.p8,
                      ),
                    ),
                  ),
                ),
              if (!isUser && !isDisclosure)
                Row(
                  children: [
                    Expanded(
                      child: Row(
                        children: [
                          Flexible(
                            child: Text(
                              label,
                              overflow: TextOverflow.ellipsis,
                              style: Theme.of(context).textTheme.bodySmall
                                  ?.copyWith(color: AppColors.textSecondary),
                            ),
                          ),
                          if (timeLabel != null) ...[
                            const SizedBox(width: AppSizes.p8),
                            _MessageTime(time: time!, label: timeLabel),
                          ],
                        ],
                      ),
                    ),
                    _CopyMessageButton(text: message.text),
                  ],
                ),
              if (!isDisclosure || _expanded) ...[
                Container(
                  width: isUser ? null : double.infinity,
                  padding: isUser
                      ? const EdgeInsets.symmetric(
                          horizontal: AppSizes.p16,
                          vertical: AppSizes.p12,
                        )
                      : isDisclosure
                      ? const EdgeInsets.only(left: AppSizes.p16)
                      : EdgeInsets.zero,
                  decoration: isUser
                      ? BoxDecoration(
                          color: AppColors.bgSurface,
                          border: Border.all(color: AppColors.borderDefault),
                          // 꼬리 쪽 모서리만 좁혀 내가 보낸 말풍선임을 드러낸다
                          borderRadius: const BorderRadius.only(
                            topLeft: Radius.circular(AppSizes.radiusCard),
                            topRight: Radius.circular(AppSizes.radiusCard),
                            bottomLeft: Radius.circular(AppSizes.radiusCard),
                            bottomRight: Radius.circular(AppSizes.p4),
                          ),
                        )
                      : null,
                  child: Semantics(
                    label: isUser ? label : null,
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        for (final attachment in message.attachments)
                          ChatAttachmentTile(
                            attachment: attachment,
                            taskId: widget.taskId,
                            provider: widget.provider,
                            sessionId: widget.sessionId,
                          ),
                        if (message.text.isNotEmpty)
                          ChatMessageContent(
                            text: message.text,
                            isUser: isUser,
                          ),
                      ],
                    ),
                  ),
                ),
                ?fileCards,
                if (isUser || isDisclosure)
                  Row(
                    mainAxisAlignment: isUser
                        ? MainAxisAlignment.end
                        : MainAxisAlignment.start,
                    children: [
                      if (timeLabel != null)
                        _MessageTime(time: time!, label: timeLabel),
                      _CopyMessageButton(text: message.text),
                    ],
                  ),
              ] else
                // 접힌 도구 실행에서도 어떤 파일이 바뀌었는지는 바로 보여야 한다
                ?fileCards,
            ],
          ),
        ),
      ),
    );
  }
}

class _CopyMessageButton extends StatelessWidget {
  const _CopyMessageButton({required this.text});
  final String text;

  @override
  Widget build(BuildContext context) {
    return IconButton(
      tooltip: '메시지 복사',
      icon: const Icon(Icons.copy_outlined, size: AppSizes.p16),
      color: AppColors.textSecondary,
      constraints: const BoxConstraints(
        minWidth: AppSizes.minTouchTarget,
        minHeight: AppSizes.minTouchTarget,
      ),
      onPressed: () async {
        try {
          await Clipboard.setData(ClipboardData(text: text));
          if (!context.mounted) return;
          ScaffoldMessenger.of(context)
              .showSnackBar(const SnackBar(content: Text('메시지를 복사했습니다.')));
        } on PlatformException {
          if (!context.mounted) return;
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(content: Text('메시지를 복사하지 못했습니다. 다시 시도해 주세요.')),
          );
        }
      },
    );
  }
}

class _MessageTime extends StatelessWidget {
  const _MessageTime({required this.time, required this.label});
  final DateTime time;
  final String label;

  @override
  Widget build(BuildContext context) => Tooltip(
    message: '${time.year}. ${time.month}. ${time.day}. $label',
    child: Text(label, style: Theme.of(context).textTheme.bodySmall),
  );
}
