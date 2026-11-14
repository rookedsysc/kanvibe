enum PaneLayoutKind {
  single('single', 1, '단일 pane'),
  horizontal2('horizontal_2', 2, '위아래 2개'),
  vertical2('vertical_2', 2, '좌우 2개'),
  leftRightTb('left_right_tb', 3, '왼쪽 1개 · 오른쪽 2개'),
  leftTbRight('left_tb_right', 3, '왼쪽 2개 · 오른쪽 1개'),
  quad('quad', 4, '4개 분할');

  const PaneLayoutKind(this.wireValue, this.paneCount, this.label);
  final String wireValue;
  final int paneCount;
  final String label;
}

class PaneLayout {
  const PaneLayout({
    required this.id,
    required this.kind,
    required this.commands,
    required this.isGlobal,
    this.projectId,
  });
  final String id;
  final PaneLayoutKind kind;
  final List<String> commands;
  final bool isGlobal;
  final String? projectId;
}
