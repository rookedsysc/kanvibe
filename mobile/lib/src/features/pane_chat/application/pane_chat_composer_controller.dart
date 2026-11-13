import 'dart:async';
import 'dart:math';

import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../data/pane_chat_attachment_intake.dart';
import '../domain/pane_chat.dart';
import '../domain/pane_chat_attachment.dart';
import 'pane_chat_controller.dart';

typedef PaneChatComposerTarget = ({
  String connectionId,
  String taskId,
  String paneId,
  String targetToken,
});

final paneChatComposerProvider = NotifierProvider.autoDispose
    .family<
      PaneChatComposerController,
      PaneChatComposerState,
      PaneChatComposerTarget
    >(PaneChatComposerController.new);

class PaneChatDraftAttachment {
  const PaneChatDraftAttachment({
    required this.localId,
    required this.file,
    this.id,
    this.isUploading = false,
    this.error,
  });
  final String localId;
  final PickedPaneChatAttachment file;
  final String? id;
  final bool isUploading;
  final String? error;
}

class PaneChatComposerState {
  const PaneChatComposerState({
    this.text = '',
    this.attachments = const [],
    this.suggestions = const [],
    this.isLoadingSuggestions = false,
    this.isSending = false,
    this.error,
    this.suggestionError,
  });
  final String text;
  final List<PaneChatDraftAttachment> attachments;
  final List<PaneChatSuggestion> suggestions;
  final bool isLoadingSuggestions, isSending;
  final String? error, suggestionError;
  bool get canSend =>
      !isSending &&
      attachments.every((a) => a.id != null) &&
      (text.trim().isNotEmpty || attachments.isNotEmpty);
}

class PaneChatComposerController extends Notifier<PaneChatComposerState> {
  PaneChatComposerController(this.target);
  final PaneChatComposerTarget target;
  String get ownerKey =>
      '${target.connectionId}:${target.taskId}:${target.paneId}:${target.targetToken}';
  Timer? _suggestionTimer;
  int _suggestionEpoch = 0, _localId = 0, _revision = 0;
  bool _active = false;
  String? _requestId, _requestSignature;

  @override
  PaneChatComposerState build() {
    final intake = ref.read(paneChatAttachmentIntakeProvider);
    final subscription = intake.drops.listen((event) {
      if (!_active || event.ownerKey != ownerKey || !ref.mounted) return;
      if (event.error != null) {
        _update(error: event.error);
        return;
      }
      addAttachments(event.attachments);
    });
    ref.onDispose(() {
      _suggestionTimer?.cancel();
      unawaited(subscription.cancel());
      unawaited(intake.setActive(ownerKey, false).catchError((Object _) {}));
    });
    return const PaneChatComposerState();
  }

  DesktopConnection? get _connection {
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    return connection?.id == target.connectionId ? connection : null;
  }

  PaneChatState? get _pane => ref
      .read(paneChatProvider((taskId: target.taskId, paneId: target.paneId)))
      .value;
  bool get _ownsTarget =>
      _active &&
      _connection != null &&
      _pane?.snapshot.targetToken == target.targetToken;

  void _update({
    String? text,
    List<PaneChatDraftAttachment>? attachments,
    List<PaneChatSuggestion>? suggestions,
    bool? loading,
    bool? sending,
    String? error,
    String? suggestionError,
  }) {
    state = PaneChatComposerState(
      text: text ?? state.text,
      attachments: attachments ?? state.attachments,
      suggestions: suggestions ?? state.suggestions,
      isLoadingSuggestions: loading ?? state.isLoadingSuggestions,
      isSending: sending ?? state.isSending,
      error: error,
      suggestionError: suggestionError,
    );
  }

  Future<void> setActive(bool active, {bool attachmentsEnabled = true}) async {
    _active = active;
    if (!active) dismissSuggestions();
    try {
      await ref
          .read(paneChatAttachmentIntakeProvider)
          .setActive(ownerKey, active && attachmentsEnabled);
    } catch (error) {
      if (ref.mounted && active) _update(error: '첨부 입력을 활성화하지 못했습니다. $error');
    }
  }

  void edit(String text) {
    if (text != state.text) {
      _revision++;
      _update(text: text);
    }
  }

  Future<void> pick(bool photos) => _intake(
    (intake) =>
        photos ? intake.pickPhotos(ownerKey) : intake.pickFiles(ownerKey),
  );
  Future<void> pasteImage() => _intake((intake) async {
    final file = await intake.readClipboardImage(ownerKey);
    if (file == null) throw const FormatException('클립보드에 이미지가 없습니다.');
    return [file];
  });
  Future<void> insertContent(KeyboardInsertedContent content) =>
      _intake((intake) async {
        final file = await intake.readInsertedContent(ownerKey, content);
        return file == null ? [] : [file];
      });

  Future<void> _intake(
    Future<List<PickedPaneChatAttachment>> Function(PaneChatAttachmentIntake)
    read,
  ) async {
    if (!_ownsTarget) return;
    try {
      final files = await read(ref.read(paneChatAttachmentIntakeProvider));
      if (ref.mounted && _ownsTarget) addAttachments(files);
    } catch (error) {
      if (ref.mounted && _ownsTarget) _update(error: '$error');
    }
  }

