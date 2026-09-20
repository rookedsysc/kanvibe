/// 태스크의 우선순위.
///
/// 값은 데스크탑의 `TaskPriority`를 그대로 따른다. 고르지 않은 태스크가 더 많아서 없을 수 있다.
enum TaskPriority {
  low('low', '낮음'),
  medium('medium', '보통'),
  high('high', '높음');

  const TaskPriority(this.wireValue, this.label);

  final String wireValue;
  final String label;

  /// 데스크탑이 보낸 값에 대응하는 우선순위. 모르는 값은 고르지 않은 것으로 본다
  static TaskPriority? fromWire(String? value) {
    for (final priority in TaskPriority.values) {
      if (priority.wireValue == value) {
        return priority;
      }
    }
    return null;
  }
}
