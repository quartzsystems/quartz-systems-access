# Run SQL

**Run SQL** runs SQL statements on a system and shows their results and messages. Choose **Run SQL** in the **Database** group of the main window's task pane. If the program has no session on the selected system, it asks you to sign on first (see [Signing on](signing-on.md)).

Run SQL needs QAP version 4. On a system with an older version the window does not open, and the program shows [QAD5001](messages-reference.md#QAD5001).

## The window

From top to bottom the window has:

- The menu bar: **File**, **Edit**, **Run**, **Results** and **Options**.
- The options row: **Run**, **Cancel**, **Schema**, **Naming**, **Commit level**, **Commit**, **Rollback** and **Clear Results**.
- The script editor.
- The tabs: **Messages**, and one tab for each result.
- The status bar: a message, the cursor's line and column in the script, and the system with the profile signed on.

## Writing a script

Type one or more statements in the editor and end each with `;`. A `;` inside a string, a quoted name or a comment does not end a statement. Comments are `--` to the end of the line, or `/*` to `*/`.

The options row sets how statements run:

- **Schema** is the schema that unqualified names are in. Pick one of the system's schemas or type a name and press `Enter`. Leave it blank for the default.
- **Naming** is **SQL** or **System**.
- **Commit level** is described under [Commitment control](#commitment-control).

## Running statements

| To run | Do this |
| --- | --- |
| The statement the cursor is in | Select **Run**, choose **Run > Run Statement**, or press `Ctrl+Enter` |
| The statements in the selected text | Choose **Run > Run Selected** or press `Ctrl+Shift+Enter` |
| Every statement in the script | Choose **Run > Run All** or press `F5` |

Statements run one after another, and the status bar says which one is running and its line in the script. When **Run > Stop at First Error** is ticked, the run ends at the first statement that fails; otherwise it carries on with the next.

**Run > History...** lists the last 100 statements run on the system, newest first. Select one and choose **Insert**, or double-click it, to put it into the script at the cursor. A statement that contains a password is not kept.

## Results

Each query gets a tab of its own, named **Result 1**, **Result 2** and so on.

- The column headings are the SQL column names. Rest the pointer on a heading to see the column's label and type.
- A null value shows as a grey `-`.
- Rows arrive 500 at a time. The line under the grid says how many rows are in and whether there are more. **Fetch More** gets the next 500 and **Fetch All** gets the rest.
- **Options > Show CCSID 65535 Data as Text** shows such data as text in the job's CCSID instead of hexadecimal. The choice is kept for each system.

**Results > Clear Results**, or **Clear Results** in the options row, removes every result tab and the messages.

### Copying and exporting

- Select a row, then choose **Edit > Copy Cell** (`Ctrl+Shift+C`) or **Edit > Copy Row** (`Ctrl+Shift+R`). A right-click on a row offers the same. A row is copied as text with a tab between values.
- **Results > Export...** (`Ctrl+E`) writes the rows fetched so far of the result shown to a file. The file's extension chooses the format: `.csv`, `.tsv`, `.xlsx`, `.ods` or `.txt` for fixed width.

## Messages

The **Messages** tab lists a line for every statement, as the system sent it. The columns are **#** (the statement's number in the run), **Line** (its line in the script), **ID**, **SQLSTATE**, **SQLCODE**, **Rows** (rows changed or fetched), **Elapsed** and **Text**.

The ID of a failed statement is red, and the status bar shows its message. Double-click a line to open the message details with the system's second-level text.

## Cancelling

**Cancel**, or **Run > Cancel**, asks the system to end the statement or the fetch that is running. A cancelled fetch keeps the rows already received, and a cancelled run stops there.

A result that was cut short keeps its rows. Its tab is marked **Incomplete** with the reason, and the status bar shows [QAD7011](messages-reference.md#QAD7011). You can still copy or export it.

## Commitment control

**Commit level** is **None** at first, which means every change is permanent at once. Choose **Read uncommitted**, **Read committed**, **Repeatable read** or **Serializable** to run under commitment control. **Commit** and **Rollback** are then available:

- **Commit** makes the changes since the last commit or rollback permanent.
- **Rollback** undoes them.

While there are uncommitted changes, the program asks what to do with them before it lets them be lost. This happens when you close the window, and when you change **Commit level**, **Naming** or **Schema**. It shows [QAD7021](messages-reference.md#QAD7021) with three choices:

- **Commit** commits the changes and carries on.
- **Roll Back** rolls them back and carries on.
- **Cancel** leaves everything as it was; the window stays open, or the option goes back to its old value.

## Opening and saving scripts

- **File > New** (`Ctrl+N`) starts an empty script.
- **File > Open...** (`Ctrl+O`) opens a `.sql` file.
- **File > Save** (`Ctrl+S`) and **File > Save As...** save the script. A name given without an extension gets `.sql`.
- **File > Recent Scripts** lists up to 10 scripts opened or saved lately.

If the script has changes that are not saved, the program asks whether to save them before it replaces the script or closes the window: **Save**, **Don't Save** or **Cancel**.

## Its own connection

Each Run SQL window has a connection of its own to the system, with its own job. A long query in one window does not hold up another window. The window's title names the system and the job, and the status bar shows [QAD7001](messages-reference.md#QAD7001) when the connection is made.

- If the connection is lost, what is shown stays, and the status bar and the **Messages** tab show [QAD7010](messages-reference.md#QAD7010). Nothing is run again by itself. The next run makes a new connection.
- If the session ends, the window shows the system's message and offers to sign on again.

Closing the window closes its connection. It does not sign you off.

## See also

- [Schemas](schemas.md)
- [Data Transfer and transfer requests](data-transfer.md)
- [Run Command and prompting](run-command.md)
- [Signing on and passwords](signing-on.md)
- [Messages reference](messages-reference.md)
