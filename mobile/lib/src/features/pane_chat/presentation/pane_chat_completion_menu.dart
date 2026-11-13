import 'package:flutter/material.dart';

import '../../../common/constants/app_sizes.dart';
import '../domain/pane_chat.dart';

class PaneChatCompletionMenu extends StatelessWidget {
  const PaneChatCompletionMenu({
    required this.items,
    required this.selectedIndex,
    required this.onSelect,
    super.key,
  });
  final List<PaneChatSuggestion> items;
  final int selectedIndex;
  final ValueChanged<PaneChatSuggestion> onSelect;
  @override
  Widget build(BuildContext context) => Semantics(
    identifier: 'pane-chat-completions',
    child: ConstrainedBox(
      constraints: const BoxConstraints(maxHeight: 180),
      child: Material(
        borderRadius: BorderRadius.circular(AppSizes.radiusLg),
        child: ListView.builder(
          shrinkWrap: true,
          itemCount: items.length,
          itemBuilder: (context, index) => ListTile(
            dense: true,
            selected: index == selectedIndex,
            title: Text(
              items[index].label,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
            subtitle: items[index].description.isEmpty
                ? null
                : Text(
                    items[index].description,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
            onTap: () => onSelect(items[index]),
          ),
        ),
      ),
    ),
  );
}
