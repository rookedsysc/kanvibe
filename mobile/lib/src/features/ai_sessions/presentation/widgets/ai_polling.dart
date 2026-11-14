import 'dart:async';

import 'package:flutter/widgets.dart';

class AiPolling extends StatefulWidget {
  const AiPolling({
    required this.interval,
    required this.onRefresh,
    required this.child,
    super.key,
  });
  final Duration interval;
  final VoidCallback onRefresh;
  final Widget child;
  @override
  State<AiPolling> createState() => _AiPollingState();
}

class _AiPollingState extends State<AiPolling> with WidgetsBindingObserver {
  Timer? _timer;
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _resume();
  }

  void _resume() {
    _timer?.cancel();
    final lifecycle = WidgetsBinding.instance.lifecycleState;
    if (lifecycle == null || lifecycle == AppLifecycleState.resumed) {
      _timer = Timer.periodic(widget.interval, (_) => widget.onRefresh());
    }
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    _timer?.cancel();
    if (state == AppLifecycleState.resumed) {
      widget.onRefresh();
      _resume();
    }
  }

  @override
  void dispose() {
    _timer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
