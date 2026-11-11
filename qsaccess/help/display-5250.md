# 5250 Display

A 5250 display session is a terminal on a QS/400 system, over Telnet or Telnet with TLS. Each session has a window of its own.

## Starting a session

Select **5250 Display** in the main window's task pane. **Start 5250 Display** opens with the selected system and the values last used for it.

| Field | Meaning |
|---|---|
| **System** | The system configuration to connect to. |
| **Screen size** | `24 x 80` or `27 x 132`. |
| **CCSID** | The code page: 37, 273, 285, 297, 500 or 1140. |
| **Device name** | Optional; empty lets the system choose. 1 to 10 letters, digits, `#`, `$`, `@` or `_`, starting with a letter, `#`, `$` or `@`. |
| **Use TLS** | Connects to the system's Telnet TLS port. It starts as the system's configuration has it. |
| **Colour map** | **QS/400 console (green on black)** or **Black on white**. |

Select **Start**. The session window opens and connects. To keep the session, choose **File > Save Session**.

## The session window

The window's title is the session's title followed by `- 5250 Display`.

The display is drawn with the QS/400 service console's 9 by 16 bitmap font and palette. When you resize the window, the largest whole-number scale that fits is used and the grid is centred; it is never stretched.

The line under the grid shows, from the left: the keyboard state (`X SYSTEM` while the system is working, `X II` after an error), `MW` when a message waits, `INS` in insert mode, `TLS` on a TLS session, the system with the device name and the connection state, and the cursor's row and column. The status bar shows the connection state and the last message.

### Menus

