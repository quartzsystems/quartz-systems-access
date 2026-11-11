# Getting started

QS/400 Access for Desktop is the desktop client for QS/400 systems, on Windows and Linux. It provides 5250 display and printer sessions, and a set of tools that work on a system once you have signed on to it: Run Command, Printer Output, Run SQL, Schemas, Data Transfer, the Integrated File System and Messages.

Before you can use a system, add a system configuration for it. See [System Configurations](getting-started.md#system-configurations) below.

## The main window

The main window has four parts, from top to bottom: the menu bar, the toolbar, the task pane with a welcome panel beside it, and the status bar.

### Menu bar

| Menu | What it holds |
|---|---|
| **File** | **Open...** (`Ctrl+O`) opens a saved file such as a session file. **Recent Sessions** lists the sessions opened lately. **Exit** (`Alt+F4`) closes the program. |
| **Edit** | **Cut**, **Copy** and **Paste**. They are dimmed: the main window has nothing to cut, copy or paste. |
| **View** | **Refresh** (`F5`) reads the system configurations from disk again. |
| **Actions** | **Sign On...**, **Change Password...** and **Sign Off**, then one entry for each task: **5250 Display...**, **5250 Printer...**, **Data Transfer...**, **Integrated File System...**, **Messages...**, **Printer Output...**, **Run SQL...** and **Run Command...**. |
| **Tools** | **System Configurations...**, **Session Manager...** and **Certificate Management...**. |
| **Help** | **Help Topics** (`F1`) opens Help, and **About QS/400 Access for Desktop...** the About box. See [Help](#help). |

Everything in the **Actions** menu works on the system selected in the task pane.

### Toolbar

The toolbar has a button for **System Configurations**, then buttons for **5250 Display**, **Data Transfer**, **Integrated File System**, **Printer Output** and **Run SQL**.

### Task pane

The **System** list at the top of the task pane holds every configured system. Choose the system to work with there. The button beside the list opens System Configurations.

Below the list, the tasks are in groups:

| Group | Tasks |
|---|---|
| **General** | **Data Transfer**, **5250 Display**, **5250 Printer**, **Integrated File System**, **Messages**, **Printer Output**, **Run Command** |
| **Database** | **Schemas**, **Run SQL** |
| **Console** | **5250 Console**, **Control Panel** |
| **Management** | **System Configurations**, **Session Manager**, **Certificate Management** |

To start a task, click it, or move to it with the `Tab` and arrow keys and press `Enter`.

- A task that needs a session on the system shows the Sign On dialog first if you are not signed on. See [Signing on and passwords](signing-on.md).
- A task the selected system does not offer is dimmed. Its tip gives the reason, [QAD5001](messages-reference.md#QAD5001).
- **5250 Console** and **Control Panel** are not available yet. They report [QAD0001](messages-reference.md#QAD0001).
- If no system is configured, choosing a task opens System Configurations first.

### Status bar

The status bar shows the last message, and the state of the selected system:

- `name: Signed on as PROFILE` when you are signed on to it.
- `name: Not connected` when you are not.
- `No system configured` when the System list is empty.

While messages are waiting on a system that reports them, the status bar also shows **Messages waiting: N**. Click it to open Messages.

## System Configurations

Select **Tools > System Configurations...**, or **System Configurations** in the **Management** group. The dialog lists every system with the columns **Name**, **Description**, **TLS** and **Default user**.

| Button | What it does |
|---|---|
| **New...** | Opens **New System Configuration** to add a system. |
| **Edit...** | Opens **System Properties** for the selected system. A double-click does the same. |
| **Delete** | Removes the selected system, after asking. |
| **Verify...** | Checks the connection to the selected system. See [Verify Connection](verify-connection.md). |
| **Import...** | Reads system configurations from a `.qsc` file. When a system of the same name exists, choose **Replace** or **Keep**. |
| **Export...** | Writes every system configuration to a `.qsc` file. |
| **Close** | Closes the dialog. |

Import and export files never hold passwords.

## System Properties

The dialog for a new or an existing system has four tabs. **OK** checks the values and saves them; when a value is not valid, the tab that holds it is shown with the reason.

### General

- **System name**: the host name or IP address of the QS/400 system. Two systems cannot have the same name.
- **Description**: free text, shown in the list.
- **Use TLS for all connections**: 5250 sessions to this system use TLS.

### Connection

- **Default user ID**: the user ID the Sign On dialog starts with. Passwords are never saved.
- **Look up address**: **Always** looks the name up for every connection, **Daily** at most once a day, and **Never** uses the fixed address instead.
- **Fixed IP address**: enabled, and required, when **Look up address** is **Never**.

### Ports

| Service | Port | TLS port |
|---|---|---|
| Telnet | 23 | 992 |
| QAP | (none) | 2320 |

Telnet carries 5250 display and printer sessions; they use the TLS port when **Use TLS for all connections** is selected. QAP carries sign-on and every other task, and always uses TLS. The QAP port must differ from both Telnet ports. **Reset to defaults** puts the numbers above back.

### Security

**Trusted CA certificates** lists the certificates this system's connections trust, in addition to those the computer trusts: each certificate's subject and its **Valid to** date, which says **Expired** in red once it has passed.

- **Add...** takes a certificate file, PEM or DER. Choose **Reference** to keep the path of the file, or **Embed** to copy its certificates into the system configuration. An embedded certificate travels with exported configurations. A file with no certificate in it is refused with [QAD0043](messages-reference.md#QAD0043).
- **Remove** takes the selected certificate out of the list.
- **View...** shows every field of the selected certificate and its PEM text, as Certificate Management does.

A file that is referenced and cannot be read, or holds no certificate, is listed by its path with the reason in red, so that the reference can be removed.

When a system presents a certificate that is not trusted, **Trust for this system** in the **Certificate Not Trusted** dialog adds it to this list. [Certificate Management](certificate-management.md) shows the same lists for every system.

## Help

Help is part of the program. It opens no browser and needs no network.

- **Help > Help Topics** in the main window, or `F1`, opens Help at this topic.
- `F1` in any other window or dialog opens the topic for that window, and so does the **Help** button of a dialog that has one.
- Two windows keep `F1` for the system. In a 5250 display session `F1` is the system's F1 key; select **Help > Help Topics** there. In the Prompt dialog of Run Command, `F1` shows the system's help for the field.
- In a message box, **Details...** shows the message's cause and recovery. The message ID there opens the message in the [Messages reference](messages-reference.md).

The Help window has the topic on the right and two tabs on the left:

- **Contents** lists every topic. Select one to show it.
- **Search** lists the topics whose title or text contains exactly what you type, in any mix of capitals and small letters. Select a topic in the list to show it at the first place the text is found.

Above them, **Back** returns to the topic shown before, **Forward** undoes Back, and **Home** shows this topic. **Print...** writes the topic shown to a PDF file, which you can then print.

## See also

- [Signing on and passwords](signing-on.md)
- [Verify Connection](verify-connection.md)
- [Session Manager and session files](session-manager.md)
- [Certificate Management](certificate-management.md)
- [Installing, repairing and uninstalling](installing.md)
- [Keyboard reference](keyboard-reference.md)
