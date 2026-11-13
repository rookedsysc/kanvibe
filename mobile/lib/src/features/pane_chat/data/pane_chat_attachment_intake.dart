import 'dart:async';
import 'dart:typed_data';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../domain/pane_chat_attachment.dart';

final paneChatAttachmentIntakeProvider = Provider<PaneChatAttachmentIntake>((
  ref,
) {
  final intake = PaneChatAttachmentIntake();
  ref.onDispose(intake.dispose);
  return intake;
});

typedef PaneChatFilePicker = Future<List<PlatformFile>> Function(FileType type);

class PaneChatAttachmentDrop {
  const PaneChatAttachmentDrop({
    required this.ownerKey,
    this.attachments = const [],
    this.error,
  });

  final String ownerKey;
  final List<PickedPaneChatAttachment> attachments;
  final String? error;
}

class PaneChatAttachmentIntake {
  PaneChatAttachmentIntake({
    PaneChatFilePicker? pickFiles,
    MethodChannel? channel,
  }) : _pickFiles = pickFiles ?? _pickNativeFiles,
       _channel =
           channel ?? const MethodChannel('kanvibe/pane-chat-attachments') {
    _channel.setMethodCallHandler(_onNativeCall);
  }

  static const maxFileBytes = 10 * 1024 * 1024;
  static const maxFiles = 10;

  final PaneChatFilePicker _pickFiles;
  final MethodChannel _channel;
  final _drops = StreamController<PaneChatAttachmentDrop>.broadcast();
  String? _activeOwner;
  int _generation = 0;
  bool _disposed = false;

  Stream<PaneChatAttachmentDrop> get drops => _drops.stream;

  Future<void> setActive(String ownerKey, bool active) async {
    if (_disposed || (!active && _activeOwner != ownerKey)) return;
    _activeOwner = active ? ownerKey : null;
    ++_generation;
    await _channel.invokeMethod<void>('setDropEnabled', {
      'enabled': active,
      'epoch': _generation,
    });
  }

  Future<List<PickedPaneChatAttachment>> pickPhotos(String ownerKey) =>
      _pick(ownerKey, FileType.image);

  Future<List<PickedPaneChatAttachment>> pickFiles(String ownerKey) =>
      _pick(ownerKey, FileType.any);

  Future<List<PickedPaneChatAttachment>> _pick(
    String ownerKey,
    FileType type,
  ) async {
    final epoch = _begin(ownerKey);
    final result = await _readOwned(ownerKey, epoch, () => _pickFiles(type));
    if (result == null) return const [];
    if (result.length > maxFiles) {
      throw const FormatException('첨부는 한 번에 10개까지 추가할 수 있습니다.');
    }
    final picked = <PickedPaneChatAttachment>[];
    for (final file in result) {
      final size = file.lengthSync();
      if (size != null && size > maxFileBytes) throw _tooLarge();
      final data = await _readOwned(
        ownerKey,
        epoch,
        () => _readBounded(file.readAsByteStream()),
      );
      if (data == null) return const [];
      picked.add(_attachment(file.name, _mimeType(file.name), data));
    }
    return picked;
  }

  Future<PickedPaneChatAttachment?> readClipboardImage(String ownerKey) async {
    final epoch = _begin(ownerKey);
    final value = await _readOwned(
      ownerKey,
      epoch,
      () => _channel.invokeMethod<Object?>('readClipboardImage', {
        'epoch': epoch,
      }),
    );
    if (!_isCurrent(ownerKey, epoch) || value == null) return null;
    return _decode(value);
  }

  Future<PickedPaneChatAttachment?> readInsertedContent(
    String ownerKey,
    KeyboardInsertedContent content,
  ) async {
    final epoch = _begin(ownerKey);
    final data = content.data;
    if (data != null && data.isNotEmpty) {
      return _attachment(
        _insertedName(content.mimeType),
        content.mimeType,
        data,
      );
    }
    if (Uri.tryParse(content.uri)?.scheme != 'content') {
      throw const FormatException('키보드가 전달한 파일 주소를 읽을 수 없습니다.');
    }
    final value = await _readOwned(
      ownerKey,
      epoch,
      () => _channel.invokeMethod<Object?>('readContentUri', {
        'uri': content.uri,
        'mimeType': content.mimeType,
        'epoch': epoch,
      }),
    );
    if (!_isCurrent(ownerKey, epoch) || value == null) return null;
    return _decode(value);
  }

  Future<T?> _readOwned<T>(
    String ownerKey,
    int epoch,
    Future<T> Function() read,
  ) async {
    if (!_isCurrent(ownerKey, epoch)) return null;
    try {
      final value = await read();
      return _isCurrent(ownerKey, epoch) ? value : null;
    } catch (_) {
      if (!_isCurrent(ownerKey, epoch)) return null;
      rethrow;
    }
  }

