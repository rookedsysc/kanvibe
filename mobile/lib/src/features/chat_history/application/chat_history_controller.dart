import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../common/network/desktop_client.dart';
import '../../../common/network/desktop_connection.dart';
import '../../../common/providers.dart';
import '../domain/chat_history.dart';

final chatHistoryProvider = AsyncNotifierProvider.autoDispose
    .family<ChatHistoryController, ChatHistoryState, String>(
      ChatHistoryController.new,
      retry: (_, _) => null,
    );

class ChatHistoryState {
  const ChatHistoryState(
    this.page, {
    this.isLoadingMore = false,
    this.loadMoreFailed = false,
  });
  final ChatHistoryPage page;
  final bool isLoadingMore;
  final bool loadMoreFailed;
}

class ChatHistoryController extends AsyncNotifier<ChatHistoryState> {
  ChatHistoryController(this.taskId);
  final String taskId;
  String _query = '';
  int _generation = 0;
  DesktopConnection? _connection;

  @override
  Future<ChatHistoryState> build() async {
    _generation++;
    final connection = (await ref.watch(connectionControllerProvider.future))
        .activeConnection;
    _connection = connection;
    if (connection == null) throw StateError('연결된 PC가 없습니다');
    return ChatHistoryState(await _fetch(connection));
  }

  Future<ChatHistoryPage> _fetch(
    DesktopConnection connection, {
    String? cursor,
  }) async {
    try {
      return await ref
          .read(desktopClientProvider)
          .fetchChatHistory(connection, taskId, query: _query, cursor: cursor);
    } on DesktopRequestException catch (error) {
      if (error.isUnauthorized && ref.mounted) {
        await ref
            .read(connectionControllerProvider.notifier)
            .forgetLocally(connection.id, connection.token);
      }
      rethrow;
    }
  }

  void search(String query) {
    _query = query.trim();
    ref.invalidateSelf();
  }

  Future<void> loadMore() async {
    final previous = state.value;
    final connection = _connection;
    if (previous == null ||
        previous.isLoadingMore ||
        previous.page.nextCursor == null ||
        connection == null) {
      return;
    }
    final generation = _generation;
    state = AsyncData(ChatHistoryState(previous.page, isLoadingMore: true));
    try {
      final page = await _fetch(connection, cursor: previous.page.nextCursor);
      if (!ref.mounted || generation != _generation) return;
      final seen = previous.page.sessions.map((session) => session.key).toSet();
      state = AsyncData(
        ChatHistoryState(
          ChatHistoryPage(
            sessions: [
              ...previous.page.sessions,
              ...page.sessions.where((session) => seen.add(session.key)),
            ],
            providers: page.providers,
            nextCursor: page.nextCursor,
          ),
        ),
      );
    } catch (_) {
      if (ref.mounted && generation == _generation) {
        state = AsyncData(
          ChatHistoryState(previous.page, loadMoreFailed: true),
        );
      }
    }
  }
}

final chatMessagesProvider = AsyncNotifierProvider.autoDispose
    .family<
      ChatMessagesController,
      ChatMessagesState,
      ({String taskId, ChatSession session})
    >(ChatMessagesController.new, retry: (_, _) => null);

class ChatMessagesState {
  const ChatMessagesState(
    this.page, {
    this.isLoadingMore = false,
    this.loadMoreFailed = false,
  });
  final ChatMessagePage page;
  final bool isLoadingMore;
  final bool loadMoreFailed;
}

class ChatMessagesController extends AsyncNotifier<ChatMessagesState> {
  ChatMessagesController(this.target);
  final ({String taskId, ChatSession session}) target;
  int _generation = 0;
  DesktopConnection? _connection;

  @override
  Future<ChatMessagesState> build() async {
    _generation++;
    final connection = (await ref.watch(connectionControllerProvider.future))
        .activeConnection;
    _connection = connection;
    if (connection == null) throw StateError('연결된 PC가 없습니다');
    return ChatMessagesState(await _fetch(connection));
  }

  Future<ChatMessagePage> _fetch(
    DesktopConnection connection, {
    String? cursor,
  }) async {
    try {
      return await ref
          .read(desktopClientProvider)
          .fetchChatMessages(
            connection,
            target.taskId,
            target.session,
            cursor: cursor,
          );
    } on DesktopRequestException catch (error) {
      if (error.isUnauthorized && ref.mounted) {
        await ref
            .read(connectionControllerProvider.notifier)
            .forgetLocally(connection.id, connection.token);
      }
      rethrow;
    }
  }

  Future<void> loadMore() async {
    final previous = state.value;
    final connection = _connection;
    if (previous == null ||
        previous.isLoadingMore ||
        previous.page.nextCursor == null ||
        connection == null) {
      return;
    }
    final generation = _generation;
    state = AsyncData(ChatMessagesState(previous.page, isLoadingMore: true));
    try {
      final page = await _fetch(connection, cursor: previous.page.nextCursor);
      if (!ref.mounted || generation != _generation) return;
      state = AsyncData(
        ChatMessagesState(
          ChatMessagePage(
            messages: [...previous.page.messages, ...page.messages],
            nextCursor: page.nextCursor,
          ),
        ),
      );
    } catch (_) {
      if (ref.mounted && generation == _generation) {
        state = AsyncData(
          ChatMessagesState(previous.page, loadMoreFailed: true),
        );
      }
    }
  }
}
