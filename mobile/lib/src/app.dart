import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'common/constants/app_theme.dart';
import 'common/providers.dart';
import 'features/board/presentation/board_screen.dart';
import 'features/connection/presentation/connection_screen.dart';

/// 앱 전체. 저장된 연결이 있으면 보드로, 없으면 연결 화면으로 연다
class KanvibeMobileApp extends ConsumerWidget {
  const KanvibeMobileApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final connection = ref.watch(connectionControllerProvider);

    return MaterialApp(
      title: 'KanVibe',
      debugShowCheckedModeBanner: false,
      theme: buildAppTheme(),
      home: connection.when(
        loading: () =>
            const Scaffold(body: Center(child: CircularProgressIndicator())),
        error: (_, _) => const ConnectionScreen(),
        data: (data) =>
            data == null ? const ConnectionScreen() : const BoardScreen(),
      ),
    );
  }
}
