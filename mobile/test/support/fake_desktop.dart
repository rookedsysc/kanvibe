import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/connection/data/connection_store.dart';
import 'package:kanvibe_mobile/src/features/task_detail/domain/mirror_pane.dart';

/// 이미 연결된 상태를 흉내 낸다. 화면 검증은 페어링 이후만 다루므로 저장소에 연결이 하나 들어 있다.
///
/// 실제 저장소처럼 값을 들고 있는 이유는, 토큰이 거절됐을 때 기기에서 지워지는지도 봐야 하기 때문이다.
class FakeConnectionStore implements ConnectionStore {
  DesktopConnection? _connection = const DesktopConnection(
    host: '127.0.0.1',
    port: 9736,
    token: 'test-token',
  );

  @override
  Future<DesktopConnection?> read() async => _connection;

  @override
  Future<void> write(DesktopConnection connection) async =>
      _connection = connection;

  @override
  Future<void> clear() async => _connection = null;
}

/// 데스크탑 대신 미리 정한 값을 돌려준다.
///
/// 실제 소켓을 열지 않으므로 pane 출력은 테스트가 직접 흘려보낼 수 있다.
class FakeDesktopClient implements DesktopClient {
  FakeDesktopClient({
    this.board = const {},
    this.boardError,
    this.surfaces,
    this.surfacesError,
  });

  final Map<String, dynamic> board;
  final Object? boardError;
  final TaskSurfaces? surfaces;
  final Object? surfacesError;

  /// pane별로 열린 구독. 테스트가 출력을 밀어 넣고 닫힘 여부를 확인한다
  final openedPanes = <String, StreamController<String>>{};
  final closedPanes = <String>[];

  /// pane별로 모바일이 보낸 입력. 어느 pane이 키를 받았는지까지 봐야 입력 대상이 옮겨졌는지 알 수 있다
  final writtenInput = <String, List<String>>{};

  @override
  Future<Map<String, dynamic>> fetchBoard(DesktopConnection connection) async {
    if (boardError != null) {
      throw boardError!;
    }
    return board;
  }

  @override
  Future<TaskSurfaces> fetchSurfaces(
    DesktopConnection connection,
    String taskId,
  ) async {
    if (surfacesError != null) {
      throw surfacesError!;
    }
    return surfaces ?? const TaskSurfaces(tabs: []);
  }

  @override
  PaneStream openPaneStream(
    DesktopConnection connection,
    String taskId,
    String paneId,
  ) {
    final controller = StreamController<String>.broadcast();
    openedPanes[paneId] = controller;

    return PaneStream(
      output: controller.stream,
      onWrite: (input) => writtenInput.putIfAbsent(paneId, () => []).add(input),
      onClose: () async {
        closedPanes.add(paneId);
        await controller.close();
      },
    );
  }

  @override
  Future<String?> pair({
    required String host,
    required int port,
    required String code,
    required String deviceName,
  }) async => null;

  @override
  void close() {}
}

/// 지정한 화면 크기로 위젯을 띄운다. 폰과 태블릿 배치를 가르는 것은 너비뿐이다
Widget wrapWithApp(
  Widget child, {
  required FakeDesktopClient client,
  required Size size,
}) {
  return ProviderScope(
    overrides: [
      desktopClientProvider.overrideWithValue(client),
      connectionStoreProvider.overrideWithValue(FakeConnectionStore()),
    ],
    child: MaterialApp(
      theme: buildAppTheme(),
      home: MediaQuery(
        data: MediaQueryData(size: size),
        child: SizedBox(width: size.width, height: size.height, child: child),
      ),
    ),
  );
}

const phoneSize = Size(390, 844);
const tabletSize = Size(1024, 768);
