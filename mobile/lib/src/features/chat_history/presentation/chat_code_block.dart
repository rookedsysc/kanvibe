import 'package:flutter/material.dart';
import 'package:flutter_highlight/themes/atom-one-dark.dart';
import 'package:highlight/highlight.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/constants/app_theme.dart';
import 'chat_text_copy.dart';

/// 언어 이름과 복사 버튼을 머리에 얹은 코드 블록.
class ChatCodeBlock extends StatelessWidget {
  const ChatCodeBlock({required this.code, this.language, super.key});
  final String code;
  final String? language;

  @override
  Widget build(BuildContext context) {
    final language = this.language;
    final hasLanguage = language != null && language.isNotEmpty;
    return Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        ColoredBox(
          color: AppColors.terminalChrome,
          child: Row(
            children: [
              const SizedBox(width: AppSizes.p12),
              Expanded(
                child: Text(
                  hasLanguage ? language : 'text',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: monoStyle(color: AppColors.textMuted),
                ),
              ),
              IconButton(
                tooltip: '코드 복사',
                icon: const Icon(Icons.copy_outlined, size: AppSizes.p16),
                color: AppColors.textSecondary,
                constraints: const BoxConstraints(
                  minWidth: AppSizes.minTouchTarget,
                  minHeight: AppSizes.minTouchTarget,
                ),
                onPressed: () => copyChatText(
                  context,
                  code,
                  copiedNotice: '코드를 복사했습니다.',
                  failedNotice: '코드를 복사하지 못했습니다. 다시 시도해 주세요.',
                ),
              ),
            ],
          ),
        ),
        ChatCodeText(
          code: code,
          language: hasLanguage ? language : null,
          padding: const EdgeInsets.all(AppSizes.p12),
        ),
      ],
    );
  }
}

/// 줄을 접지 않고 가로로 밀어 읽는 고정폭 코드. 언어를 알면 문법 색을 입힌다.
class ChatCodeText extends StatefulWidget {
  const ChatCodeText({
    required this.code,
    this.language,
    this.padding = EdgeInsets.zero,
    super.key,
  });
  final String code;

  /// highlight가 아는 언어 이름이나 파일 확장자
  final String? language;
  final EdgeInsets padding;

  @override
  State<ChatCodeText> createState() => _ChatCodeTextState();
}

class _ChatCodeTextState extends State<ChatCodeText> {
  final _scroll = ScrollController();
  late TextSpan _codeSpan = _buildCodeSpan(widget.code, widget.language);

  @override
  void didUpdateWidget(ChatCodeText oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.code != widget.code ||
        oldWidget.language != widget.language) {
      _codeSpan = _buildCodeSpan(widget.code, widget.language);
    }
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Scrollbar(
    controller: _scroll,
    child: SingleChildScrollView(
      controller: _scroll,
      scrollDirection: Axis.horizontal,
      padding: widget.padding,
      child: SelectableText.rich(_codeSpan),
    ),
  );
}

/// 문법 분석은 화면 스레드에서 돌기 때문에 이보다 긴 코드는 색 없이 보여 준다
const _maxHighlightedCodeLength = 50000;

/// highlight가 별칭으로 알지 못하는 확장자를 같은 문법의 언어로 잇는다
const _highlightLanguageByExtension = {
  'tsx': 'typescript',
  'mdx': 'markdown',
  'htm': 'xml',
};

TextSpan _buildCodeSpan(String code, String? language) {
  final codeStyle = monoStyle();
  if (language == null || code.length > _maxHighlightedCodeLength) {
    return TextSpan(text: code, style: codeStyle);
  }
  final name = language.toLowerCase();
  try {
    final syntaxNodes = highlight
        .parse(code, language: _highlightLanguageByExtension[name] ?? name)
        .nodes;
    return TextSpan(
      style: codeStyle,
      children: _buildSyntaxSpans(syntaxNodes ?? const []),
    );
  } catch (_) {
    // highlight는 문법이 허용하지 않는 토큰을 만나면 문자열을 던진다. 색을 못 입혀도 코드는 그대로 읽혀야 한다.
    return TextSpan(text: code, style: codeStyle);
  }
}

List<TextSpan> _buildSyntaxSpans(List<Node> syntaxNodes) => [
  for (final node in syntaxNodes)
    TextSpan(
      text: node.value,
      style: atomOneDarkTheme[node.className],
      children: node.children == null
          ? null
          : _buildSyntaxSpans(node.children!),
    ),
];