- **File > Open Session...** (`Ctrl+O`), **Save Session** (`Ctrl+S`), **Save Session As...** and **Recent Sessions**; see [Session Manager and session files](session-manager.md). Saving keeps the session's settings, keyboard map, keypad and window position.
- **File > Properties...**; see [Session Properties](#session-properties).
- **File > Print Screen** (`Ctrl+P`); see [Print Screen](#print-screen).
- **File > Close** (`Alt+F4`) disconnects and closes the window.
- **Edit > Copy** (`Ctrl+C`) and **Edit > Paste** (`Ctrl+V`); see [Copy and paste](#copy-and-paste).
- **View > Keypad** shows or hides the keypad.
- **View > Colour Map** switches between **QS/400 console** and **Black on white**.
- **Communication > Connect**, **Disconnect** and **Reconnect**.
- **Actions > Enter**, **Reset**, **Attention**, **System Request**, **Help** and **Clear** send that 5250 key.
- **Actions > Keyboard Map...**, **Load Keyboard Map...** and **Export Keyboard Map...**; see [The keyboard](#the-keyboard).
- **Macro**; see [Macros](#macros).
- **Help > Help Topics** opens this topic, and **Help > About QS/400 Access for Desktop...** the About box.

## The keyboard

Keys go to the system through the session's keyboard map. The default map is in the [Keyboard reference](keyboard-reference.md): `F1` to `F12` are the system's F1 to F12, with `Shift` they are F13 to F24, and `Right Ctrl` on its own is Field Exit.

Inside a display session `F1` is the system's own F1 key and is sent to the system. Help for the window is **Help > Help Topics**. The 5250 Help key has no key by default; use **Actions > Help** or the keypad.

**Actions > Keyboard Map...** lists every 5250 function with its key, or `(none)`.

1. Select a row and choose **Change...** (or double-click it).
2. Press the key, then **OK**. **No key** takes the key away from the function.
3. Choose **OK** to use the map.

A key given to a function is taken from the function it had. **Reset to defaults** restores the default map. Saving the session keeps its map.

**Actions > Export Keyboard Map...** writes the whole map to a `.qkm` file, and **Actions > Load Keyboard Map...** replaces the session's map with one ([QAD2010](messages-reference.md#QAD2010)). A `.qkm` file opened on its own can also become the map new sessions start with; see [Session Manager and session files](session-manager.md#opening-a-file).

## The keypad

**View > Keypad** adds a panel of buttons at the right of the window: F1 to F24, Enter, Reset, Attn, SysRq, PgUp, PgDn, FldExit, Field+, Field-, Help, Print and Clear.

## Copy and paste

Drag over the display to select a rectangle of cells. **Edit > Copy** copies the selection, or the whole screen when nothing is selected. **Edit > Paste** types the clipboard's text into the input fields from the cursor.

## Print Screen

**File > Print Screen** writes the screen as a one-page PDF, in the session's colours, to `QS400 Access/Screens` in your Documents folder. The file is named with the system, the date and the time. The status bar names the file ([QAD1040](messages-reference.md#QAD1040)).

## Hotspots

Some text on the screen acts when clicked. The pointer becomes a hand over it.

- A function key label such as `F3=Exit` sends that function key.
- `More...` sends Page Down and `Bottom` sends Page Up.

Only protected text is looked at, never the contents of an input field. A click elsewhere moves the cursor. Switch hotspots on or off on the **Hotspots** tab of Session Properties.

## Macros

A macro is a list of steps kept in a `.qsm` file.

**Recording.** Select **Macro > Record...**, work as usual, then select **Macro > Stop Recording**, name the macro and save it. It is added to **Macro > Run** ([QAD2021](messages-reference.md#QAD2021)). Text typed into a hidden field, such as a password, is never recorded: a step that asks for the value takes its place.

**Running.** Choose the macro from **Macro > Run**, or **Macro > Play...** for a file. **Macro > Stop** stops it. A macro stops by itself when a wait times out, the session is not connected, or the screen size is not the macro's; [QAD2020](messages-reference.md#QAD2020) names the step and the reason.

**Editing.** **Macro > Edit...** opens a file in the **Macro Editor**: the macro's **Name**, **Description** and **Screen size**, and its steps, with **Add...**, **Edit...**, **Remove**, **Move Up** and **Move Down**. Choose **Save** or **Save As...** before **Close**.

| Step | Does |
|---|---|
| **Type text** | Types the text at the cursor. |
| **Press a key** | Sends a 5250 key. |
| **Wait for the keyboard** | Waits until the keyboard unlocks, up to the time given. |
| **Wait for text on the screen** | Waits until the text is at the row and column. **Pick from Screen...** takes them from a click on an open session. |
| **Pause** | Waits the time given. |
| **Ask for a value** | Asks when the macro runs and types the answer. **Hide what is typed** is for passwords. |

Times are in milliseconds.

## Automatic sign-on

A session can sign on without a password: tick **Sign on automatically (QAP ticket, no password)** in Session Properties. At each connect the session gets a one-time ticket from the system over QAP, asking you to sign on first if this program has no session there (see [Signing on](signing-on.md)), and sends the ticket in place of a password.

It needs:

- Telnet over TLS (**Use TLS**); otherwise [QAD5030](messages-reference.md#QAD5030).
- QAP version 2 on the system; otherwise [QAD5001](messages-reference.md#QAD5001).
- A system that allows it. If the system shows its sign-on display instead, the status bar says [QAD4040](messages-reference.md#QAD4040).

When one of the first two is not met, the check box is disabled and the reason is under it.

## Session Properties

**File > Properties...** has two tabs.

- **General**: the **Title**, shown in the window title and in Session Manager, the choices of the Start dialog, automatic sign-on, and **Connect when the session opens**. Changes to the system, screen size, CCSID, device name and TLS take effect at the next connection.
- **Hotspots**: **Act on text clicked on the screen**, and the kinds of text.

## When a session ends

- **Communication > Disconnect** ends the connection ([QAD1006](messages-reference.md#QAD1006)). The last screen stays, greyed. **Communication > Reconnect** connects again with a fresh screen.
- When the system ends the session, the program shows [QAD1005](messages-reference.md#QAD1005). It does not reconnect by itself.
- A connection that cannot be made shows why: [QAD1002](messages-reference.md#QAD1002), [QAD1003](messages-reference.md#QAD1003), [QAD1004](messages-reference.md#QAD1004) or [QAD1010](messages-reference.md#QAD1010).
- A device name that is in use or not valid ([QAD1020](messages-reference.md#QAD1020), [QAD1021](messages-reference.md#QAD1021)) offers **Retry without device name**.

## Untrusted certificates

When a TLS session meets a certificate this computer does not trust, **Certificate Not Trusted** shows its subject, issuer, validity and SHA-256 fingerprint ([QAD1011](messages-reference.md#QAD1011)). **Trust for this system** adds the certificate to that system's list and connects again. See [Certificate Management](certificate-management.md).

## See also

- [Session Manager and session files](session-manager.md)
- [5250 Printer](printer-5250.md)
- [Keyboard reference](keyboard-reference.md)
- [Signing on and passwords](signing-on.md)
- [Certificate Management](certificate-management.md)
- [Messages reference](messages-reference.md)
