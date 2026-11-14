import 'package:flutter/material.dart';

import '../../../../common/constants/app_sizes.dart';
import '../../../../common/l10n/app_localizations.dart';
import '../../../../common/network/desktop_client.dart';

String pcRequestError(Object? error) => error is DesktopRequestException
    ? (error.message ?? 'PC 연결과 데스크탑 앱 버전을 확인해 주세요.')
    : 'PC에 요청하지 못했습니다. 네트워크를 확인해 주세요.';

class PcRequestFailure extends StatelessWidget {
  const PcRequestFailure({
    required this.error,
    required this.onRetry,
    super.key,
  });
  final Object? error;
  final VoidCallback onRetry;
  @override
  Widget build(BuildContext context) => Center(
    child: Padding(
      padding: const EdgeInsets.all(AppSizes.p24),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(context.tr(pcRequestError(error)), textAlign: TextAlign.center),
          TextButton(onPressed: onRetry, child: Text(context.tr('다시 시도'))),
        ],
      ),
    ),
  );
}
