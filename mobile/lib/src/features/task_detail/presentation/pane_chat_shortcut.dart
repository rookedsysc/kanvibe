import 'package:flutter/services.dart';
import 'package:flutter/widgets.dart';

SingleActivator paneChatShortcut(TargetPlatform platform) {
  final usesMeta =
      platform == TargetPlatform.macOS || platform == TargetPlatform.iOS;
  return SingleActivator(
    LogicalKeyboardKey.keyJ,
    meta: usesMeta,
    control: !usesMeta,
    shift: true,
    includeRepeats: false,
  );
}
