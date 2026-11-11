# Integrated File System, transfers and the editor

**Integrated File System** browses a system's files and folders, moves files to and from this computer, and edits text files. Choose **Integrated File System** in the **General** group of the main window's task pane. If the program has no session on the selected system, it asks you to sign on first (see [Signing on](signing-on.md)).

It needs QAP version 5. For a system with an older version the task is dimmed, and the reason is [QAD5001](messages-reference.md#QAD5001).

## The window

- **The folder tree**, on the left, starts with your home folder and `/`. Select a folder to list it.
- **The entries list**, on the right, has the columns **Name**, **Type**, **Size**, **Modified**, **Owner** and **CCSID**. Select a column heading to sort the entries loaded so far.
- **Path** with **Go** opens the folder you type. **Back**, **Forward**, **Go > Up** and **Go > Home Folder** move between folders, and **View > Refresh** (`F5`) reads the folder again.
- **Show** takes a name pattern: `*` stands for any characters and `?` for one. Press `Enter` to apply it.

The list is read 500 entries at a time, and the next page loads as you scroll down. The status bar shows [QAD8001](messages-reference.md#QAD8001) with the number of entries. A folder you may not read shows the system's message in place of the list.

A link shows its name and its target, with the type **Link to file** or **Link to folder**, and opens as its target. A link whose target is missing is a **Broken link**, shown in red, and cannot be opened.

## Working with entries

The actions are in the **File** and **Edit** menus and in the menu a right-click opens.

- **New Folder...** creates a folder in the folder shown.
- **Rename...** (`F2`) renames the selected entry.
- **Copy** and **Cut**, then **Paste** in another folder, copy or move entries on the system. The data does not pass through this computer. When an entry of the same name is there, [QAD8010](messages-reference.md#QAD8010) offers **Replace**, **Skip** and **Cancel**.
- **Delete** asks first, in **Confirm Delete**. For a folder it says how many files and folders are in it, because they are deleted too. A delete the system refuses part way stops there with [QAD8006](messages-reference.md#QAD8006) and the system's message; entries already deleted stay deleted.
- **Properties** shows the entry's folder, type, a link's target, size, last change, owner and CCSID. Type a new name in **Name** and select **OK** to rename it. Authorities are not shown or changed here.

## Download and upload

**File > Download...** copies the selected files and folders to a folder you choose on this computer. **File > Upload...** copies files you choose into the folder shown, as does dragging files and folders from the desktop's file manager onto the list. Each first opens **Transfer Options**:

| Option | Meaning |
| --- | --- |
| **Binary** | The default. The bytes are copied unchanged. |
| **Text** | The system converts between the file's CCSID and UTF-8. This computer changes only line endings. |
| **CCSID for new files** | The CCSID of a file an upload creates on the system. 1208 is UTF-8. |
| **Change line endings** | Text only: LF on the system, CR LF on this computer. It is on by default on Windows. |
| **If a file already exists** | **Ask**, **Skip**, **Replace** or **Replace if newer**. |

With **Ask**, [QAD8010](messages-reference.md#QAD8010) offers **Replace**, **Skip**, **Replace All**, **Skip All** and, for a download, **Keep Both**, which adds a number to the new file's name. **Replace All** and **Skip All** hold for the rest of that batch.

**Save Request...** saves the transfer and its options as a `.qdx` request, and **Open Request...** runs a saved files request instead. See [Transfer requests](data-transfer.md#transfer-requests).

## The transfer queue

Transfers go to the **Transfers** panel under the list; **View > Transfers** shows or hides it. The queue runs one file at a time on a connection of the window's own. A folder becomes a line for each of its files. The columns are **Direction**, **Name**, **Size**, **Progress**, **Speed** and **Status**.

| Status | Meaning |
| --- | --- |
| Queued | Waiting for its turn. |
| Running | Being copied now. |
| Done | Complete: [QAD8020](messages-reference.md#QAD8020). |
| Skipped | The file was already there and was left alone. |
| Cancelled | You cancelled it: [QAD8022](messages-reference.md#QAD8022). |
| Failed | It stopped, with the reason: [QAD8021](messages-reference.md#QAD8021). |

- **Cancel** stops the selected transfers.
- **Retry** queues the selected failed or cancelled transfers again. A binary transfer, and a text download, go on from the last part the system confirmed, and [QAD8023](messages-reference.md#QAD8023) says from where. A text upload starts again from the beginning.
- **Remove Finished** takes finished lines off the list. **Clear** cancels the running transfer and takes every other line off the list, queued ones included.

A download is written beside its target as `<name>.qsapart` and renamed when it is complete. After a cancel or a failure the target is unchanged and the `.qsapart` file is kept for **Retry**.

If the session has no connection left for the window, transfers run on the shared connection and [QAD8030](messages-reference.md#QAD8030) says so once.

## Open and Edit

**Open**, or a double-click, shows a folder. For a file it makes a read-only copy on this computer and hands it to the default application. Changes to that copy are not saved to the system.

**Edit** (`Ctrl+E`) opens a text file in the built-in editor. The system converts the text from the file's CCSID, and the status bar shows the line, the column and the CCSID.

- A binary file (CCSID 65535, or NUL bytes near its start) opens read-only as a hex dump, and a file larger than 8 MiB opens read-only, both with [QAD8013](messages-reference.md#QAD8013).
- A file larger than 64 MiB is not opened: [QAD8012](messages-reference.md#QAD8012) offers **Download...** instead.

**File > Save** (`Ctrl+S`) writes the text back, and the file keeps its CCSID. **File > Save As...** asks for a path on the system and a CCSID for the new file. **File > Reload** reads the system's file again. The **Edit** menu has **Undo**, **Find...**, **Find Next** (`F3`) and **Replace...**. A `*` in the title marks unsaved changes, and closing the editor asks whether to save them.

Before a save, the editor checks whether the file changed on the system since you opened or last saved it. If it did, [QAD8011](messages-reference.md#QAD8011) offers:

- **Overwrite**: save your text over the system's file.
- **Reload**: read the system's file again and lose your changes.
- **Cancel**: go on editing.

## See also

- [Data Transfer and transfer requests](data-transfer.md)
- [Running a request with qsaccess run](qsaccess-run.md)
- [Signing on and passwords](signing-on.md)
- [Keyboard reference](keyboard-reference.md)
- [Messages reference](messages-reference.md)