  int _begin(String ownerKey) {
    if (_disposed || ownerKey != _activeOwner) {
      throw StateError('첨부를 추가할 채팅이 활성화되어 있지 않습니다.');
    }
    return _generation;
  }

  bool _isCurrent(String ownerKey, int epoch) =>
      !_disposed && _activeOwner == ownerKey && _generation == epoch;

  Future<void> _onNativeCall(MethodCall call) async {
    final owner = _activeOwner;
    final arguments = call.arguments;
    if (owner == null ||
        _disposed ||
        arguments is! Map ||
        arguments['epoch'] != _generation) {
      return;
    }
    if (call.method == 'dropFailed') {
      _drops.add(
        PaneChatAttachmentDrop(
          ownerKey: owner,
          error: arguments['message'] is String
              ? arguments['message'] as String
              : '파일을 가져오지 못했습니다.',
        ),
      );
      return;
    }
    if (call.method != 'filesDropped') return;
    try {
      final values = arguments['files'];
      if (values is! List || values.isEmpty || values.length > maxFiles) {
        throw const FormatException('가져올 파일은 1개 이상 10개 이하여야 합니다.');
      }
      _drops.add(
        PaneChatAttachmentDrop(
          ownerKey: owner,
          attachments: values.map(_decode).toList(),
        ),
      );
    } on FormatException catch (error) {
      _drops.add(PaneChatAttachmentDrop(ownerKey: owner, error: error.message));
    }
  }

  void dispose() {
    if (_disposed) return;
    _disposed = true;
    _activeOwner = null;
    ++_generation;
    _channel.setMethodCallHandler(null);
    // 엔진이 먼저 해제된 경우 정리 요청 실패로 이미 닫힌 채팅을 복구하지 않는다.
    unawaited(
      _channel
          .invokeMethod<void>('setDropEnabled', {
            'enabled': false,
            'epoch': _generation,
          })
          .catchError((_) {}),
    );
    unawaited(_drops.close());
  }
}

Future<List<PlatformFile>> _pickNativeFiles(FileType type) =>
    FilePicker.pickFiles(
      type: type,
      darwinOptions: const DarwinOptions(
        assetRepresentationMode: DarwinAssetRepresentationMode.compatible,
      ),
    );

Future<Uint8List> _readBounded(Stream<List<int>> stream) async {
  final builder = BytesBuilder(copy: false);
  await for (final chunk in stream) {
    if (builder.length + chunk.length > PaneChatAttachmentIntake.maxFileBytes) {
      throw _tooLarge();
    }
    builder.add(chunk);
  }
  return builder.takeBytes();
}

PickedPaneChatAttachment _decode(Object? value) {
  if (value is! Map ||
      value['name'] is! String ||
      value['mimeType'] is! String ||
      value['bytes'] is! Uint8List) {
    throw const FormatException('가져온 파일 정보가 올바르지 않습니다.');
  }
  return _attachment(
    value['name'] as String,
    value['mimeType'] as String,
    value['bytes'] as Uint8List,
  );
}

PickedPaneChatAttachment _attachment(
  String name,
  String mimeType,
  Uint8List bytes,
) {
  if (bytes.length > PaneChatAttachmentIntake.maxFileBytes) throw _tooLarge();
  final fileName = name.split(RegExp(r'[/\\]')).last.trim();
  if (fileName.isEmpty || RegExp(r'[\x00-\x1f\x7f]').hasMatch(fileName)) {
    throw const FormatException('파일 이름이 올바르지 않습니다.');
  }
  if (!RegExp(r'^[\w!#$&^.+-]+/[\w!#$&^.+-]+$').hasMatch(mimeType)) {
    throw const FormatException('파일 형식이 올바르지 않습니다.');
  }
  return PickedPaneChatAttachment(
    name: fileName,
    mimeType: mimeType,
    bytes: bytes,
  );
}

FormatException _tooLarge() => const FormatException('파일은 10 MiB 이하여야 합니다.');

String _mimeType(String name) => switch (name.split('.').last.toLowerCase()) {
  'png' => 'image/png',
  'jpg' || 'jpeg' => 'image/jpeg',
  'webp' => 'image/webp',
  'gif' => 'image/gif',
  'heic' => 'image/heic',
  'heif' => 'image/heif',
  'avif' => 'image/avif',
  'bmp' => 'image/bmp',
  'tif' || 'tiff' => 'image/tiff',
  'svg' => 'image/svg+xml',
  'pdf' => 'application/pdf',
  'txt' => 'text/plain',
  'md' => 'text/markdown',
  'json' => 'application/json',
  'csv' => 'text/csv',
  _ => 'application/octet-stream',
};

String _insertedName(String mimeType) => switch (mimeType) {
  'image/png' => 'clipboard.png',
  'image/jpeg' => 'clipboard.jpg',
  'image/webp' => 'clipboard.webp',
  'image/gif' => 'clipboard.gif',
  _ => 'clipboard',
};
