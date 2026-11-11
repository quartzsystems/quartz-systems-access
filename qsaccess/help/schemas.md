# Schemas

**Schemas** shows what is in a system's database: its schemas, the tables, views and files in each, and their columns. From there you can look at a table's data, copy a name, or start a data transfer. Choose **Schemas** in the **Database** group of the main window's task pane. If the program has no session on the selected system, it asks you to sign on first (see [Signing on](signing-on.md)).

Schemas needs QAP version 4. On a system with an older version the window does not open, and the program shows [QAD5001](messages-reference.md#QAD5001).

## The window

The title is **Schemas** followed by the system's name. The window has:

- The menu bar: **File**, **Actions** and **View**.
- **Schema pattern** and **Go**.
- On the left, the tree.
- On the right, a heading and a grid that describe what is selected in the tree.
- Under the grid, the buttons **View Data**, **Copy Name**, **Transfer Data from...** and **Transfer Data to...**.
- The status bar, which shows the system and the profile signed on.

## Choosing which schemas are listed

**Schema pattern** decides which schemas the tree lists. It is an SQL `LIKE` pattern:

| Character | Stands for |
| --- | --- |
| `%` | Any characters, or none |
| `_` | Exactly one character |

The pattern is `%` at first, which lists every schema. To narrow the list:

1. Type a pattern in **Schema pattern**, for example `QS%` for the schemas whose names start with QS. Lower-case letters are treated as upper case.
2. Select **Go** or press `Enter`.

The status bar says how many schemas match the pattern.

**View > Refresh** or `F5` reads the schemas again with the pattern as it stands and closes the tree back to the list of schemas.

## The tree

The top entry is the system. Under it are the schemas, and each schema has five folders:

- **Tables**
- **Views**
- **Aliases**
- **Physical files**
- **Logical files**

Open a folder to list what it holds, and open a table, view, alias or file to list its columns. Each level is read from the system the first time you open it, so opening a large schema does not wait for everything inside it.

Schemas and the contents of a folder are read 100 at a time. When there are more, the last entry says how many are shown out of how many there are, for example `More schemas (100 of 240)...`. Select that entry to read the next 100.

## What the right side shows

Select an entry in the tree to see it described on the right.

| Selected | Heading | Grid |
| --- | --- | --- |
| The system | The system's name | Empty |
| A schema | The schema's name | Its **Name** and **Description** |
| A folder | The folder and its schema | **Shown**: how many of its entries have been read |
| A table, view, alias or file | Its kind, its schema and name, and its text | One row for each column |
| A column | The column and its table | The column's details |

For a table, view, alias or file, the grid has these columns:

| Column | Meaning |
| --- | --- |
| **Name** | The column's SQL name |
| **Label** | The column's label |
| **Type** | The data type |
| **Length** | The length |
| **Scale** | The number of decimal places |
| **Nullable** | **Yes** when the column can hold a null value, otherwise **No** |
| **CCSID** | The CCSID of character data; empty when there is none |

If your profile may not read an object, the program shows the system's message, as the system sent it, when you select or open the object. The object stays in the tree without columns.

## Actions

The actions are in the **Actions** menu, on the buttons under the grid, and in the menu that a right-click in the tree opens.

**View Data**, **Transfer Data from...** and **Transfer Data to...** need a table, view, alias or file to be selected. Selecting one of its columns counts as selecting it. With anything else selected the three buttons are unavailable, and choosing one of these actions from a menu makes the status bar ask you to select a table, view, alias or file.

### View Data

**View Data** (`Ctrl+V`) opens a [Run SQL](run-sql.md) window with a `SELECT * FROM` statement for the selected object and runs it at once. The first 1,000 rows are fetched; use **Fetch More** or **Fetch All** in that window for the rest.

The Run SQL window is an ordinary one: you can change the statement and run it again, and it has its own connection to the system.

### Copy Name

**Copy Name** (`Ctrl+C`) copies the name of the selected entry to the clipboard, written as an SQL statement needs it:

- For a schema, the schema's name.
- For a table, view, alias or file, its name with its schema in front.
- For a column, the column's name.

The status bar shows what was copied. Paste the name into a script in Run SQL or anywhere else.

### Transfer Data from and to

- **Transfer Data from...** opens [Data Transfer](data-transfer.md) to copy data from the system to this computer, with the selected object already filled in as the source.
- **Transfer Data to...** opens Data Transfer to copy data from this computer to the system, with the selected object already filled in as the target.

This only opens Data Transfer with the object filled in; the transfer itself is run from there.

## Closing

**File > Close** closes the window. It does not sign you off.

## See also

- [Run SQL](run-sql.md)
- [Data Transfer and transfer requests](data-transfer.md)
- [Signing on and passwords](signing-on.md)
- [Messages reference](messages-reference.md)
