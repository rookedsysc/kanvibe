import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/constants/app_sizes.dart';
import '../../../../common/l10n/app_localizations.dart';
import '../../../../common/providers.dart';
import '../../application/mermaid_controller.dart';
import '../../domain/mermaid_image.dart';

/// PC worker가 다이어그램 둘레에 두는 여백(양쪽 8px). 원래 너비에 더해 요청해야 줄이지 않고 그린다
const _workerHorizontalPadding = 16;
const _minRenderWidth = 320;
const _maxRenderWidth = 1200;
const _maxInlineHeight = 600.0;

class MermaidDiagram extends ConsumerWidget {
  const MermaidDiagram({required this.definition, super.key});
  final String definition;

  /// 칸 너비로 그린 이미지가 줄어들었으면 원래 크기로 다시 받을 너비. 줄지 않았거나 PC가 원래 너비를 모르면 null이다
  int? _naturalRenderWidth(MermaidImage? fitted, int fittedWidth) {
    final naturalWidth = fitted?.naturalWidth;
    if (naturalWidth == null) return null;
    final renderWidth = (naturalWidth + _workerHorizontalPadding).clamp(
      _minRenderWidth,
      _maxRenderWidth,
    );
    return renderWidth > fittedWidth ? renderWidth : null;
  }

