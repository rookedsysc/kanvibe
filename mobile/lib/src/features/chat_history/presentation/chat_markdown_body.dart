import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_markdown_plus/flutter_markdown_plus.dart';
import 'package:markdown/markdown.dart' as md;
import 'package:url_launcher/url_launcher.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../../../common/constants/app_theme.dart';
import '../../../common/l10n/app_localizations.dart';
import 'chat_code_block.dart';

/// AI 답변과 Markdown 파일 미리보기가 함께 쓰는 Markdown 본문.
class ChatMarkdownBody extends StatelessWidget {
  const ChatMarkdownBody({required this.data, super.key})
    : scrollPadding = null;

  /// 스스로 스크롤하며 화면에 보이는 블록만 만든다.
  ///
  /// 파일 한 개를 통째로 보여 줄 때 쓴다. 블록을 한꺼번에 만들면 긴 문서에서 첫 화면이 블록 수만큼 늦어진다.
  const ChatMarkdownBody.scrolling({
    required this.data,
    required EdgeInsets this.scrollPadding,
    super.key,
  });

  final String data;
  final EdgeInsets? scrollPadding;

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
    final styleSheet = MarkdownStyleSheet.fromTheme(Theme.of(context)).copyWith(
      p: textTheme.bodyMedium?.copyWith(height: 1.6),
      h1: textTheme.titleLarge,
      h2: textTheme.titleMedium,
      h3: textTheme.titleSmall,
      a: const TextStyle(color: AppColors.brandPrimary),
      code: monoStyle(),
      codeblockDecoration: BoxDecoration(
        color: AppColors.terminalBg,
        borderRadius: BorderRadius.circular(AppSizes.radiusLg),
      ),
      blockquoteDecoration: const BoxDecoration(
        border: Border(
          left: BorderSide(color: AppColors.borderStrong, width: AppSizes.p4),
        ),
      ),
      tableColumnWidth: const _ReadableColumnWidth(),
      tableBorder: TableBorder.all(color: AppColors.borderStrong),
      tableCellsPadding: const EdgeInsets.symmetric(
        horizontal: AppSizes.p12,
        vertical: AppSizes.p8,
      ),
    );
    Widget buildImage(Uri uri, String? title, String? alt) {
      final fallback = Text(
        '이미지: ${alt ?? title ?? uri.toString()}',
        style: textTheme.bodySmall,
      );
      // 암호화되지 않은 주소와 기기 안의 파일은 메시지를 여는 것만으로 불러오지 않는다
      if (uri.scheme != 'https' || uri.host.isEmpty) return fallback;
      return _TapToLoadImage(uri: uri, label: alt ?? title, fallback: fallback);
    }

    final builders = {'pre': _CodeBlockBuilder()};
    void openTappedLink(String _, String? href, String _) =>
        _openLink(context, href);
    final padding = scrollPadding;
    if (padding != null) {
      return Markdown(
        data: data,
        selectable: true,
        padding: padding,
        styleSheet: styleSheet,
        builders: builders,
        onTapLink: openTappedLink,
        imageBuilder: buildImage,
      );
    }
    return MarkdownBody(
      data: data,
      selectable: true,
      softLineBreak: true,
      fitContent: false,
      styleSheet: styleSheet,
      builders: builders,
      onTapLink: openTappedLink,
      imageBuilder: buildImage,
    );
  }
}

/// 누르기 전에는 이미지를 불러오지 않는다.
///
/// AI 답변에 섞인 외부 주소로, 메시지를 여는 것만으로 폰에서 요청이 나가지 않게 한다.
class _TapToLoadImage extends StatefulWidget {
  const _TapToLoadImage({
    required this.uri,
    required this.label,
    required this.fallback,
  });
  final Uri uri;
  final String? label;
  final Widget fallback;

  @override
  State<_TapToLoadImage> createState() => _TapToLoadImageState();
}

class _TapToLoadImageState extends State<_TapToLoadImage> {
  bool _isLoadRequested = false;

  @override
  Widget build(BuildContext context) {
    if (!_isLoadRequested) {
      return TextButton.icon(
        onPressed: () => setState(() => _isLoadRequested = true),
        icon: const Icon(Icons.image_outlined),
        label: Text(
          context.tr('이미지 불러오기: {name}', {
            'name': widget.label ?? widget.uri.host,
          }),
        ),
      );
    }
    return ConstrainedBox(
      constraints: const BoxConstraints(maxHeight: _maxImageHeight),
      child: Image.network(
        widget.uri.toString(),
        fit: BoxFit.contain,
        semanticLabel: widget.label,
        frameBuilder: (_, image, frame, _) => frame == null
            ? const SizedBox(
                height: _imagePlaceholderHeight,
                child: Center(child: CircularProgressIndicator()),
              )
            : image,
        errorBuilder: (_, _, _) => widget.fallback,
      ),
    );
  }
}

const _maxImageHeight = 280.0;
const _imagePlaceholderHeight = 100.0;

/// 표의 한 열이 차지할 수 있는 최대 너비. 이보다 긴 셀은 열 안에서 줄을 바꾼다
const _maxTableColumnWidth = 240.0;

/// 펜스 코드 블록(`pre`)만 맡는다. 인라인 `code`는 패키지 기본 모양을 그대로 쓴다
class _CodeBlockBuilder extends MarkdownElementBuilder {
  @override
  Widget? visitElementAfterWithContext(
    BuildContext context,
    md.Element element,
    TextStyle? preferredStyle,
    TextStyle? parentStyle,
  ) {
    final languageClass = element.children
        ?.whereType<md.Element>()
        .firstOrNull
        ?.attributes['class'];
    return ChatCodeBlock(
      // 파서는 닫는 펜스 앞의 줄바꿈까지 코드에 넣는다
      code: element.textContent.replaceFirst(RegExp(r'\n$'), ''),
      language: languageClass?.replaceFirst('language-', ''),
    );
  }
}

/// 열을 내용 너비에 맞추되 상한을 둔다.
///
/// flutter_markdown_plus는 열 너비가 [IntrinsicColumnWidth]일 때만 표를 가로 스크롤에 넣는다.
/// 상한이 없으면 긴 문장이 든 셀 하나가 열을 한 줄로 늘려 화면 여러 장을 밀어야 읽힌다.
class _ReadableColumnWidth extends IntrinsicColumnWidth {
  const _ReadableColumnWidth();

  @override
  double minIntrinsicWidth(Iterable<RenderBox> cells, double containerWidth) =>
      math.min(
        super.minIntrinsicWidth(cells, containerWidth),
        _maxTableColumnWidth,
      );

  @override
  double maxIntrinsicWidth(Iterable<RenderBox> cells, double containerWidth) =>
      math.min(
        super.maxIntrinsicWidth(cells, containerWidth),
        _maxTableColumnWidth,
      );
}
