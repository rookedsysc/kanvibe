import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../../common/l10n/app_localizations.dart';
import '../../../notifications/presentation/background_sync_card.dart';
import '../../application/pc_settings_controller.dart';

class PcSyncSection extends ConsumerWidget {
  const PcSyncSection({super.key});
  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final operation = ref.watch(pcOperationsProvider);
    final saving = ref.watch(pcSettingsProvider).value?.isSaving == true;
    return BackgroundSyncCard(
      title: context.tr('PC 동기화'),
      buttonIdentifier: 'pc-sync-now',
      canStart: !operation.isLoading && !saving,
    );
  }
}
