import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../data/ai_accounts_repository.dart';
import '../domain/ai_account.dart';
import '../domain/ai_usage.dart';

final aiAccountsRepositoryProvider = Provider<AiAccountsRepository>(
  (ref) => AiAccountsRepository(ref.watch(desktopClientProvider)),
);
final aiAccountsProvider = FutureProvider.autoDispose<List<AiAccount>>(
  (ref) => _query(
    ref,
    (repository, connection) => repository.fetchAccounts(connection),
  ),
  retry: (_, _) => null,
);
final aiUsageProvider = FutureProvider.autoDispose<AiUsageSnapshot>(
  (ref) => _query(
    ref,
    (repository, connection) => repository.fetchUsage(connection),
  ),
  retry: (_, _) => null,
);
final aiAccountActionsProvider =
    AsyncNotifierProvider.autoDispose<AiAccountActions, void>(
      AiAccountActions.new,
      retry: (_, _) => null,
    );

bool _isCurrent(Ref ref, DesktopConnection connection) {
  if (!ref.mounted) return false;
  final active = ref.read(connectionControllerProvider).value?.activeConnection;
  return active?.id == connection.id && active?.token == connection.token;
}

Future<T> _query<T>(
  Ref ref,
  Future<T> Function(AiAccountsRepository, DesktopConnection) fetch,
) async {
  final connection = (await ref.watch(connectionControllerProvider.future))
      .activeConnection;
  if (connection == null) throw StateError('연결된 PC가 없습니다.');
  try {
    return await fetch(ref.read(aiAccountsRepositoryProvider), connection);
  } on DesktopRequestException catch (error) {
    if (error.isUnauthorized && _isCurrent(ref, connection)) {
      await ref
          .read(connectionControllerProvider.notifier)
          .forgetLocally(connection.id, connection.token);
    }
    rethrow;
  }
}

class AiAccountActions extends AsyncNotifier<void> {
  int _generation = 0;
  @override
  Future<void> build() async {
    _generation++;
    await ref.watch(connectionControllerProvider.future);
  }

  Future<T?> _mutate<T>(
    Future<T> Function(AiAccountsRepository, DesktopConnection) operation,
  ) async {
    if (state.isLoading) return null;
    final connection = ref
        .read(connectionControllerProvider)
        .value
        ?.activeConnection;
    if (connection == null) return null;
    final generation = _generation;
    state = const AsyncLoading();
    try {
      final result = await operation(
        ref.read(aiAccountsRepositoryProvider),
        connection,
      );
      if (!_isCurrent(ref, connection) || generation != _generation) {
        return null;
      }
      state = const AsyncData(null);
      ref.invalidate(aiAccountsProvider);
      ref.invalidate(aiUsageProvider);
      return result;
    } catch (error, stack) {
      if (!_isCurrent(ref, connection) || generation != _generation) {
        return null;
      }
      state = AsyncError(error, stack);
      if (error is DesktopRequestException && error.isUnauthorized) {
        await ref
            .read(connectionControllerProvider.notifier)
            .forgetLocally(connection.id, connection.token);
      }
      return null;
    }
  }

  Future<String?> add(AiAccountProvider provider, String name) => _mutate(
    (repository, connection) =>
        repository.addAccount(connection, provider, name),
  );
  Future<bool> remove(String accountId) async =>
      await _mutate((repository, connection) async {
        await repository.removeAccount(connection, accountId);
        return true;
      }) ??
      false;
  PaneStream openLogin(DesktopConnection connection, String accountId) {
    if (!_isCurrent(ref, connection)) throw StateError('선택한 PC가 변경되었습니다.');
    return ref
        .read(aiAccountsRepositoryProvider)
        .openLogin(connection, accountId);
  }
}