  /// 원래 크기로 받은 이미지는 줄이지 않고 그대로 두고 옆으로 밀어 읽게 한다
  Widget _scrollableNaturalImage(BuildContext context, MermaidImage image) {
    final height = math.min(_maxInlineHeight, image.height.toDouble());
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: SizedBox(
        width: image.width * height / image.height,
        height: height,
        child: _image(context, image, 'mermaid-image-ready'),
      ),
    );
  }

  Future<void> _copy(BuildContext context) async {
    try {
      await Clipboard.setData(ClipboardData(text: definition));
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(context.tr('다이어그램 원문을 복사했습니다.'))),
        );
      }
    } on PlatformException {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(context.tr('복사하지 못했습니다. 다시 시도해 주세요.'))),
        );
      }
    }
  }

  void _source(BuildContext context) => showDialog<void>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(context.tr('다이어그램 원문')),
      content: SingleChildScrollView(child: SelectableText(definition)),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: Text(context.tr('닫기')),
        ),
      ],
    ),
  );

  void _zoom(BuildContext context) => Navigator.of(context).push(
    MaterialPageRoute<void>(
      builder: (context) => Consumer(
        builder: (context, ref, _) => Scaffold(
          appBar: AppBar(title: Text(context.tr('Mermaid 다이어그램'))),
          body: SafeArea(
            child: LayoutBuilder(
              builder: (context, constraints) {
                final target = (
                  definition: definition,
                  theme: Theme.of(context).brightness == Brightness.dark
                      ? MermaidTheme.dark
                      : MermaidTheme.light,
                  width: constraints.maxWidth.round().clamp(320, 1200),
                );
                final provider = mermaidImageProvider(target);
                return ref
                    .watch(provider)
                    .when(
                      skipLoadingOnRefresh: false,
                      skipLoadingOnReload: false,
                      loading: () =>
                          const Center(child: CircularProgressIndicator()),
                      error: (error, stack) =>
                          _failure(context, () => ref.invalidate(provider)),
                      data: (image) => Column(
                        children: [
                          Expanded(
                            child: InteractiveViewer(
                              minScale: 1,
                              maxScale: 8,
                              child: Center(
                                child: _image(
                                  context,
                                  image,
                                  'mermaid-zoom-ready',
                                ),
                              ),
                            ),
                          ),
                          TextButton(
                            onPressed: () => ref.invalidate(provider),
                            child: Text(context.tr('다시 시도')),
                          ),
                        ],
                      ),
                    );
              },
            ),
          ),
        ),
      ),
    ),
  );

  Widget _failure(BuildContext context, VoidCallback retry) => Semantics(
    identifier: 'mermaid-render-error',
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(context.tr('다이어그램을 표시하지 못했습니다.')),
        Text(context.tr('PC 연결을 확인한 뒤 다시 시도해 주세요. 원문은 아래에서 확인할 수 있습니다.')),
        Semantics(
          identifier: 'mermaid-render-retry',
          child: TextButton(onPressed: retry, child: Text(context.tr('다시 시도'))),
        ),
      ],
    ),
  );

  Widget _image(BuildContext context, MermaidImage image, String identifier) =>
      Image.memory(
        image.bytes,
        fit: BoxFit.contain,
        excludeFromSemantics: true,
        frameBuilder: (context, child, frame, synchronous) {
          if (frame == null && !synchronous) {
            return const Center(child: CircularProgressIndicator());
          }
          return Semantics(
            identifier: identifier,
            label: context.tr('Mermaid 다이어그램 준비 완료'),
            image: true,
            child: child,
          );
        },
        errorBuilder: (context, error, stack) => Semantics(
          identifier: 'mermaid-image-error',
          child: Text(context.tr('다이어그램 이미지를 읽지 못했습니다. 원문을 확인하거나 다시 시도해 주세요.')),
        ),
      );

  @override
  Widget build(BuildContext context, WidgetRef ref) => LayoutBuilder(
    builder: (context, constraints) {
      final available = constraints.maxWidth.isFinite
          ? constraints.maxWidth
          : 640.0;
      final target = (
        definition: definition,
        theme: Theme.of(context).brightness == Brightness.dark
            ? MermaidTheme.dark
            : MermaidTheme.light,
        width: available.round().clamp(320, 1200),
      );
      final provider = mermaidImageProvider(target);
      final rendered = ref.watch(provider);
      final naturalRenderWidth = _naturalRenderWidth(
        rendered.value,
        target.width,
      );
      final naturalProvider = naturalRenderWidth == null
          ? null
          : mermaidImageProvider((
              definition: target.definition,
              theme: target.theme,
              width: naturalRenderWidth,
            ));
      // 원래 크기 이미지를 받지 못하면 칸 너비 이미지를 그대로 보여 준다
      final naturalImage = naturalProvider == null
          ? null
          : ref.watch(naturalProvider).value;
      final active = ref
          .watch(connectionControllerProvider)
          .value
          ?.activeConnection;
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            context.tr('Mermaid 다이어그램'),
            style: Theme.of(context).textTheme.labelLarge,
          ),
          rendered.when(
            skipLoadingOnRefresh: false,
            skipLoadingOnReload: false,
            loading: () => Semantics(
              identifier: 'mermaid-render-loading',
              label: context.tr('다이어그램을 준비하는 중…'),
              child: const SizedBox(
                height: 160,
                child: Center(child: CircularProgressIndicator()),
              ),
            ),
            error: (error, stack) =>
                _failure(context, () => ref.invalidate(provider)),
            data: (image) => Semantics(
              identifier: 'mermaid-image-zoom',
              button: true,
              label: context.tr('다이어그램 확대'),
              child: InkWell(
                onTap: () => _zoom(context),
                child: naturalImage != null
                    ? KeyedSubtree(
                        key: ValueKey(
                          Object.hash(active?.id, active?.token, target),
                        ),
                        child: _scrollableNaturalImage(context, naturalImage),
                      )
                    : SizedBox(
                        key: ValueKey(
                          Object.hash(active?.id, active?.token, target),
                        ),
                        height: math.min(
                          _maxInlineHeight,
                          math.max(100, available * image.height / image.width),
                        ),
                        child: _image(context, image, 'mermaid-image-ready'),
                      ),
              ),
            ),
          ),
          Wrap(
            spacing: AppSizes.p8,
            children: [
              Semantics(
                identifier: 'mermaid-source-view',
                child: TextButton.icon(
                  onPressed: () => _source(context),
                  icon: const Icon(Icons.code),
                  label: Text(context.tr('원문 보기')),
                ),
              ),
              Semantics(
                identifier: 'mermaid-source-copy',
                child: TextButton.icon(
                  onPressed: () => _copy(context),
                  icon: const Icon(Icons.copy),
                  label: Text(context.tr('원문 복사')),
                ),
              ),
              if (rendered.hasValue)
                TextButton(
                  onPressed: () {
                    ref.invalidate(provider);
                    if (naturalProvider != null) {
                      ref.invalidate(naturalProvider);
                    }
                  },
                  child: Text(context.tr('다시 시도')),
                ),
            ],
          ),
        ],
      );
    },
  );
}
