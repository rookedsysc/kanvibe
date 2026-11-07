import 'package:flutter/material.dart';

import '../../../../common/constants/app_colors.dart';
import '../../../../common/constants/app_sizes.dart';

/// 단축키 설정 페이지와 콤보 편집기가 함께 쓰는 설정 화면 골격.
///
/// 큰 제목, 짧은 섹션 라벨, 둥근 카드 묶음, 하단에 고정된 주 버튼으로 한 화면에 한 가지 결정만 보이게 한다.
class SettingsPageTitle extends StatelessWidget {
  const SettingsPageTitle({required this.title, this.subtitle, super.key});

  final String title;
  final String? subtitle;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(top: AppSizes.p8, bottom: AppSizes.p24),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          title,
          style: const TextStyle(
            fontSize: 26,
            height: 1.3,
            fontWeight: FontWeight.w700,
            color: AppColors.textPrimary,
          ),
        ),
        if (subtitle case final subtitle?) ...[
          const SizedBox(height: AppSizes.p4),
          Text(
            subtitle,
            style: const TextStyle(
              fontSize: 15,
              color: AppColors.textSecondary,
            ),
          ),
        ],
      ],
    ),
  );
}

class SettingsSectionLabel extends StatelessWidget {
  const SettingsSectionLabel(this.label, {this.caption, super.key});

  final String label;
  final String? caption;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(top: AppSizes.p24, bottom: AppSizes.p12),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          label,
          style: const TextStyle(
            fontSize: 15,
            fontWeight: FontWeight.w600,
            color: AppColors.textPrimary,
          ),
        ),
        if (caption case final caption?)
          Text(
            caption,
            style: const TextStyle(
              fontSize: 13,
              color: AppColors.textSecondary,
            ),
          ),
      ],
    ),
  );
}

/// 설정 행을 묶는 둥근 카드. 행 사이에는 구분선을 두지 않고 여백으로만 나눈다
class SettingsCard extends StatelessWidget {
  const SettingsCard({required this.children, super.key});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) => Material(
    color: AppColors.bgSurface,
    clipBehavior: Clip.antiAlias,
    shape: RoundedRectangleBorder(
      borderRadius: BorderRadius.circular(AppSizes.radiusCard),
    ),
    child: Padding(
      padding: const EdgeInsets.symmetric(vertical: AppSizes.p8),
      child: Column(mainAxisSize: MainAxisSize.min, children: children),
    ),
  );
}

/// 화면 아래에 고정된 주 버튼. 오류는 버튼 바로 위에 보여 눈을 옮기지 않게 한다
class SettingsBottomAction extends StatelessWidget {
  const SettingsBottomAction({
    required this.label,
    required this.onPressed,
    this.error,
    super.key,
  });

  final String label;
  final VoidCallback? onPressed;
  final String? error;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.fromLTRB(
      AppSizes.p24,
      AppSizes.p8,
      AppSizes.p24,
      AppSizes.p16,
    ),
    child: Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        if (error case final error?)
          Padding(
            padding: const EdgeInsets.only(bottom: AppSizes.p12),
            child: Text(
              error,
              style: const TextStyle(color: AppColors.statusError),
            ),
          ),
        FilledButton(
          style: FilledButton.styleFrom(
            minimumSize: const Size.fromHeight(_bottomActionHeight),
            textStyle: const TextStyle(
              fontSize: 16,
              fontWeight: FontWeight.w600,
            ),
            shape: RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(AppSizes.radiusCard),
            ),
          ),
          onPressed: onPressed,
          child: Text(label),
        ),
      ],
    ),
  );
}

const _bottomActionHeight = 56.0;
