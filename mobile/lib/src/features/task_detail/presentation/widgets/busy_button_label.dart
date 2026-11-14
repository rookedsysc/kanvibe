import 'package:flutter/material.dart';

import '../../../../common/constants/app_sizes.dart';

/// 시트 버튼 안의 문구. 고른 버튼이 요청을 기다리는 동안 무엇을 하는지와 진행 표시를 함께 보인다
class BusyButtonLabel extends StatelessWidget {
  const BusyButtonLabel({
    required this.label,
    required this.busyLabel,
    required this.isBusy,
    super.key,
  });

  final String label;
  final String busyLabel;
  final bool isBusy;

  @override
  Widget build(BuildContext context) {
    if (!isBusy) {
      return Text(label);
    }
    return Semantics(
      liveRegion: true,
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          const SizedBox.square(
            dimension: AppSizes.p16,
            child: CircularProgressIndicator(strokeWidth: 2),
          ),
          const SizedBox(width: AppSizes.p8),
          Flexible(child: Text(busyLabel, overflow: TextOverflow.ellipsis)),
        ],
      ),
    );
  }
}
