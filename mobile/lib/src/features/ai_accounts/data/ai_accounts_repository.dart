import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../domain/ai_account.dart';
import '../domain/ai_usage.dart';

class AiAccountsRepository {
  const AiAccountsRepository(this.client);
  final DesktopClient client;
  static const _providerTimeout = Duration(seconds: 90);
  Future<List<AiAccount>> fetchAccounts(DesktopConnection connection) async {
    final envelope = await client.requestJson(connection, 'GET', [
      'ai',
      'accounts',
    ], timeout: _providerTimeout);
    final accounts = envelope['accounts'];
    if (accounts is! List) throw const FormatException('AI 계정 목록을 읽지 못했습니다.');
    return List.unmodifiable(
      accounts.map((raw) {
        final value = Map<String, dynamic>.from(raw as Map);
        final provider = AiAccountProvider.values
            .where((provider) => provider.name == value['provider'])
            .firstOrNull;
        if (provider == null || value['accountId'] is! String) {
          throw const FormatException('AI 계정 정보가 올바르지 않습니다.');
        }
        return AiAccount(
          accountId: value['accountId'] as String,
          provider: provider,
          label: value['label'] as String? ?? provider.label,
          accountName: value['accountName'] as String?,
          isLoggedIn: value['isLoggedIn'] == true,
          isRemovable: value['isRemovable'] == true,
          planName: value['planName'] as String?,
        );
      }),
    );
  }

  Future<String> addAccount(
    DesktopConnection connection,
    AiAccountProvider provider,
    String name,
  ) async {
    final envelope = await client.requestJson(
      connection,
      'POST',
      ['ai', 'accounts'],
      body: {'provider': provider.name, 'accountName': name},
      timeout: _providerTimeout,
    );
    final id = envelope['accountId'];
    if (id is! String || id.isEmpty) {
      throw const FormatException('새 계정을 확인하지 못했습니다.');
    }
    return id;
  }

  Future<void> removeAccount(
    DesktopConnection connection,
    String accountId,
  ) async {
    await client.requestJson(connection, 'DELETE', [
      'ai',
      'accounts',
      accountId,
    ], timeout: _providerTimeout);
  }

  PaneStream openLogin(DesktopConnection connection, String accountId) =>
      client.openAuthenticatedTerminal(connection, [
        'ai',
        'accounts',
        accountId,
        'login',
      ]);
  Future<AiUsageSnapshot> fetchUsage(DesktopConnection connection) async {
    final envelope = await client.requestJson(connection, 'GET', [
      'ai',
      'usage',
    ], timeout: _providerTimeout);
    final value = Map<String, dynamic>.from(envelope['usage'] as Map);
    return AiUsageSnapshot(
      fetchedAt: DateTime.tryParse(value['fetchedAt']?.toString() ?? ''),
      accounts: List.unmodifiable(
        (value['accounts'] as List).map((raw) {
          final account = Map<String, dynamic>.from(raw as Map);
          return AiUsageAccount(
            accountId: account['accountId'] as String,
            provider: account['provider'] as String,
            label: account['label'] as String,
            status: account['status'] as String,
            planName: account['planName'] as String?,
            reason: account['reason'] as String?,
            fetchedAt: DateTime.tryParse(
              account['fetchedAt']?.toString() ?? '',
            ),
            windows: List.unmodifiable(
              (account['windows'] as List).map((rawWindow) {
                final window = Map<String, dynamic>.from(rawWindow as Map);
                return AiUsageWindow(
                  kind: window['kind'] as String,
                  modelName: window['modelName'] as String?,
                  usedPercent: (window['usedPercent'] as num).toDouble(),
                  resetsAt: DateTime.tryParse(
                    window['resetsAt']?.toString() ?? '',
                  ),
                );
              }),
            ),
          );
        }),
      ),
    );
  }
}
