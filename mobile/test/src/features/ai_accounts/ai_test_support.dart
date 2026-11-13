import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/constants/app_theme.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/common/providers.dart';
import 'package:kanvibe_mobile/src/features/connection/data/connection_store.dart';
import 'package:kanvibe_mobile/src/features/connection/domain/saved_connections.dart';

const fixturePc = DesktopConnection(
  host: 'fixture',
  port: 9746,
  token: 'paired',
);
const otherPc = DesktopConnection(host: 'other', port: 9746, token: 'other');

class AiMemoryConnections extends ConnectionStore {
  SavedConnections registry = const SavedConnections(
    connections: [fixturePc, otherPc],
    activeId: 'fixture:9746',
  );
  @override
  Future<SavedConnections> readRegistry() async => registry;
  @override
  Future<void> writeRegistry(SavedConnections value) async {
    registry = value;
  }
}

class AiStreamClient extends DesktopClient {
  AiStreamClient(Future<http.Response> Function(http.Request) handler)
    : super(httpClient: MockClient(handler));
  final frames = StreamController<String>();
  final writes = <String>[];
  final sizes = <({int cols, int rows})>[];
  List<String>? loginSegments;
  int closes = 0;
  int? closeCode;
  @override
  PaneStream openAuthenticatedTerminal(
    DesktopConnection connection,
    List<String> segments,
  ) {
    loginSegments = segments;
    return PaneStream(
      output: frames.stream,
      onWrite: writes.add,
      onResize: (cols, rows) => sizes.add((cols: cols, rows: rows)),
      onClose: () async {
        closes++;
      },
      readCloseCode: () => closeCode,
    );
  }
}

http.Response aiResponse(Object body, {int status = 200}) => http.Response(
  jsonEncode(body),
  status,
  headers: {'content-type': 'application/json; charset=utf-8'},
);
Map<String, dynamic> fixtureAccount({bool removable = true}) => {
  'accountId': 'opaque-id',
  'provider': 'claude',
  'accountName': 'work',
  'label': 'Work Account',
  'isLoggedIn': false,
  'isRemovable': removable,
  'planName': 'Pro',
};
Future<ProviderContainer> mountAi(
  WidgetTester tester,
  Widget screen,
  DesktopClient client, {
  double textScale = 1,
}) async {
  final container = ProviderContainer(
    overrides: [
      desktopClientProvider.overrideWithValue(client),
      connectionStoreProvider.overrideWithValue(AiMemoryConnections()),
    ],
  );
  await tester.pumpWidget(
    UncontrolledProviderScope(
      container: container,
      child: MaterialApp(
        theme: buildAppTheme(),
        builder: (context, child) => MediaQuery(
          data: MediaQuery.of(context)
              .copyWith(textScaler: TextScaler.linear(textScale)),
          child: child!,
        ),
        home: screen,
      ),
    ),
  );
  await tester.pump();
  await tester.pump(const Duration(milliseconds: 100));
  addTearDown(() async {
    await tester.pumpWidget(const SizedBox());
    container.dispose();
    client.close();
  });
  return container;
}
