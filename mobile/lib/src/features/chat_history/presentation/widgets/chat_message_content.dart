import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';
import '../../../../common/constants/app_theme.dart';
import 'mermaid_diagram.dart';

class ChatMessageContent extends StatelessWidget {
  const ChatMessageContent({
    required this.text,
    this.isUser = false,
    super.key,
  });

  final String text;
  final bool isUser;

  @override
  Widget build(BuildContext context) {
    if (isUser) {
      return SelectableText(
        text,
        style: Theme.of(context).textTheme.bodyMedium?.copyWith(height: 1.6),
      );
    }
    final blocks = RegExp(r'```[ \t]*mermaid[^\r\n]*\r?\n([\s\S]*?)```');
    final children = <Widget>[];
    var offset = 0;
    for (final block in blocks.allMatches(text)) {
      if (block.start > offset) {
        children.add(
          _MarkdownContent(text: text.substring(offset, block.start)),
        );
      }
      children.add(MermaidDiagram(definition: block.group(1)!.trim()));
      offset = block.end;
    }
    if (offset < text.length || children.isEmpty) {
      children.add(_MarkdownContent(text: text.substring(offset)));
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: children,
    );
  }
}

class _MarkdownContent extends StatelessWidget {
  const _MarkdownContent({required this.text});
  final String text;

  Future<void> _openLink(BuildContext context, String? href) async {
    final uri = Uri.tryParse(href ?? '');
    if (uri == null ||
        (uri.scheme != 'http' && uri.scheme != 'https') ||
        uri.host.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('HTTP 또는 HTTPS 링크만 열 수 있습니다.')),
      );
      return;
    }
    var opened = false;
    try {
      opened = await launchUrl(uri, mode: LaunchMode.externalApplication);
    } on PlatformException {
      // Platform failures are reported beside the tapped message rather than lost asynchronously.
      opened = false;
    } on MissingPluginException {
      opened = false;
    }
    if (!opened && context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('링크를 열지 못했습니다. 다시 시도해 주세요.')),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    final textTheme = Theme.of(context).textTheme;
    final proseStyle = textTheme.bodyMedium?.copyWith(height: 1.6);
    return MarkdownBody(
      data: text,
      selectable: true,
      softLineBreak: true,
      fitContent: false,
      styleSheet: MarkdownStyleSheet.fromTheme(Theme.of(context)).copyWith(
        p: proseStyle,
        h1: textTheme.titleLarge,
        h2: textTheme.titleMedium,
        h3: textTheme.titleSmall,
        a: const TextStyle(color: AppColors.brandPrimary),
        code: monoStyle(),
        codeblockPadding: const EdgeInsets.all(AppSizes.p12),
        codeblockDecoration: BoxDecoration(
          color: AppColors.terminalBg,
          borderRadius: BorderRadius.circular(AppSizes.radiusLg),
        ),
        blockquoteDecoration: const BoxDecoration(
          border: Border(
            left: BorderSide(color: AppColors.borderStrong, width: AppSizes.p4),
          ),
        ),
      ),
      onTapLink: (_, href, _) => _openLink(context, href),
      imageBuilder: (uri, title, alt) {
        final fallback = Text(
          '이미지: ${alt ?? title ?? uri.toString()}',
          style: textTheme.bodySmall,
        );
        return fallback;
      },
    );
  }
}
