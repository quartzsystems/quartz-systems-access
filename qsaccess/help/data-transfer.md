# Data Transfer and transfer requests

**Data Transfer** moves rows between a table or query on a system and a file on this computer. Choose **Data Transfer** in the **General** group of the main window's task pane, then **Transfer from System...** or **Transfer to System...** in the menu that opens. In Schemas, **Transfer Data from...** and **Transfer Data to...** open the same windows with the table filled in. If the program has no session on the selected system, it asks you to sign on first (see [Signing on](signing-on.md)).

Data Transfer needs QAP version 4. For a system with an older version the task is dimmed, and the reason is [QAD5001](messages-reference.md#QAD5001).

## Transfer from System

The **Data Transfer from System** window has three tabs.

### Source

- **A table**: type the **Schema** and **Table**, or select **Browse...** to choose a schema and one of its tables. **Where** takes a condition for the rows, without the word `WHERE`; leave it blank for every row.
- **An SQL query**: type the query. **Check** has the system prepare the query without running it, and the status line gives the number of columns.

### Columns

**Get Columns** reads the source's columns into **Available**. Select columns and **Add >** to put them in **Columns to write**; **< Remove**, **Up** and **Down** change that list and its order. When nothing is chosen, every column is written. **Row limit** stops the transfer after that many rows; leave it blank for every row.

### Output

**Format** is CSV, TSV, XLSX, ODS or fixed width. The options under it change with the format:

| Format | Options |
| --- | --- |
| CSV | **Delimiter** and **Quote**, one character each, and **Encoding**: UTF-8 or UTF-8 with BOM. |
| TSV | As CSV, with the delimiter fixed as a tab. |
| XLSX, ODS | **Sheet name**. |
| Fixed width | **Widths**: select a column, type its width in characters and select **Set**. |

- **Column names in the first row** writes a header row. Fixed width has none.
- **Dates** writes dates, times and timestamps in ISO 8601 or in the job's date format. **Decimal point** is a period or a comma.
- **File** names the output file; **Browse...** chooses it.
- **If it is there** says what to do with an existing file: **Replace it**, **Ask** or **Append to it**. Appending works for CSV, TSV and fixed width only.

Decimals are written exactly. In CSV and TSV a NULL is an empty field and an empty string is `""`. A spreadsheet number cell is used only when it keeps the value exactly; otherwise the value is a text cell. In fixed width each column that allows NULL is followed by a `Y` or `N` indicator. Binary data is written as hex.

## Transfer to System

The **Data Transfer to System** window has four tabs.

### Input

Type the **File** or select **Browse...**, which also sets **Format** from the file's extension. Set the **Sheet** of a spreadsheet, the **Encoding** of a text file (UTF-8, UTF-16 or Windows-1252), the **Delimiter** and **Quote**, and whether the file has **Column names in the first row**. **Preview** shows the first 50 rows; a NULL shows as `-`. A fixed-width file is read by the layout of the target table's columns.

### Target

Type the **Schema** and **Table**, or select **Browse...**. Then choose a mode:

- **Append the rows to the table**.
- **Replace the table's rows (DELETE, then insert)**. Without commitment control the delete cannot be undone, and the program asks before it runs.

### Mapping

**Get Columns** reads the target's columns into the grid, which has the columns **Target column**, **Type**, **Source** and **First problem**.

- **Map by Name** matches source columns to target columns by name; **Map by Position** matches them in order. With nothing set, the program maps by name when the file has a header row and by position when it has none.
- To map one column by hand, select its row, choose a column in **Source** and select **Set**. `(default)` leaves the column unmapped, and it gets its default value.
- **First problem** shows, in red, the first value in the preview that does not fit the column's type: [QAD7035](messages-reference.md#QAD7035).

### Commit

- **One transaction: a failure keeps nothing** is the default. The rows are committed at the end.
- **Commit every** N **batches**: a failure keeps the batches already committed.
- **Without commitment control**: a failure leaves every earlier row in the table.

**Resume from row** starts at that data row; leave it blank to start at the first. **Dates** and **Decimal point** say how the file writes them. **CCSID 65535 data** is read as hex or as text in the job's CCSID.

## Running a transfer

Select **Run**. Each run has a connection of its own; when the session has no more, the program shows [QAD7003](messages-reference.md#QAD7003). A progress window shows the rows moved and the time taken, with **Cancel**.

The run ends with a summary in a dialog and on the status line:

| Message | Meaning |
| --- | --- |
| [QAD7030](messages-reference.md#QAD7030) | The transfer is complete. |
| [QAD7031](messages-reference.md#QAD7031) | A transfer to the system failed. It names the row, its line in the file and the system's message. |
| [QAD7036](messages-reference.md#QAD7036) | A transfer from the system failed. It gives the reason. The output file is as it was before the transfer. |
| [QAD7032](messages-reference.md#QAD7032) | The transfer was cancelled and nothing was kept. |

A download is written to a temporary file beside the output and renamed at the end, so a cancelled or failed download leaves no partial file.

When a failed or cancelled upload has kept rows, the summary adds [QAD7033](messages-reference.md#QAD7033) and offers **Resume from Row** with the first row not kept. Select it, correct the file if needed, and select **Run**.

When the table cannot be changed under commitment control, the first run stops with the system's message and [QAD7034](messages-reference.md#QAD7034). Select **Upload Without Commitment Control** to go on.

## Transfer requests

**Save Request...** saves the transfer as a `.qdx` file and reports [QAD7040](messages-reference.md#QAD7040). A request holds the system's name and everything set in the tabs, including the row to resume from. It never holds a password. A file in the request's folder, or in a folder under it, is saved as a relative path.

To open a request, select **Open Request...** in either window, choose **File > Open...** in the main window, or double-click the file. The window for its direction opens, filled in and ready to run. A request for a system that is not configured here gives [QAD2003](messages-reference.md#QAD2003).

To run a request without its window, see [Running a request with qsaccess run](qsaccess-run.md).

## See also

- [Running a request with qsaccess run](qsaccess-run.md)
- [Run SQL](run-sql.md)
- [Schemas](schemas.md)
- [Integrated File System, transfers and the editor](integrated-file-system.md)
- [Messages reference](messages-reference.md)
