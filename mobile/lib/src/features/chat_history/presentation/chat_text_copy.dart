import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

/// 글을 클립보드에 옮기고 결과를 스낵바로 알린다.
Future<void> copyChatText(
  BuildContext context,
  String text, {
  required String copiedNotice,
  required String failedNotice,
}) async {
  final messenger = ScaffoldMessenger.of(context);
  try {
    await Clipboard.setData(ClipboardData(text: text));
    messenger.showSnackBar(SnackBar(content: Text(copiedNotice)));
  } on PlatformException {
    messenger.showSnackBar(SnackBar(content: Text(failedNotice)));
  }
}
