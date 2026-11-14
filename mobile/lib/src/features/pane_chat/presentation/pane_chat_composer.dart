import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/constants/app_colors.dart';
import '../../../common/constants/app_sizes.dart';
import '../application/pane_chat_composer_controller.dart';
import '../application/pane_chat_controller.dart';
import '../domain/pane_chat.dart';
import 'pane_chat_attachment_strip.dart';
import 'pane_chat_completion_menu.dart';

class PaneChatComposer extends ConsumerStatefulWidget {
  const PaneChatComposer({
    required this.target,
    required this.toolbar,
    required this.showPrompt,
    required this.isActive,
    this.cliScreen,
    super.key,
  });
  final PaneChatComposerTarget target;
  final Widget toolbar;
  final Widget? cliScreen;
  final bool showPrompt, isActive;
  @override
  ConsumerState<PaneChatComposer> createState() => _PaneChatComposerState();
}

class _PaneChatComposerState extends ConsumerState<PaneChatComposer> {
  late final _input = TextEditingController(
    text: ref.read(paneChatComposerProvider(widget.target)).text,
  );
  final _focus = FocusNode();
  int _selectedIndex = 0;
  ({int start, int end, String kind, String query})? _completion;
  bool _syncing = false;

  @override
  void initState() {
    super.initState();
    _input.addListener(_edited);
    _synchronizeActivity();
  }

