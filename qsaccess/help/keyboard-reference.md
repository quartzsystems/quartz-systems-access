# Keyboard reference

This is the default keyboard map of a 5250 display session: each 5250 function, and the keys that perform it. A key is named as the Keyboard Map dialog names it, with its modifiers first, for example `Shift+F1`. Where a function has several keys, any of them performs it.

{{default-keymap}}

## Function keys

`F1` to `F12` send the system's F1 to F12, and `Shift+F1` to `Shift+F12` send F13 to F24. They are the system's keys, so `F1` in a display session asks the system for its help on the screen shown. Help for the session window itself is on its **Help** menu.

## Changing the map

Each session has its own keyboard map, kept in its session file as the changes from this default map. Select **Actions > Keyboard Map...** in the session window to see the map the session uses and to change the key of a function. **Actions > Export Keyboard Map...** writes the whole map to a `.qkm` file, and **Actions > Load Keyboard Map...** replaces the session's map with one. See [5250 Display](display-5250.md).

## See also

- [5250 Display](display-5250.md)
- [Session Manager and session files](session-manager.md)
