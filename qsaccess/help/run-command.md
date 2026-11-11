# Run Command and prompting

**Run Command** runs CL commands on a system and shows every message each command sent. Choose **Run Command** in the **General** group of the main window's task pane. If the program has no session on the selected system, it asks you to sign on first (see [Signing on](signing-on.md)).

Run Command needs QAP version 2. On a system with an older version the window opens with everything but **Close** disabled, and the status line shows [QAD5001](messages-reference.md#QAD5001).

## The window

From top to bottom the window has:

- **Command**: the command line.
- The buttons **Prompt**, **Check**, **Run**, **Cancel** and **Clear**.
- The job line, which names the job the commands run in: its name, user and number.
- The results list.
- The status line and **Close**.

## Running a command

1. Type a command in **Command**, for example `CRTLIB LIB(TESTLIB)`.
2. Select **Run** or press `Enter`.

While the command runs, the status line says so and only **Cancel** and **Close** can be used. **Cancel** asks the system to end the command; what the command sent before it ended stays in the results, with the message about the cancel last.

When the command ends, the status line says how:

| Message | Meaning |
| --- | --- |
| [QAD5010](messages-reference.md#QAD5010) | The command completed. |
| [QAD5011](messages-reference.md#QAD5011) | The command ended with an error. |
| [QAD5012](messages-reference.md#QAD5012) | The command was cancelled. |
| [QAD5013](messages-reference.md#QAD5013) | The command was not found. |

If you are not authorised to the command or to an object it uses, the status line shows the system's own message instead.

**Check** checks the command without running it. The results show `Check:` and the command, then any messages; the status line says that the command has no errors, or shows [QAD5020](messages-reference.md#QAD5020).

## The results

Each run adds to the results list, which has the columns **ID**, **Severity** and **Text**:

- A line starting with `>` shows the command that was run.
- After it comes every message the command sent, as the system sent it and in the order it was sent. The list does not sort.
- **Severity** shows the system's two digits. From severity 20 the message ID is red.

Double-click a message to open its details, with the system's second-level text.

**Clear** empties the results.

## The history

The program keeps the last 50 commands you ran on each system. In **Command**, press `Up` for an earlier command and `Down` for a later one.

A command that gives a password is never kept in the history, and the program's log shows it with the password masked.

## Prompting a command

1. Type a command name in **Command**. A library can go in front, as in `QGPL/MYCMD`. Any parameters already typed are carried into the fields.
2. Select **Prompt** or press `F4`.

The Prompt dialog's title is the command's description and name. It shows one row for each parameter: its prompt text, its keyword, and a field for the value.

- A parameter that needs a value has its prompt text in bold.
- Defaults are filled in.
- Where the command allows only certain values, or has special values such as `*LIBL`, the field is a list you can also type in.
- The parts of a qualified name, for example an object and its library, are indented rows under the parameter.
- The elements of a parameter with several parts are an indented group of rows.
- A parameter that takes a list shows one entry and a **+** button. Select **+** to add another entry, up to the number the command allows.

**Additional Parameters (F10)** shows the parameters that are seldom needed, under the heading **Additional Parameters**; the button then reads **Hide Additional (F10)**. Press `F10` for the same. The button is unavailable when the command has no such parameters.

**Show Command String** writes the command your values make into **Command string**. Only parameters that differ from their defaults are written. Values that need quotes get them.

### Help in the Prompt dialog

- Press `F1` in a field, or select the **?** button at the end of its row, for help on that value.
- **Help** shows help for the whole command. `F1` does the same when the cursor is not in a field.

The help is the system's own text for the command or the value, followed by what the value may be: its kind and length, whether it is required, its default, its special values, the values allowed, its range and how many values it takes. When the system has no help text for it, the help says so and still shows what the value may be.

### OK and the checked command

**OK** has the system check the command first.

- If the command is valid, the dialog closes and the command is placed in **Command**, ready to run. Nothing has been run yet.
- If it has errors, the dialog stays open. The messages from the check are listed under **Command string**, and the status line shows [QAD5020](messages-reference.md#QAD5020), or [QAD5013](messages-reference.md#QAD5013) when the command does not exist. Double-click a message for its details, correct the value and select **OK** again.

**Cancel** closes the dialog and leaves **Command** as it was.

## Its own connection

On a system with QAP version 4 or later, each Run Command window has a connection of its own, so a long command does not hold up other windows. The title and the job line name the window's job, and the status line shows [QAD7001](messages-reference.md#QAD7001). On an older system commands run on the session's shared connection, and the status line shows [QAD7002](messages-reference.md#QAD7002).

- A session allows a limited number of connections. When there are no more, the program shows [QAD7003](messages-reference.md#QAD7003); close another window and try again.
- If the connection is lost while a command runs, the status line shows [QAD7010](messages-reference.md#QAD7010). The program does not run the command again by itself.
- If the session ends, the window shows the system's message and offers to sign on again.

Closing the window closes its connection. It does not sign you off.

## See also

- [Signing on and passwords](signing-on.md)
- [Run SQL](run-sql.md)
- [Printer Output and the viewer](printer-output.md)
- [Keyboard reference](keyboard-reference.md)
- [Messages reference](messages-reference.md)
