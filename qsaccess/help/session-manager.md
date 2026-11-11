# Session Manager and session files

A 5250 session is kept as a session file. Session Manager lists your session files and starts, changes and tidies them.

## Session files

The program uses four kinds of file. Each is a text file.

| File | Holds |
|---|---|
| `.qsd` | A display session: the system, title, screen size, CCSID, device name, colour map, whether it connects when opened and signs on automatically, the keypad and window position, the keyboard map, the hotspot settings and the list of the session's macros. |
| `.qsp` | A printer session: the system, device name, message queue and library, title, CCSID, where and how output goes, whether it connects when opened, and the window position. |
| `.qkm` | A whole keyboard map, for passing between sessions and computers. |
| `.qsm` | A macro: its name, description, screen size and steps. |

A session file names its system configuration. It also keeps the system's host, Telnet port and TLS setting, so that the configuration can be created on a computer that does not have it.

Passwords are never in a session file, and neither is certificate trust, which is kept only in the system configuration. A recorded macro holds no password either: where one was typed, the macro asks for it when it runs.

## The Sessions folder

Session files are kept in `QS400 Access for Desktop/Sessions` in your Documents folder unless you choose another place when saving. The program creates the folder when it is first needed. File choosers for session files, keyboard maps and macros open there.

Everything the program saves in Documents is under `QS400 Access for Desktop`: `Sessions`, `Printer Output` for 5250 printer sessions and `Screens` for Print Screen. Earlier releases used the folders `Quartz Access` and `QS400 Access`. The first time this release starts it moves what they hold into the one folder and reports [QAD2012](messages-reference.md#QAD2012); Recent Sessions and the folders added to Session Manager follow. A file whose name is already taken in the new folder is left where it was, unchanged.

## The Session Manager window

Select **Session Manager** in the main window's task pane, or **Tools > Session Manager...**. The window lists the session files in the Sessions folder and in any folders you add. Files in folders below those are not listed.

The list follows the folders: a file added, changed or removed by anything else shows within a few seconds. The line at the bottom gives the number of sessions and folders.

### The list

| Column | Shows |
|---|---|
| **Title** | The session's title, or its file name when it has none. |
| **Type** | Display or Printer. |
| **System** | The system configuration it uses, or `(cannot be read)` for a damaged file. |
| **Screen size** | A display session's screen size. |
| **Device name** | The device name, when one is set. |
| **Last used** | When the session was last opened. |
| **Location** | The folder the file is in. |

Select a column heading to sort by it; select it again to reverse the order. You can select several rows.

**Show** filters the list: **All sessions**, **Display sessions** or **Printer sessions**.

### The buttons

| Button | Does |
|---|---|
| **New...** | Asks for the kind, **Display...** or **Printer...**, then for the session's settings and a file name. The session is saved, not started. |
| **Start** | Opens every selected session, each in a window of its own. A double-click starts one. |
| **Edit...** | Changes the selected session's file: Session Properties for a display session, Printer Session Settings for a printer session. |
| **Copy** | Makes a copy of each selected file beside it, named with `(copy)`. |
| **Rename...** | Gives the session a new title and renames its file to match. |
| **Delete** | Deletes the selected files, after asking. This cannot be undone. |
| **Add Folder...** | Adds a folder to the list ([QAD2030](messages-reference.md#QAD2030)). |
| **Remove Folder** | Takes the selected session's folder off the list. Its files are not touched. The Sessions folder is always listed. |
| **Refresh** | Reads the folders again. |
| **Close** | Closes Session Manager. Open sessions stay open. |

**New...** needs at least one system configuration. A new display session starts with the keyboard map set for new sessions; see [5250 Display](display-5250.md#the-keyboard).

## Opening a file

A file can be opened in several ways, and each does the same:

- **Start** in Session Manager.
- **File > Open...** (`Ctrl+O`) in the main window, or **File > Open Session...** in a display session window.
- **File > Recent Sessions**.
- A double-click on the file in this computer's file manager, when the installer registered the file types; see [Installing](installing.md). If the program is already running, the file opens in it.

What happens depends on the kind of file:

| File | Opening it |
|---|---|
| `.qsd` | Opens a display session window, and connects if the session says to. |
| `.qsp` | Opens a printer session window, and connects if the session says to. |
| `.qkm` | **Load Keyboard Map** asks where the map goes: an open session, or **New sessions**. |
| `.qsm` | **Run Macro** asks which open session to run it in; **Edit...** opens it in the Macro Editor. |

Every open makes a new window, so the same session file can be open more than once. A file with another extension is recognised by its contents.

A file that cannot be read gives [QAD2001](messages-reference.md#QAD2001), and one made by a newer release of the program gives [QAD2002](messages-reference.md#QAD2002).

## Saving a session

In a session window, **File > Save Session** writes the session to its file, and asks for a name the first time. **File > Save Session As...** writes it to another file. The status bar confirms with [QAD2004](messages-reference.md#QAD2004).

A display session started from **Start 5250 Display**, or a printer session from **Start 5250 Printer**, is not in a file until you save it.

## Recent Sessions

**File > Recent Sessions**, in the main window and in every display session window, lists the ten session files opened or saved last, the newest first. Select one to open it. A file that is no longer there is taken off the list when you try to open it. A session renamed or deleted in Session Manager is followed or removed.

## A session whose system is not configured

When a session file names a system that has no configuration on this computer, the program says so ([QAD2003](messages-reference.md#QAD2003)) and offers three choices:

- **Create system from session file** opens a new system configuration, filled in with the name, host, Telnet port and TLS setting from the file. Check it and save it; the session then opens.
- **Details...** shows the message's cause and recovery.
- **Cancel** leaves the session unopened.

A configuration made this way has no default user ID, and it trusts no certificates beyond those this computer already trusts. If the system uses a certificate of its own, **Certificate Not Trusted** opens at the first TLS connection; see [Certificate Management](certificate-management.md).

## See also

- [5250 Display](display-5250.md)
- [5250 Printer](printer-5250.md)
- [Keyboard reference](keyboard-reference.md)
- [Installing, repairing and uninstalling](installing.md)
- [Certificate Management](certificate-management.md)
- [Messages reference](messages-reference.md)
