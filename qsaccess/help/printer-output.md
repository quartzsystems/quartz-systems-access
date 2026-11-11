# Printer Output and the viewer

**Printer Output** lists the spooled files on a system. You can look at them, save or print them on this computer, and hold, release, delete or move them on the system. Choose **Printer Output** in the **General** group of the main window's task pane. If the program has no session on the selected system, it asks you to sign on first (see [Signing on](signing-on.md)).

Printer Output needs QAP version 3. On a system with an older version the window does not open, and the program shows [QAD5001](messages-reference.md#QAD5001).

This is not the same as a 5250 printer session, which keeps output that this computer received (see [5250 printer sessions](printer-5250.md)).

There is one Printer Output window for each system. Choosing **Printer Output** again brings it to the front.

## Choosing what is listed

The row under the toolbar says which files are listed. Change it and select **Go**.

| Field | What to enter |
| --- | --- |
| **User** | `*CURRENT` for your own files, `*ALL` for every user's, or a user profile |
| **Output queue** | `*ALL`, or a queue picked from the list or typed as `LIBRARY/QUEUE` |
| **Status** | `*ALL` or one status |
| **Form type** | A form type; blank or `*ALL` for every one |
| **User data** | The user data; blank or `*ALL` for any |

At first the window lists your own files on every queue.

## The list

The columns are **File**, **User**, **User data**, **Status**, **Pages**, **Copies**, **Form type**, **Output queue**, **Created** and **Job**. **Created** is shown in this computer's time zone.

**Status** is one of Ready, Held, Open, Closed, Writing, Printing, Message, Pending or Saved. Ready and Printing are green; Message is red.

The system sends the list a page at a time. **Previous** and **Next** move between pages, and the line beside them says which files and which page are shown. The status bar shows the total in [QAD6001](messages-reference.md#QAD6001).

Select a column heading to sort by that column; select it again to reverse the order. Sorting orders only the page shown, and the status bar says so.

## Actions

The actions are in the **File** and **Actions** menus, on the toolbar, and in the menu that a right-click on a file opens.

These act on one file, the first one selected:

- **Open** (or a double-click) shows the file. See [The viewer](#the-viewer).
- **Save as PDF...** and **Save as Text...** save the file on this computer.
- **Print...** prints the file on one of this computer's printers, which you pick in the **Print** dialog.
- **Properties** shows every detail of the file, including its device type and size.

These act on every selected file:

- **Hold...** asks whether a file that is being printed stops **Immediately** or **At End of Page**.
- **Release** releases held files.
- **Delete...** lists every file it is about to delete and asks you to confirm. A deleted file cannot be brought back.
- **Move...** asks for the output queue to move the files to, picked or typed as `LIBRARY/QUEUE`. Tick **To the top of the queue** to put them first.

For one file, the status bar says what was done. If the system refuses, its message opens. For several files, a summary lists each file with its result; when any failed its heading is [QAD6006](messages-reference.md#QAD6006). Double-click a refusal in the summary for the system's second-level text.

While a file is read for saving or printing, the status bar shows the progress ([QAD6010](messages-reference.md#QAD6010)) and **Cancel Reading** stops it. Reading a file that is still being written fails with [QAD6012](messages-reference.md#QAD6012).

On Windows, a PDF that the system itself makes for a file that is not SCS cannot be printed from here; save it and print it from a PDF viewer.

## Output Queues

**View > Output Queues...** opens the **Output Queues** window, which lists the system's output queues with the columns **Queue**, **Library**, **Text**, **Files**, **Writer** and **Held**.

- In **Library**, enter `*ALL`, `*LIBL` or a library name, and select **Go**.
- Double-click a queue to list its spooled files in Printer Output, for every user.

## Keeping the list up to date

**View > Refresh** or `F5` asks for the page again and keeps your selection.

**Auto-refresh**, at the bottom right, refreshes the page shown every 15, 30 or 60 seconds, or not at all when it is **Off**. It runs only while the window is shown and not minimised, and keeps the selection and the scroll position. The status bar shows [QAD9021](messages-reference.md#QAD9021).

On a system with QAP version 6 or later, the list updates as changes happen and the status bar shows [QAD9020](messages-reference.md#QAD9020). **Auto-refresh** is then unavailable, because it is not needed. This works when the list is for one output queue, or for one user on every queue. The files of every user on every queue cannot be followed this way, so that list still uses **Auto-refresh**, as does any list when live updating fails.

## The viewer

**Open** shows an SCS spooled file in the program's own viewer. Pages appear as they arrive, so you can read the first page before the whole file is in. **Cancel Reading** stops the read and keeps the pages already read.

A file of another device type opens in this computer's usual program for the format the system sends.

In the viewer:

- **First**, **Previous**, **Next** and **Last** move between pages, as do `Page Up`, `Page Down`, `Ctrl+Home` and `Ctrl+End`. To go to a page, type its number in the page field and press `Enter`.
- **Zoom** is **Fit Width** or **100 %**.
- **Find**: type some text and press `Enter`. Every match on the page is highlighted, upper or lower case alike. **Find Next** or `F3` goes to the next match and carries on through the pages.
- **Copy** or `Ctrl+C` copies the text of the page shown.
- **Save as PDF...**, **Save as Text...** and **Print...** work from the pages already read, without asking the system again. They become available when the whole file has been read.

## See also

- [5250 Printer](printer-5250.md)
- [Run Command and prompting](run-command.md)
- [Signing on and passwords](signing-on.md)
- [Keyboard reference](keyboard-reference.md)
- [Messages reference](messages-reference.md)