  @override
  void didUpdateWidget(PaneChatComposer oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.isActive != oldWidget.isActive ||
        widget.showPrompt != oldWidget.showPrompt) {
      _synchronizeActivity();
    }
  }

  void _synchronizeActivity() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      final pane = ref
          .read(
            paneChatProvider((
              taskId: widget.target.taskId,
              paneId: widget.target.paneId,
            )),
          )
          .value;
      unawaited(
        ref
            .read(paneChatComposerProvider(widget.target).notifier)
            .setActive(
              widget.isActive && widget.showPrompt,
              attachmentsEnabled: pane?.snapshot.composer.attachments == true,
            ),
      );
    });
  }

  @override
  void dispose() {
    _input.removeListener(_edited);
    _input.dispose();
    _focus.dispose();
    super.dispose();
  }

  void _edited() {
    if (_syncing) return;
    final notifier = ref.read(paneChatComposerProvider(widget.target).notifier);
    final textChanged =
        ref.read(paneChatComposerProvider(widget.target)).text != _input.text;
    notifier.edit(_input.text);
    final value = _input.value;
    if (value.composing.isValid && !value.composing.isCollapsed) {
      _completion = null;
      notifier.dismissSuggestions();
      return;
    }
    final caret = value.selection.baseOffset;
    if (caret < 0 || !value.selection.isCollapsed) {
      _completion = null;
      notifier.dismissSuggestions();
      return;
    }
    if (!textChanged &&
        _completion?.start == caret &&
        _completion?.end == caret &&
        _completion?.query == '') {
      return;
    }
    final match = RegExp(r'(^|\s)([$/@])([^\s]*)$')
        .firstMatch(value.text.substring(0, caret));
    if (match == null) {
      _completion = null;
      notifier.dismissSuggestions();
      return;
    }
    final prefix = match.group(2)!;
    final kind = prefix == '@'
        ? 'file'
        : prefix == '/'
        ? 'command'
        : 'skill';
    final pane = ref
        .read(
          paneChatProvider((
            taskId: widget.target.taskId,
            paneId: widget.target.paneId,
          )),
        )
        .value;
    final caps = pane?.snapshot.composer;
    if ((kind == 'file' ? caps?.fileMentions : caps?.skills) != true) {
      _completion = null;
      notifier.dismissSuggestions();
      return;
    }
    final query = match.group(3)!;
    final next = (
      start: caret - query.length - 1,
      end: caret,
      kind: kind,
      query: query,
    );
    if (_completion == next) return;
    _completion = next;
    _selectedIndex = 0;
    notifier.suggest(kind, query);
  }

  void _choose(PaneChatSuggestion item) {
    final completion = _completion;
    if (completion == null ||
        (_input.value.composing.isValid &&
            !_input.value.composing.isCollapsed)) {
      return;
    }
    final insertion = '${item.insertText} ';
    final text = _input.text.replaceRange(
      completion.start,
      completion.end,
      insertion,
    );
    _syncing = true;
    _input.value = TextEditingValue(
      text: text,
      selection: TextSelection.collapsed(
        offset: completion.start + insertion.length,
      ),
    );
    _syncing = false;
    final notifier = ref.read(paneChatComposerProvider(widget.target).notifier);
    notifier.edit(text);
    notifier.dismissSuggestions();
    _completion = null;
    _focus.requestFocus();
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent ||
        (_input.value.composing.isValid &&
            !_input.value.composing.isCollapsed)) {
      return KeyEventResult.ignored;
    }
    final state = ref.read(paneChatComposerProvider(widget.target));
    if (event.logicalKey == LogicalKeyboardKey.escape && _completion != null) {
      _completion = null;
      ref
          .read(paneChatComposerProvider(widget.target).notifier)
          .dismissSuggestions();
      return KeyEventResult.handled;
    }
    if (state.suggestions.isEmpty) return KeyEventResult.ignored;
    if (event.logicalKey == LogicalKeyboardKey.arrowDown ||
        event.logicalKey == LogicalKeyboardKey.arrowUp) {
      setState(
        () => _selectedIndex =
            (_selectedIndex +
                (event.logicalKey == LogicalKeyboardKey.arrowDown ? 1 : -1)) %
            state.suggestions.length,
      );
      return KeyEventResult.handled;
    }
    if (event.logicalKey == LogicalKeyboardKey.enter ||
        event.logicalKey == LogicalKeyboardKey.tab) {
      _choose(
        state.suggestions[_selectedIndex.clamp(
          0,
          state.suggestions.length - 1,
        )],
      );
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  void _startFileMention() {
    final selection = _input.selection;
    final caret = selection.isValid ? selection.baseOffset : _input.text.length;
    final needsSpace =
        caret > 0 && !_input.text[caret - 1].contains(RegExp(r'\s'));
    final mention = needsSpace ? ' @' : '@';
    // 입력 변화를 듣는 쪽이 방금 넣은 @를 보고 파일 후보를 불러온다
    _input.value = TextEditingValue(
      text: _input.text.replaceRange(caret, caret, mention),
      selection: TextSelection.collapsed(offset: caret + mention.length),
    );
    _focus.requestFocus();
  }

  void _showSkills() {
    final selection = _input.selection;
    final caret = selection.isValid ? selection.baseOffset : _input.text.length;
    _completion = (start: caret, end: caret, kind: 'skill', query: '');
    _selectedIndex = 0;
    ref
        .read(paneChatComposerProvider(widget.target).notifier)
        .suggest('skill', '');
    _focus.requestFocus();
  }

  Future<void> _showAttachments() async {
    final action = await showModalBottomSheet<String>(
      context: context,
      builder: (context) => SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final entry in const [
              ('photos', Icons.photo_outlined, '사진 선택'),
              ('files', Icons.attach_file, '파일 선택'),
              ('paste', Icons.content_paste, '이미지 붙여넣기'),
            ])
              ListTile(
                leading: Icon(entry.$2),
                title: Text(entry.$3),
                onTap: () => Navigator.pop(context, entry.$1),
              ),
          ],
        ),
      ),
    );
    if (!mounted || action == null) return;
    // The sheet temporarily makes the pane route inactive; wait for its owner to resume.
    await WidgetsBinding.instance.endOfFrame;
    if (!mounted || !widget.isActive || !widget.showPrompt) return;
    final notifier = ref.read(paneChatComposerProvider(widget.target).notifier);
    if (action == 'paste') {
      await notifier.pasteImage();
    } else {
      await notifier.pick(action == 'photos');
    }
  }

  @override
  Widget build(BuildContext context) {
    final provider = paneChatComposerProvider(widget.target);
    ref.listen(provider.select((value) => value.text), (_, text) {
      if (_input.text == text) return;
      _syncing = true;
      _input.value = TextEditingValue(
        text: text,
        selection: TextSelection.collapsed(offset: text.length),
      );
      _syncing = false;
    });
    final draft = ref.watch(provider);
    final notifier = ref.read(provider.notifier);
    final pane = ref
        .watch(
          paneChatProvider((
            taskId: widget.target.taskId,
            paneId: widget.target.paneId,
          )),
        )
        .value;
    final attachmentsEnabled = pane?.snapshot.composer.attachments == true;
    return Container(
      margin: const EdgeInsets.fromLTRB(
        AppSizes.p12,
        AppSizes.p4,
        AppSizes.p12,
        AppSizes.p12,
      ),
      padding: const EdgeInsets.all(AppSizes.p8),
      decoration: BoxDecoration(
        color: AppColors.bgSurface,
        border: Border.all(color: AppColors.borderStrong),
        borderRadius: BorderRadius.circular(AppSizes.radiusCard),
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (widget.cliScreen != null) widget.cliScreen!,
          if (draft.attachments.isNotEmpty)
            PaneChatAttachmentStrip(
              attachments: draft.attachments,
              sending: draft.isSending,
              onRemove: notifier.remove,
              onRetry: notifier.upload,
            ),
          if (draft.error != null)
            Text(
              draft.error!,
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
          if (draft.isLoadingSuggestions) const LinearProgressIndicator(),
          if (draft.suggestionError != null) Text(draft.suggestionError!),
          if (draft.suggestions.isNotEmpty)
            PaneChatCompletionMenu(
              items: draft.suggestions,
              selectedIndex: _selectedIndex,
              onSelect: _choose,
            ),
          if (widget.showPrompt)
            Focus(
              onKeyEvent: _onKey,
              child: Semantics(
                identifier: 'pane-chat-input',
                child: TextField(
                  controller: _input,
                  focusNode: _focus,
                  minLines: 1,
                  maxLines: 4,
                  keyboardType: TextInputType.multiline,
                  textInputAction: TextInputAction.newline,
                  contentInsertionConfiguration: attachmentsEnabled
                      ? ContentInsertionConfiguration(
                          allowedMimeTypes: const ['image/*'],
                          onContentInserted: notifier.insertContent,
                        )
                      : null,
                  decoration: const InputDecoration(
                    labelText: '현재 CLI에 메시지',
                    hintText: r'메시지, $스킬, /명령, @파일',
                    border: InputBorder.none,
                    enabledBorder: InputBorder.none,
                    focusedBorder: InputBorder.none,
                  ),
                ),
              ),
            ),
          Row(
            children: [
              if (widget.showPrompt && attachmentsEnabled)
                Semantics(
                  identifier: 'pane-chat-attach',
                  child: IconButton(
                    tooltip: '첨부 추가',
                    onPressed: widget.isActive && pane?.canPrompt == true
                        ? _showAttachments
                        : null,
                    icon: const Icon(Icons.add),
                  ),
                ),
              if (widget.showPrompt &&
                  pane?.snapshot.composer.fileMentions == true)
                Semantics(
                  identifier: 'pane-chat-mention',
                  child: IconButton(
                    tooltip: '파일 참조',
                    onPressed: widget.isActive && pane?.canPrompt == true
                        ? _startFileMention
                        : null,
                    icon: const Icon(Icons.alternate_email),
                  ),
                ),
              if (widget.showPrompt && pane?.snapshot.composer.skills == true)
                Semantics(
                  identifier: 'pane-chat-skills',
                  child: IconButton(
                    tooltip: '스킬 선택',
                    onPressed: widget.isActive && pane?.canPrompt == true
                        ? _showSkills
                        : null,
                    icon: const Icon(Icons.auto_awesome_outlined),
                  ),
                ),
              Expanded(child: widget.toolbar),
              if (widget.showPrompt)
                Semantics(
                  identifier: 'pane-chat-send',
                  child: IconButton.filled(
                    tooltip: '메시지 전송',
                    onPressed:
                        widget.isActive &&
                            pane?.canPrompt == true &&
                            draft.canSend
                        ? notifier.send
                        : null,
                    icon: draft.isSending
                        ? const SizedBox(
                            width: 20,
                            height: 20,
                            child: CircularProgressIndicator(strokeWidth: 2),
                          )
                        : const Icon(Icons.arrow_upward),
                    constraints: const BoxConstraints(
                      minWidth: AppSizes.minTouchTarget,
                      minHeight: AppSizes.minTouchTarget,
                    ),
                  ),
                ),
            ],
          ),
        ],
      ),
    );
  }
}
