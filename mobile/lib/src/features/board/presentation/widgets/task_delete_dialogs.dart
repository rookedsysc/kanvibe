import 'package:flutter/material.dart';

import '../../../../common/constants/app_colors.dart';

/// 삭제 확인. 보드의 길게 누르기와 상태 시트가 같은 문구로 묻는다
Future<bool> confirmTaskDeletion(BuildContext context) => _askConfirmation(
  context,
  title: '이 작업을 삭제하시겠습니까?',
  message: 'worktree·브랜치·세션이 함께 지워지고, 같은 태스크를 가진 다른 데스크탑에서도 삭제됩니다.',
  confirmLabel: '삭제',
);

/// 커밋하지 않은 변경 때문에 멈춘 뒤 강제로 지울지 묻는다
Future<bool> confirmForcedTaskDeletion(BuildContext context) =>
    _askConfirmation(
      context,
      title: '커밋하지 않은 변경이 있습니다',
      message: '강제로 삭제하면 이 worktree의 커밋하지 않은 변경도 함께 사라집니다.',
      confirmLabel: '강제 삭제',
    );

Future<bool> confirmTaskRegistrationRemoval(BuildContext context) =>
    _askConfirmation(
      context,
      title: '태스크 등록만 제거할까요?',
      message:
          'Git 저장소를 찾을 수 없습니다. 이 데스크탑의 태스크 등록만 제거합니다. '
          '파일·브랜치·세션과 다른 데스크탑의 태스크는 그대로 유지됩니다.',
      confirmLabel: '등록만 제거',
    );

Future<bool> _askConfirmation(
  BuildContext context, {
  required String title,
  required String message,
  required String confirmLabel,
}) async {
  final isConfirmed = await showDialog<bool>(
    context: context,
    builder: (dialogContext) => AlertDialog(
      backgroundColor: AppColors.bgSurface,
      title: Text(title),
      content: Text(message),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(dialogContext).pop(false),
          child: const Text('취소'),
        ),
        TextButton(
          onPressed: () => Navigator.of(dialogContext).pop(true),
          style: TextButton.styleFrom(foregroundColor: AppColors.statusError),
          child: Text(confirmLabel),
        ),
      ],
    ),
  );
  return isConfirmed ?? false;
}
