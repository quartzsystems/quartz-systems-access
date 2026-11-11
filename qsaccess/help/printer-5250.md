# 5250 Printer

A 5250 printer session makes this computer a printer device of a QS/400 system. Output the system sends to that device arrives here and is saved as PDF files, printed on a printer of this computer, or both.

This is not the same as [Printer Output](printer-output.md), which lists the spooled files that are on the system.

## Starting a session

Select **5250 Printer** in the main window's task pane. If no system is configured yet, System Configurations opens first. Fill in the **Start 5250 Printer** dialog and select **Start**.

| Field | Meaning |
|---|---|
| **System** | The system configuration to connect to. |
| **Device name** | Required: the printer device on the system. 1 to 10 letters, digits, `#`, `$`, `@` or `_`, starting with a letter, `#`, `$` or `@`. |
| **Message queue** | The device's message queue; `QSYSOPR` unless changed. |
| **Library** | The message queue's library; `QSYS` unless changed. |
| **CCSID** | The code page the print data is read in. |
| **Output** | **PDF files**, **System printer**, or **PDF files and system printer**. |
| **Use TLS** | Connects to the system's Telnet TLS port. It starts as the system's configuration has it. |

Without a device name the session does not start ([QAD3002](messages-reference.md#QAD3002)).

The session window opens and connects. To keep the session, choose **File > Save Session**; it is saved as a `.qsp` file. See [Session Manager and session files](session-manager.md).

## The printer session window

Under the toolbar a line shows **System**, **Device** and **State**, and a lock when the connection uses TLS. The states are:

| State | Meaning |
|---|---|
| Disconnected | Not connected. |
| Connecting | The connection is being made. |
| Ready | Connected and waiting for output. |
| Receiving | A job is arriving. |
| Printing | Output is being made. |
| Error | Output failed; the system was told the printer is not ready. |
| Reconnecting | Waiting to connect again; the seconds left are shown. |

The status bar at the bottom shows the device with its state, and the last message.

### Menus

- **File > Save Session** (`Ctrl+S`) and **Save Session As...** write the session file.
- **File > Settings...** opens [Printer Session Settings](#printer-session-settings).
- **File > Open Output Folder** opens the folder the PDF files go to.
- **File > Print Test Page**; see [Test page](#test-page).
- **File > Close** (`Alt+F4`) disconnects and closes the window.
- **Edit > Open Output**, **Show in Folder**, **Print Again** and **Delete** (`Delete`) act on the job selected in the list.
- **Communication > Connect** and **Disconnect**.
- **Help > Help Topics** (`F1`) opens this topic, and **Help > About QS/400 Access for Desktop...** the About box.

The toolbar has the commands used most.

## Where output goes

Each job is written to this computer's disk as it arrives, before the system is told it arrived. When the job ends, its output is made:

- **PDF files.** A PDF in the session's PDF folder. Unless you set another, the folder is `Quartz Access/Printer Output/<device>` in your Documents folder, and the file name is `{device}-{date}-{time}-{seq}.pdf`. [QAD3010](messages-reference.md#QAD3010) names the file.
- **System printer.** The job is printed on the printer chosen in the settings, or on this computer's default printer ([QAD3011](messages-reference.md#QAD3011)). When this computer cannot print, a job meant only for the printer is saved as a PDF instead ([QAD3012](messages-reference.md#QAD3012)).

If the connection ends in the middle of a job, what arrived is still made into output and the job is marked incomplete ([QAD3023](messages-reference.md#QAD3023)). A job left unfinished because the program stopped is made into output when the session is next opened.

## The Received output list

**Received output** has one row per job, the newest last.

| Column | Shows |
|---|---|
| **Time** | When the job ended. |
| **Pages** | The number of pages. |
| **Size** | The amount of print data received. |
| **Target** | PDF, Printer, or PDF and printer. |
| **Status** | Saved, Printed, Saved and printed, Incomplete, Discarded (transformed), or an error. |
| **Output file** | The PDF's name, or the printer. |

Double-click a row, or choose **Edit > Open Output**, to open the job's PDF. A right-click offers **Open**, **Show in Folder**, **Print Again** and **Delete**.

- **Print Again** prints the job on the system printer.
- **Delete** removes the row and, after asking, the job's files.

## Printer Session Settings

**File > Settings...** changes the session. Choose **OK**, then **File > Save Session** to keep the changes in the session file.

| Field | Meaning |
|---|---|
| **Device name**, **Message queue**, **Library** | As in the Start dialog. Changes take effect when the session connects again. |
| **Title** | Shown in the window title and in Session Manager. |
| **Output** | PDF files, the system printer, or both. |
| **PDF folder** | Where PDF files go; **Browse...** chooses a folder. Empty uses the default folder. |
| **File name** | The pattern for file names. `{device}`, `{date}`, `{time}` and `{seq}` are filled in. |
| **Paper** | Letter, A4 or Legal, used when the print data does not say. |
| **Margins (inches)** | From 0 to 2. Empty is a quarter inch. |
| **System printer** | **Default printer** or one of this computer's printers. On Windows, **Print Setup...** is beside it. |
| **Open each PDF when it is saved** | Opens every PDF in this computer's PDF viewer. |
| **Also save a text file** | Writes the job's text beside the PDF. |
| **Connect when the session is opened** | Connects as soon as the session file is opened. |

## Test page

**File > Print Test Page** makes a test page on this computer and sends it to the session's output, exactly as a received job would go. The system takes no part in it, so it works without a connection. The page is added to the list.

## Reconnecting

A printer session is meant to run unattended. When the connection is lost or cannot be made, the session connects again by itself: after 5 seconds, then after twice as long each time, up to 5 minutes ([QAD3030](messages-reference.md#QAD3030)). A device name that is in use is tried again the same way. **Communication > Disconnect** stops the retries, and **Communication > Connect** connects at once.

The session does not retry after these; correct the cause and connect again:

- The device name is not valid ([QAD1021](messages-reference.md#QAD1021)) or is not a printer device ([QAD3003](messages-reference.md#QAD3003)).
- The message queue is not found ([QAD3004](messages-reference.md#QAD3004)).
- The TLS handshake fails ([QAD1010](messages-reference.md#QAD1010)).
- The certificate is not trusted ([QAD1011](messages-reference.md#QAD1011)). **Certificate Not Trusted** opens, as for a display session; see [5250 Display](display-5250.md#untrusted-certificates).

## Messages

The status bar shows a session's messages. Besides those named above:

| Message | Meaning |
|---|---|
| [QAD3001](messages-reference.md#QAD3001) | The session is connected. |
| [QAD3013](messages-reference.md#QAD3013) | The printer reported an error. |
| [QAD3014](messages-reference.md#QAD3014) | The output folder cannot be written. |
| [QAD3020](messages-reference.md#QAD3020) | Transformed print data was received and discarded. |
| [QAD3021](messages-reference.md#QAD3021) | A job was closed after no more data came. |
| [QAD3022](messages-reference.md#QAD3022) | A print record was rejected. |

## See also

- [Session Manager and session files](session-manager.md)
- [5250 Display](display-5250.md)
- [Printer Output and the viewer](printer-output.md)
- [Certificate Management](certificate-management.md)
- [Messages reference](messages-reference.md)
