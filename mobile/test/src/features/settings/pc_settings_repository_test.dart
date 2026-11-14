import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:http/http.dart' as http;
import 'package:http/testing.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_client.dart';
import 'package:kanvibe_mobile/src/common/network/desktop_connection.dart';
import 'package:kanvibe_mobile/src/features/settings/data/pc_settings_repository.dart';
import 'package:kanvibe_mobile/src/features/task_create/domain/session_type.dart';

const pc = DesktopConnection(host: 'fixture', port: 9746, token: 'paired');
const settingsJson = {
  'defaultSessionType': 'herdr',
  'backgroundSync': {'isEnabled': true, 'intervalMs': 600000},
};
void main() {
  test('PC 기본 세션과 동기화 주기를 읽고 지정한 필드만 저장한다', () async {
    final requests = <http.Request>[];
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        requests.add(request);
        return http.Response(
          jsonEncode({'success': true, 'settings': settingsJson}),
          200,
        );
      }),
    );
    addTearDown(client.close);
    final repository = PcSettingsRepository(client);
    final settings = await repository.fetchSettings(pc);
    expect(settings.defaultSessionType, SessionType.herdr);
    expect(settings.intervalMinutes, 10);
    await repository.saveSettings(pc, defaultSessionType: SessionType.tmux);
    expect(jsonDecode(requests.last.body), {'defaultSessionType': 'tmux'});
    await repository.saveSettings(pc, isSyncEnabled: false, intervalMinutes: 5);
    expect(jsonDecode(requests.last.body), {
      'backgroundSync': {'isEnabled': false, 'intervalMs': 300000},
    });
  });
  test('일괄 변경은 미리보기와 이전 세션 종료 선택을 전송하고 결과를 읽는다', () async {
    final client = DesktopClient(
      httpClient: MockClient((request) async {
        if (request.method == 'GET') {
          expect(request.url.queryParameters, {'sessionType': 'tmux'});
          return http.Response(
            '{"success":true,"targets":{"taskCount":4,"liveSessionCount":2,"sshHosts":[null,"work"]}}',
            200,
          );
        }
        expect(jsonDecode(request.body), {
          'sessionType': 'tmux',
          'shouldCloseExistingSessions': true,
        });
        return http.Response(
          '{"success":true,"result":{"changedCount":3,"failedCount":1}}',
          200,
        );
      }),
    );
    addTearDown(client.close);
    final repository = PcSettingsRepository(client);
    expect(
      (await repository.fetchTargets(pc, SessionType.tmux)).liveSessionCount,
      2,
    );
    final result = await repository.changeSessions(pc, SessionType.tmux, true);
    expect(result.changedCount, 3);
    expect(result.failedCount, 1);
  });
}