  void addAttachments(List<PickedPaneChatAttachment> files) {
    if (!_ownsTarget || files.isEmpty) return;
    const supportedImages = {
      'image/png',
      'image/jpeg',
      'image/gif',
      'image/webp',
    };
    if (files.any(
      (file) => file.isImage && !supportedImages.contains(file.mimeType),
    )) {
      _update(
        error: '이 이미지 형식은 지원하지 않습니다. PNG, JPEG, GIF, WebP로 변환한 후 첨부해 주세요.',
      );
      return;
    }
    final capabilities = _pane!.snapshot.composer;
    if (!capabilities.attachments) return;
    if (state.attachments.length + files.length > capabilities.maxAttachments ||
        files.any(
          (file) => file.bytes.length > capabilities.maxAttachmentBytes,
        )) {
      _update(
        error:
            '첨부는 ${capabilities.maxAttachments}개까지, 파일당 ${capabilities.maxAttachmentBytes ~/ (1024 * 1024)}MiB까지 가능합니다.',
      );
      return;
    }
    final added = files
        .map(
          (file) =>
              PaneChatDraftAttachment(localId: '${++_localId}', file: file),
        )
        .toList();
    _revision++;
    _update(attachments: [...state.attachments, ...added]);
    for (final item in added) {
      unawaited(upload(item.localId));
    }
  }

  Future<void> upload(String localId) async {
    final connection = _connection;
    final item = state.attachments
        .where((a) => a.localId == localId)
        .firstOrNull;
    if (!_ownsTarget ||
        connection == null ||
        item == null ||
        item.isUploading) {
      return;
    }
    void replace(PaneChatDraftAttachment updated) => _update(
      attachments: [
        for (final a in state.attachments)
          if (a.localId == localId) updated else a,
      ],
    );
    replace(
      PaneChatDraftAttachment(
        localId: localId,
        file: item.file,
        isUploading: true,
      ),
    );
    try {
      final id = await ref
          .read(desktopClientProvider)
          .stagePaneChatAttachment(
            connection,
            target.taskId,
            target.targetToken,
            name: item.file.name,
            mimeType: item.file.mimeType,
            bytes: item.file.bytes,
          );
      if (!ref.mounted) return;
      final exists = state.attachments.any((a) => a.localId == localId);
      if (!exists) {
        await ref
            .read(desktopClientProvider)
            .removePaneChatAttachment(
              connection,
              target.taskId,
              target.targetToken,
              id,
            );
        return;
      }
      replace(
        PaneChatDraftAttachment(localId: localId, file: item.file, id: id),
      );
    } catch (error) {
      if (ref.mounted) {
        replace(
          PaneChatDraftAttachment(
            localId: localId,
            file: item.file,
            error: '업로드 실패 · 다시 시도',
          ),
        );
      }
    }
  }

  Future<void> remove(String localId) async {
    final item = state.attachments
        .where((a) => a.localId == localId)
        .firstOrNull;
    if (item == null || state.isSending) return;
    _revision++;
    _update(
      attachments: state.attachments
          .where((a) => a.localId != localId)
          .toList(),
    );
    final connection = _connection;
    if (item.id == null || connection == null) return;
    try {
      await ref
          .read(desktopClientProvider)
          .removePaneChatAttachment(
            connection,
            target.taskId,
            target.targetToken,
            item.id!,
          );
    } catch (_) {
      if (ref.mounted) _update(error: '첨부 정리를 완료하지 못했습니다. 전송에서는 제외했습니다.');
    }
  }

  Future<bool> send() async {
    if (!_ownsTarget || !state.canSend || _pane?.canPrompt != true) {
      return false;
    }
    final text = state.text;
    final sent = state.attachments;
    final ids = sent.map((a) => a.id!).toList();
    final revision = _revision;
    final signature = '$text\u0000${ids.join(',')}';
    if (_requestSignature != signature) {
      _requestSignature = signature;
      final random = Random.secure();
      _requestId = List.generate(
        24,
        (_) => random.nextInt(256).toRadixString(16).padLeft(2, '0'),
      ).join();
    }
    _update(sending: true);
    final accepted = await ref
        .read(
          paneChatProvider((taskId: target.taskId, paneId: target.paneId))
              .notifier,
        )
        .send(text, attachmentIds: ids, requestId: _requestId);
    if (!ref.mounted) return accepted;
    if (accepted) {
      final sentIds = sent.map((a) => a.localId).toSet();
      _update(
        text: revision == _revision ? '' : state.text,
        attachments: state.attachments
            .where((a) => !sentIds.contains(a.localId))
            .toList(),
        sending: false,
        suggestions: [],
      );
      _requestId = null;
      _requestSignature = null;
    } else {
      _update(
        sending: false,
        error: _pane?.actionError == null
            ? '전송하지 못했습니다. 내용을 유지했습니다. 다시 시도해 주세요.'
            : null,
      );
    }
    return accepted;
  }

  void dismissSuggestions() {
    _suggestionEpoch++;
    _suggestionTimer?.cancel();
    _update(suggestions: [], loading: false);
  }

  void suggest(String kind, String query) {
    _suggestionTimer?.cancel();
    final epoch = ++_suggestionEpoch;
    _update(suggestions: [], loading: true);
    _suggestionTimer = Timer(const Duration(milliseconds: 180), () async {
      final connection = _connection;
      if (!_ownsTarget || connection == null) return;
      try {
        final items = await ref
            .read(desktopClientProvider)
            .getPaneChatSuggestions(
              connection,
              target.taskId,
              target.targetToken,
              kind,
              query,
            );
        if (ref.mounted && epoch == _suggestionEpoch && _ownsTarget) {
          _update(suggestions: items, loading: false);
        }
      } catch (_) {
        if (ref.mounted && epoch == _suggestionEpoch && _ownsTarget) {
          _update(
            suggestions: [],
            loading: false,
            suggestionError: '추천을 불러오지 못했습니다. 다시 입력해 주세요.',
          );
        }
      }
    });
  }
}
