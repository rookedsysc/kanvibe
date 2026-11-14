import 'package:flutter/material.dart';

import '../chat_markdown_body.dart';
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
          ChatMarkdownBody(data: text.substring(offset, block.start)),
        );
      }
      children.add(MermaidDiagram(definition: block.group(1)!.trim()));
      offset = block.end;
    }
    if (offset < text.length || children.isEmpty) {
      children.add(ChatMarkdownBody(data: text.substring(offset)));
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: children,
    );
  }
}
