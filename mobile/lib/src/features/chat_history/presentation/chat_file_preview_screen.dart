import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:webview_flutter/webview_flutter.dart';
import 'package:webview_flutter_android/webview_flutter_android.dart';

import '../../../common/constants/app_sizes.dart';
import '../application/task_file_provider.dart';
import '../domain/chat_history.dart';
import '../domain/task_file.dart';
import 'chat_code_block.dart';
import 'chat_markdown_body.dart';
import 'chat_text_copy.dart';

/// 대화에 나온 파일 하나를 전체 화면으로 읽는다.
class ChatFilePreviewScreen extends ConsumerStatefulWidget {
  const ChatFilePreviewScreen({
    required this.taskId,
    required this.file,
    super.key,
  });
  final String taskId;
  final ChatMessageFile file;

  @override
  ConsumerState<ChatFilePreviewScreen> createState() =>
      _ChatFilePreviewScreenState();
}

class _ChatFilePreviewScreenState extends ConsumerState<ChatFilePreviewScreen> {
  bool _showsSource = false;

  @override
  Widget build(BuildContext context) {
    final fileName = widget.file.name;
    final extension = fileName.contains('.')
        ? fileName.split('.').last.toLowerCase()
        : null;
    final renderedForm = _renderedFormByExtension[extension];
    final target = taskFileProvider((
      taskId: widget.taskId,
      path: widget.file.path,
    ));
    final taskFile = ref.watch(target);
    return Scaffold(
      appBar: AppBar(
        title: Text(fileName),
        actions: [
          IconButton(
            tooltip: '파일 내용 복사',
            icon: const Icon(Icons.copy_outlined),
            onPressed: switch (taskFile) {
              AsyncData(:final value) => () => copyChatText(
                context,
                value.content,
                copiedNotice: '파일 내용을 복사했습니다.',
                failedNotice: '파일 내용을 복사하지 못했습니다. 다시 시도해 주세요.',
              ),
              _ => null,
            },
          ),
        ],
        bottom: renderedForm == null
            ? null
            : PreferredSize(
                preferredSize: const Size.fromHeight(
                  AppSizes.minTouchTarget + AppSizes.p8,
                ),
                child: Padding(
                  padding: const EdgeInsets.only(bottom: AppSizes.p8),
                  child: SegmentedButton<bool>(
                    showSelectedIcon: false,
                    segments: const [
                      ButtonSegment(value: false, label: Text('미리보기')),
                      ButtonSegment(value: true, label: Text('원문')),
                    ],
                    selected: {_showsSource},
                    onSelectionChanged: (selection) =>
                        setState(() => _showsSource = selection.single),
                  ),
                ),
              ),
      ),
      body: taskFile.when(
        skipLoadingOnRefresh: false,
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (error, _) => Center(
          child: Padding(
            padding: const EdgeInsets.all(AppSizes.p24),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(_describeFailure(error), textAlign: TextAlign.center),
                TextButton(
                  onPressed: () => ref.invalidate(target),
                  child: const Text('다시 시도'),
                ),
              ],
            ),
          ),
        ),
        data: (file) => switch (_showsSource ? null : renderedForm) {
          _RenderedForm.markdown => ChatMarkdownBody.scrolling(
            data: file.content,
            scrollPadding: const EdgeInsets.all(AppSizes.p16),
          ),
          _RenderedForm.web => _SandboxedWebView(content: file.content),
          null => SingleChildScrollView(
            child: ChatCodeText(
              code: file.content,
              language: extension,
              padding: const EdgeInsets.all(AppSizes.p16),
            ),
          ),
        },
      ),
    );
  }
}

/// 원문 말고 그려서도 보여 줄 수 있는 파일의 형태
enum _RenderedForm { markdown, web }

const _renderedFormByExtension = {
  'md': _RenderedForm.markdown,
  'markdown': _RenderedForm.markdown,
  'mdx': _RenderedForm.markdown,
  'html': _RenderedForm.web,
  'htm': _RenderedForm.web,
  'svg': _RenderedForm.web,
};

String _describeFailure(Object error) => switch (error) {
  TaskFileException(failure: TaskFileFailure.outsideTaskFolder) =>
    '이 태스크 폴더 밖에 있는 파일은 열 수 없습니다.',
  TaskFileException(failure: TaskFileFailure.notFound) =>
    '파일을 찾을 수 없습니다. 파일이 지워졌거나 데스크탑 업데이트가 필요할 수 있습니다.',
  TaskFileException(failure: TaskFileFailure.tooLarge) =>
    '파일이 1MB보다 커서 열 수 없습니다.',
  _ => '파일을 읽지 못했습니다. 다시 시도해 주세요.',
};

/// AI가 만든 문서를 바깥과 끊어 놓고 그린다.
///
/// 문서 안의 스크립트는 돌지만 네트워크 요청과 다른 주소로의 이동은 모두 막는다.
class _SandboxedWebView extends StatefulWidget {
  const _SandboxedWebView({required this.content});
  final String content;

  @override
  State<_SandboxedWebView> createState() => _SandboxedWebViewState();
}

class _SandboxedWebViewState extends State<_SandboxedWebView> {
  late final WebViewController _controller;

  @override
  void initState() {
    super.initState();
    _controller = WebViewController()
      ..setJavaScriptMode(JavaScriptMode.unrestricted)
      ..setNavigationDelegate(
        NavigationDelegate(
          onNavigationRequest: (request) => isSandboxedDocumentLoad(request.url)
              ? NavigationDecision.navigate
              : NavigationDecision.prevent,
        ),
      );
    // Android 29 이하는 WebView의 파일 접근이 기본으로 켜져 있다
    if (_controller.platform case final AndroidWebViewController android) {
      android.setAllowFileAccess(false);
    }
    _controller.loadHtmlString(buildSandboxedHtmlDocument(widget.content));
  }

  @override
  Widget build(BuildContext context) => WebViewWidget(controller: _controller);
}

// 폼 전송은 default-src가 막지 않고, Android WebView는 POST 이동을 onNavigationRequest에 묻지 않으므로 form-action으로 따로 막는다.
const _networkBlockingPolicy =
    '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; '
    'script-src \'unsafe-inline\'; style-src \'unsafe-inline\'; '
    'img-src data: blob:; font-src data:; form-action \'none\'">';

final _leadingDoctype = RegExp(r'^\s*<!doctype[^>]*>', caseSensitive: false);

/// 문서가 어떤 네트워크 요청도 하지 못하도록 정책 meta를 맨 앞에 넣는다.
///
/// doctype보다 앞에 다른 태그가 오면 브라우저가 호환 모드로 그리므로 doctype이 있으면 그 바로 뒤에 넣는다.
String buildSandboxedHtmlDocument(String content) {
  final doctype = _leadingDoctype.firstMatch(content);
  if (doctype == null) return '$_networkBlockingPolicy$content';
  return content.replaceRange(doctype.end, doctype.end, _networkBlockingPolicy);
}

/// `loadHtmlString`으로 넣은 문서의 주소는 `about:blank`다. iOS는 이 첫 로드도 이동 요청으로 물어본다
bool isSandboxedDocumentLoad(String url) => url == 'about:blank';
