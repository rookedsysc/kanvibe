# Android compact accessory and combo draft

Current product tree: compact row and combo editor. compact-above-keyboard.png shows the accessory adjacent to the native IME. combo-draft.png shows Ctrl+c → ↑ before the third step is added. android-combo-draft.mp4 is198.36seconds.

Device execution verification is partial. System UI ANR and later nonresponsive UI hierarchy/scroll calls prevented completing Save→Apply→execute on the emulator. Do not treat this video as a passed full combo journey. Flutter widget tests verify exact output bytes, repeated execution, settings persistence and cancellation.
