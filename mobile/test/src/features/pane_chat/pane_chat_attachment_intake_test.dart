import 'dart:async';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kanvibe_mobile/src/features/pane_chat/data/pane_chat_attachment_intake.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('kanvibe/test-pane-chat-attachments');
  final calls = <MethodCall>[];
  Future<Object?> Function(MethodCall)? handler;

  setUp(() {
    calls.clear();
    handler = null;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          calls.add(call);
          return handler?.call(call);
        });
  });

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
  });

  PaneChatAttachmentIntake create({PaneChatFilePicker? picker}) {
    final intake = PaneChatAttachmentIntake(
      channel: channel,
      pickFiles: picker,
    );
    addTearDown(intake.dispose);
    return intake;
  }

  Future<void> native(String method, Object arguments) =>
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .handlePlatformMessage(
            channel.name,
            const StandardMethodCodec().encodeMethodCall(
              MethodCall(method, arguments),
            ),
            (_) {},
          );

  Map<String, Object> file({String name = 'photo.png', Uint8List? bytes}) => {
    'name': name,
    'mimeType': 'image/png',
    'bytes': bytes ?? Uint8List.fromList([1, 2, 3]),
  };

  test('사진과 문서는 각각 네이티브 필터를 사용하고 bytes와 파일명을 유지한다', () async {
    final types = <FileType>[];
    final intake = create(
      picker: (type) async {
        types.add(type);
        return [
          _PickedFile(
            name: type == FileType.image ? '사진.PNG' : 'notes.txt',
            bytes: [1, 2, 3],
          ),
        ];
      },
    );
    await intake.setActive('a', true);
    final photo = (await intake.pickPhotos('a')).single;
    final document = (await intake.pickFiles('a')).single;
    expect(types, [FileType.image, FileType.any]);
    expect(photo.name, '사진.PNG');
    expect(photo.mimeType, 'image/png');
    expect(photo.bytes, [1, 2, 3]);
    expect(document.mimeType, 'text/plain');
  });

  test('파일 선택 취소는 빈 결과이고 최대 10개 경계를 지킨다', () async {
    var count = 0;
    final intake = create(
      picker: (_) async => List.generate(count, (_) => _PickedFile()),
    );
    await intake.setActive('a', true);
    expect(await intake.pickFiles('a'), isEmpty);
    count = 10;
    expect(await intake.pickFiles('a'), hasLength(10));
    count = 11;
    await expectLater(intake.pickFiles('a'), throwsFormatException);
  });

  test('선언 크기가 10MiB를 넘으면 파일 스트림을 열지 않는다', () async {
    var opened = false;
    final intake = create(
      picker: (_) async => [
        _PickedFile(
          declaredLength: PaneChatAttachmentIntake.maxFileBytes + 1,
          stream: () {
            opened = true;
            return const Stream.empty();
          },
        ),
      ],
    );
    await intake.setActive('a', true);
    await expectLater(intake.pickFiles('a'), throwsFormatException);
    expect(opened, isFalse);
  });

  test('크기를 모르는 스트림도 10MiB까지만 읽고 넘으면 취소한다', () async {
    final bytes = Uint8List(PaneChatAttachmentIntake.maxFileBytes);
    var overflow = false;
    final intake = create(
      picker: (_) async => [
        _PickedFile(
          declaredLength: null,
          stream: () =>
              Stream.fromIterable([bytes, if (overflow) Uint8List(1)]),
        ),
      ],
    );
    await intake.setActive('a', true);
    expect((await intake.pickFiles('a')).single.bytes.length, bytes.length);
    overflow = true;
    await expectLater(intake.pickFiles('a'), throwsFormatException);
  });

  test('다른 pane으로 옮긴 뒤 완료된 파일 선택은 새 pane에 추가하지 않는다', () async {
    final picker = Completer<List<PlatformFile>>();
    final intake = create(picker: (_) => picker.future);
    await intake.setActive('a', true);
    final pending = intake.pickFiles('a');
    await intake.setActive('b', true);
    picker.complete([_PickedFile()]);
    expect(await pending, isEmpty);
  });

  test('다른 pane으로 옮긴 뒤 파일 선택 오류도 현재 pane을 방해하지 않는다', () async {
    final picker = Completer<List<PlatformFile>>();
    final intake = create(picker: (_) => picker.future);
    await intake.setActive('a', true);
    final pending = expectLater(intake.pickFiles('a'), completion(isEmpty));
    await intake.setActive('b', true);
    picker.completeError(PlatformException(code: 'cancelled'));
    await pending;
  });

  test('이전 pane의 비활성화가 현재 pane의 붙여넣기를 끄지 않는다', () async {
    handler = (call) async =>
        call.method == 'readClipboardImage' ? file() : null;
    final intake = create();
    await intake.setActive('a', true);
    await intake.setActive('b', true);
    await intake.setActive('a', false);
    expect((await intake.readClipboardImage('b'))!.bytes, [1, 2, 3]);
    expect(
      calls.where((call) => call.method == 'setDropEnabled'),
      hasLength(2),
    );
  });

  test('명시적 이미지 붙여넣기는 native bytes를 사용하고 이미지가 없으면 null이다', () async {
    var hasImage = true;
    handler = (call) async =>
        call.method == 'readClipboardImage' && hasImage ? file() : null;
    final intake = create();
    await intake.setActive('a', true);
    expect((await intake.readClipboardImage('a'))!.isImage, isTrue);
    hasImage = false;
    expect(await intake.readClipboardImage('a'), isNull);
  });

  test('소유 pane이 변경된 뒤 도착한 클립보드 bytes는 버린다', () async {
    final read = Completer<Object?>();
    handler = (call) async =>
        call.method == 'readClipboardImage' ? read.future : null;
    final intake = create();
    await intake.setActive('a', true);
    final pending = intake.readClipboardImage('a');
    await intake.setActive('b', true);
    read.complete(file());
    expect(await pending, isNull);
  });

  test('소유 pane이 변경된 뒤 발생한 클립보드 오류는 버린다', () async {
    final read = Completer<Object?>();
    handler = (call) async =>
        call.method == 'readClipboardImage' ? read.future : null;
    final intake = create();
    await intake.setActive('a', true);
    final pending = expectLater(
      intake.readClipboardImage('a'),
      completion(isNull),
    );
    await intake.setActive('b', true);
    read.completeError(PlatformException(code: 'read-failed'));
    await pending;
  });

  test('Android rich content bytes가 있으면 URI를 다시 읽지 않는다', () async {
    final intake = create();
    await intake.setActive('a', true);
    final result = await intake.readInsertedContent(
      'a',
      KeyboardInsertedContent(
        mimeType: 'image/png',
        uri: 'content://photo/1',
        data: Uint8List.fromList([4, 5]),
      ),
    );
    expect(result!.name, 'clipboard.png');
    expect(result.bytes, [4, 5]);
    expect(calls.where((call) => call.method == 'readContentUri'), isEmpty);
  });

  test('rich content bytes가 없을 때 content URI만 native로 읽는다', () async {
    handler = (call) async => call.method == 'readContentUri' ? file() : null;
    final intake = create();
    await intake.setActive('a', true);
    expect(
      await intake.readInsertedContent(
        'a',
        const KeyboardInsertedContent(
          mimeType: 'image/png',
          uri: 'content://photo/1',
        ),
      ),
      isNotNull,
    );
    await expectLater(
      intake.readInsertedContent(
        'a',
        const KeyboardInsertedContent(
          mimeType: 'image/png',
          uri: 'file:///private/photo.png',
        ),
      ),
      throwsFormatException,
    );
    expect(
      calls.where((call) => call.method == 'readContentUri'),
      hasLength(1),
    );
  });

  test('native drop epoch이 다르면 버리고 현재 소유자에게만 전달한다', () async {
    final intake = create();
    final events = <PaneChatAttachmentDrop>[];
    final subscription = intake.drops.listen(events.add);
    addTearDown(subscription.cancel);
    await intake.setActive('a', true);
    final oldEpoch = (calls.last.arguments as Map)['epoch'];
    await intake.setActive('b', true);
    final epoch = (calls.last.arguments as Map)['epoch'];
    await native('filesDropped', {
      'epoch': oldEpoch,
      'files': [file()],
    });
    await native('filesDropped', {
      'epoch': epoch,
      'files': [file()],
    });
    await Future<void>.delayed(Duration.zero);
    expect(events, hasLength(1));
    expect(events.single.ownerKey, 'b');
    expect(events.single.attachments.single.bytes, [1, 2, 3]);
  });

  test('손상되거나 큰 native drop은 파일 대신 해당 채팅의 오류를 전달한다', () async {
    final intake = create();
    final events = <PaneChatAttachmentDrop>[];
    final subscription = intake.drops.listen(events.add);
    addTearDown(subscription.cancel);
    await intake.setActive('a', true);
    final epoch = (calls.last.arguments as Map)['epoch'];
    await native('filesDropped', {
      'epoch': epoch,
      'files': [
        {'name': 'broken.png'},
      ],
    });
    await native('filesDropped', {
      'epoch': epoch,
      'files': [
        file(bytes: Uint8List(PaneChatAttachmentIntake.maxFileBytes + 1)),
      ],
    });
    await Future<void>.delayed(Duration.zero);
    expect(events, hasLength(2));
    expect(
      events.every((event) => event.attachments.isEmpty && event.error != null),
      isTrue,
    );
  });

  test('비활성 상태에서는 picker와 clipboard를 열지 않는다', () async {
    var picks = 0;
    final intake = create(
      picker: (_) async {
        picks++;
        return [];
      },
    );
    await expectLater(intake.pickFiles('a'), throwsStateError);
    await expectLater(intake.readClipboardImage('a'), throwsStateError);
    expect(picks, 0);
    expect(calls, isEmpty);
  });
}

final class _PickedFile extends PlatformFile {
  _PickedFile({
    this.name = 'file.txt',
    List<int> bytes = const [1],
    this.declaredLength = 1,
    Stream<Uint8List> Function()? stream,
  }) : _stream = stream ?? (() => Stream.value(Uint8List.fromList(bytes)));

  @override
  final String name;
  final int? declaredLength;
  final Stream<Uint8List> Function() _stream;

  @override
  Uri get uri => Uri.file('/picked/$name');
  @override
  get xFile => throw UnsupportedError('첨부는 한도를 검사하며 스트림으로 읽어야 한다.');
  @override
  int? lengthSync() => declaredLength;
  @override
  Future<int?> length() async => declaredLength;
  @override
  Future<Uint8List> readAsBytes() =>
      throw UnsupportedError('첨부는 한도를 검사하며 스트림으로 읽어야 한다.');
  @override
  Stream<Uint8List> readAsByteStream() => _stream();
}
